use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use crossbeam_channel::{bounded, Sender};
use file_id::{get_file_id, FileId};

use crate::error::FileError;
use crate::model::ScanOptions;
use crate::walk_index::{DirFiles, DirRecord, FileStat, WalkIndex};

/// A file found during traversal, stat-ed but not yet read.
///
/// Stored as its parent directory (an `Arc` shared by every sibling) plus
/// its own file name, rather than one full-path allocation per file: on a
/// tree of ~1M files the repeated directory prefixes alone were enough to
/// get the process OOM-killed mid-walk (ADR-0033). Callers that need the
/// full path join it on demand with [`Self::path`].
pub(crate) struct Candidate {
    pub parent: Arc<Path>,
    pub name: Box<OsStr>,
    pub size: u64,
    pub file_id: FileId,
}

impl Candidate {
    /// The candidate's full path (`parent` joined with `name`).
    pub fn path(&self) -> PathBuf {
        self.parent.join(&*self.name)
    }
}

/// Candidates and errors queued between the walk's workers and the caller.
/// Bounded so a walk that outruns its consumer blocks instead of buffering
/// the tree in memory (ADR-0035).
const FOUND_QUEUE: usize = 1 << 14;

/// What a walk worker hands the consuming thread.
enum Found {
    Candidate(Candidate),
    Error(FileError),
}

/// Walks `root` in parallel on rayon's pool, one directory per task,
/// `stat`-ing every regular file it finds and handing each one to
/// `on_candidate` as soon as it's ready (ADR-0012). Candidates arrive
/// unordered, over a bounded queue.
///
/// With an `index` (ADR-0034, ADR-0036), a directory whose mtime matches
/// its record is neither listed nor are its files `stat`-ed: the record
/// supplies both. Every directory listed afresh is recorded for next time.
///
/// Symlinks are skipped unless `options.follow_symlinks` is set, and the
/// walk stays on the filesystem `root` lives on unless
/// `options.cross_filesystems` is set (ADR-0003). Per-file errors are
/// reported through `on_error` and otherwise ignored — one bad file never
/// aborts the whole traversal (ADR-0004).
#[tracing::instrument(skip(options, index, on_error, on_candidate), fields(root = %root.display()))]
pub(crate) fn traverse(
    root: &Path,
    options: &ScanOptions,
    index: Option<Arc<WalkIndex>>,
    mut on_error: impl FnMut(FileError),
    mut on_candidate: impl FnMut(Candidate),
) {
    let (tx, rx) = bounded::<Option<Found>>(FOUND_QUEUE);
    let mut candidate_count = 0u64;
    std::thread::scope(|scope| {
        scope.spawn(|| {
            walk(root, options, index.as_deref(), &tx);
            let _ = tx.send(None);
        });
        // `None` marks the end, since `tx` itself outlives the walk.
        for found in rx.iter().map_while(|found| found) {
            match found {
                Found::Candidate(candidate) => {
                    candidate_count += 1;
                    on_candidate(candidate);
                }
                Found::Error(err) => on_error(err),
            }
        }
    });
    tracing::debug!(candidates = candidate_count, "traversal finished");
}

/// Walks `root`, sending every candidate and error to `tx`. A file `root`
/// is its own only candidate.
fn walk(root: &Path, options: &ScanOptions, index: Option<&WalkIndex>, tx: &Sender<Option<Found>>) {
    let walker = Walker {
        options,
        index,
        root_device: get_file_id(root).ok().and_then(|id| device_component(&id)),
        tx,
    };
    let metadata = match fs::metadata(root) {
        Ok(metadata) => metadata,
        Err(err) => return walker.error(root, err),
    };
    if metadata.is_file() {
        if let (Some(parent), Some(name)) = (root.parent(), root.file_name()) {
            walker.send_file(&Arc::from(parent), name, stat_file(root));
        }
        return;
    }
    let ancestors = walker.ancestor(root, &metadata, None);
    rayon::scope(|scope| walker.visit(scope, Arc::from(root), metadata.modified().ok(), ancestors));
}

/// The directory chain from the root down to a directory, by file id: lets
/// a `follow_symlinks` walk refuse to descend into its own ancestor.
struct Ancestor {
    id: FileId,
    parent: Option<Arc<Ancestor>>,
}

impl Ancestor {
    fn contains(&self, id: &FileId) -> bool {
        &self.id == id
            || self
                .parent
                .as_ref()
                .is_some_and(|parent| parent.contains(id))
    }
}

/// What one directory holds that the walk cares about.
struct Listing {
    files: Vec<OsString>,
    /// Subdirectories to descend into, including followed symlinks to them.
    dirs: Vec<OsString>,
    /// Whether `dirs` is exactly the directory's real subdirectories, so it
    /// can be recorded (false when symlinks were followed).
    recordable: bool,
}

/// Shared, read-only state of one walk.
struct Walker<'a> {
    options: &'a ScanOptions,
    index: Option<&'a WalkIndex>,
    root_device: Option<u64>,
    tx: &'a Sender<Option<Found>>,
}

impl<'a> Walker<'a> {
    /// Handles one directory -- its files here, each subdirectory as its own
    /// task -- reusing its record when `mtime` matches.
    fn visit<'s>(
        &'s self,
        scope: &rayon::Scope<'s>,
        dir: Arc<Path>,
        mtime: Option<SystemTime>,
        ancestors: Option<Arc<Ancestor>>,
    ) where
        'a: 's,
    {
        let recorded = self
            .index
            .zip(mtime)
            .and_then(|(index, mtime)| index.lookup(&dir, mtime));
        let reusable = recorded
            .as_ref()
            .filter(|_| !self.options.follow_symlinks)
            .and_then(Listing::recorded);
        let listing = match reusable.map_or_else(|| self.list(&dir), Ok) {
            Ok(listing) => listing,
            Err(err) => return self.error(&dir, err),
        };

        let files = self.stat_files(&dir, &listing.files, recorded.as_ref());
        let mut subdirs = Vec::with_capacity(listing.dirs.len());
        for name in listing.dirs {
            subdirs.push(name.to_str().map(str::to_owned));
            let path = dir.join(&name);
            if self.excluded(&path) {
                continue;
            }
            if let Some((metadata, ancestors)) = self.enter(&path, ancestors.as_ref()) {
                let mtime = metadata.modified().ok();
                scope.spawn(move |scope| self.visit(scope, Arc::from(path), mtime, ancestors));
            }
        }

        let subdirs = listing
            .recordable
            .then(|| subdirs.into_iter().collect::<Option<Vec<_>>>())
            .flatten();
        let fresh = files.map(|files| DirRecord { files, subdirs });
        if let (Some(index), Some(mtime), Some(fresh)) = (self.index, mtime, fresh) {
            if recorded.as_ref() != Some(&fresh) {
                index.record(&dir, mtime, &fresh);
            }
        }
    }

    /// Reads `dir`'s listing, resolving symlinks when following them.
    fn list(&self, dir: &Path) -> io::Result<Listing> {
        let follow = self.options.follow_symlinks;
        let mut listing = Listing {
            files: Vec::new(),
            dirs: Vec::new(),
            recordable: !follow,
        };
        for entry in fs::read_dir(dir)? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    self.error(dir, err);
                    continue;
                }
            };
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(err) => {
                    self.error(&entry.path(), err);
                    continue;
                }
            };
            let (is_file, is_dir) = if file_type.is_symlink() {
                if !follow {
                    continue;
                }
                match fs::metadata(entry.path()) {
                    Ok(target) => (target.is_file(), target.is_dir()),
                    Err(err) => {
                        self.error(&entry.path(), err);
                        continue;
                    }
                }
            } else {
                (file_type.is_file(), file_type.is_dir())
            };
            if is_file {
                listing.files.push(entry.file_name());
            } else if is_dir {
                listing.dirs.push(entry.file_name());
            }
        }
        Ok(listing)
    }

    /// Stats and sends each unexcluded file, reusing `recorded` stats by
    /// name. Returns the directory's complete file stats, or `None` when any
    /// can't be recorded (an error, a non-UTF-8 name, or an exclusion).
    fn stat_files(
        &self,
        dir: &Arc<Path>,
        names: &[OsString],
        recorded: Option<&DirRecord>,
    ) -> Option<DirFiles> {
        let mut fresh = Some(DirFiles::with_capacity(names.len()));
        for name in names {
            let path = dir.join(name);
            if self.excluded(&path) {
                fresh = None;
                continue;
            }
            let key = name.to_str();
            let reused = recorded
                .zip(key)
                .and_then(|(recorded, key)| recorded.files.get(key).copied());
            let stat = reused.map_or_else(|| stat_file(&path), Ok);
            match (&stat, key, fresh.as_mut()) {
                (Ok(stat), Some(key), Some(files)) => {
                    files.insert(key.to_owned(), *stat);
                }
                _ => fresh = None,
            }
            self.send_file(dir, name, stat);
        }
        fresh
    }

    /// `stat`s subdirectory `path` and decides whether to descend into it:
    /// not onto another filesystem, nor (when following symlinks) into one
    /// of its own ancestors. Returns its metadata and ancestor chain.
    fn enter(
        &self,
        path: &Path,
        ancestors: Option<&Arc<Ancestor>>,
    ) -> Option<(fs::Metadata, Option<Arc<Ancestor>>)> {
        let metadata = match fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(err) => {
                self.error(path, err);
                return None;
            }
        };
        let id = file_id_of(path, &metadata).ok();
        let device = id.as_ref().and_then(device_component);
        if is_excluded_by_filesystem_boundary(
            self.options.cross_filesystems,
            self.root_device,
            device,
        ) {
            tracing::trace!(path = %path.display(), "skipped -- different filesystem");
            return None;
        }
        if let (Some(ancestors), Some(id)) = (ancestors, &id) {
            if ancestors.contains(id) {
                let err = io::Error::other("symlink loop: directory is its own ancestor");
                self.error(path, err);
                return None;
            }
        }
        let ancestors = self.ancestor(path, &metadata, ancestors.cloned());
        Some((metadata, ancestors))
    }

    /// `parent` extended by directory `path`, when following symlinks.
    fn ancestor(
        &self,
        path: &Path,
        metadata: &fs::Metadata,
        parent: Option<Arc<Ancestor>>,
    ) -> Option<Arc<Ancestor>> {
        if !self.options.follow_symlinks {
            return None;
        }
        let id = file_id_of(path, metadata).ok()?;
        Some(Arc::new(Ancestor { id, parent }))
    }

    /// Whether `path` is under one of the scan's `exclude_paths`
    /// (`DETECTION-SCAN-FILTERS`), so an excluded subtree is never read.
    fn excluded(&self, path: &Path) -> bool {
        self.options
            .exclude_paths
            .iter()
            .any(|excluded| path.starts_with(excluded))
    }

    /// Sends file `name` in `dir` as a candidate (if the scan filters keep
    /// it) or its `stat` error.
    fn send_file(&self, dir: &Arc<Path>, name: &OsStr, stat: io::Result<FileStat>) {
        match stat {
            Ok(stat) => {
                let candidate = candidate(dir, name, stat, self.options, self.root_device);
                if let Some(candidate) = candidate {
                    let _ = self.tx.send(Some(Found::Candidate(candidate)));
                }
            }
            Err(err) => self.error(&dir.join(name), err),
        }
    }

    fn error(&self, path: &Path, err: io::Error) {
        tracing::warn!(path = %path.display(), error = %err, "traversal error");
        let _ = self.tx.send(Some(Found::Error(FileError {
            path: Arc::from(path),
            source: err,
        })));
    }
}

impl Listing {
    /// `record`'s listing, if it holds one.
    fn recorded(record: &DirRecord) -> Option<Self> {
        let subdirs = record.subdirs.as_ref()?;
        Some(Self {
            files: record.files.keys().map(OsString::from).collect(),
            dirs: subdirs.iter().map(OsString::from).collect(),
            recordable: true,
        })
    }
}

/// File `name` in `dir` as a candidate, or `None` when a scan filter
/// excludes it.
fn candidate(
    dir: &Arc<Path>,
    name: &OsStr,
    stat: FileStat,
    options: &ScanOptions,
    root_device: Option<u64>,
) -> Option<Candidate> {
    if !size_allowed(stat.size, options) {
        tracing::trace!(dir = %dir.display(), ?name, size = stat.size, "skipped -- size filter");
        return None;
    }
    if !extension_allowed(Path::new(name), options) {
        tracing::trace!(dir = %dir.display(), ?name, "skipped -- extension filter");
        return None;
    }
    if is_excluded_by_filesystem_boundary(
        options.cross_filesystems,
        root_device,
        device_component(&stat.file_id),
    ) {
        tracing::trace!(dir = %dir.display(), ?name, "skipped -- different filesystem");
        return None;
    }
    Some(Candidate {
        parent: dir.clone(),
        name: name.into(),
        size: stat.size,
        file_id: stat.file_id,
    })
}

/// `stat`s one regular file, following a symlink to it.
fn stat_file(path: &Path) -> io::Result<FileStat> {
    let metadata = fs::metadata(path)?;
    Ok(FileStat {
        size: metadata.len(),
        file_id: file_id_of(path, &metadata)?,
    })
}

/// The file's identity for hardlink collapse. On Unix it's read straight
/// from the metadata just fetched (device + inode), saving a second `stat`
/// per file -- which matters on slow filesystems such as ntfs-3g/FUSE
/// mounts, where every `stat` is a userspace round trip. Elsewhere it falls
/// back to `file_id::get_file_id`, which has to open the file to read its
/// volume serial + file index.
#[cfg(unix)]
fn file_id_of(_path: &Path, metadata: &fs::Metadata) -> io::Result<FileId> {
    use std::os::unix::fs::MetadataExt;
    Ok(FileId::new_inode(metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn file_id_of(path: &Path, _metadata: &fs::Metadata) -> io::Result<FileId> {
    get_file_id(path)
}

/// Decides whether a candidate should be skipped for being on a different
/// filesystem/volume than the scan root (ADR-0003). `cross_filesystems`
/// disables the check entirely; if either device is unknown (lookup failed)
/// the candidate is never excluded on this basis, since we can't tell.
fn is_excluded_by_filesystem_boundary(
    cross_filesystems: bool,
    root_device: Option<u64>,
    entry_device: Option<u64>,
) -> bool {
    if cross_filesystems {
        return false;
    }
    match (root_device, entry_device) {
        (Some(root), Some(entry)) => root != entry,
        _ => false,
    }
}

/// Decides whether a file's size passes `options.min_size`/`max_size`
/// (`DETECTION-SCAN-FILTERS`). Both bounds are inclusive; either being
/// `None` disables that side of the check.
fn size_allowed(size: u64, options: &ScanOptions) -> bool {
    if let Some(min) = options.min_size {
        if size < min {
            return false;
        }
    }
    if let Some(max) = options.max_size {
        if size > max {
            return false;
        }
    }
    true
}

/// Decides whether a file's extension passes `options.include_extensions`/
/// `exclude_extensions` (`DETECTION-SCAN-FILTERS`). Extensions are compared
/// case-insensitively, without the leading `.`. A file with no extension is
/// excluded by a non-empty `include_extensions` (nothing to match) but never
/// excluded by `exclude_extensions` (nothing on the list can match it).
fn extension_allowed(path: &Path, options: &ScanOptions) -> bool {
    let extension = path.extension().and_then(|ext| ext.to_str());

    if let Some(include) = &options.include_extensions {
        if !include.is_empty() {
            match extension {
                Some(ext) => {
                    if !include
                        .iter()
                        .any(|allowed| allowed.eq_ignore_ascii_case(ext))
                    {
                        return false;
                    }
                }
                None => return false,
            }
        }
    }

    if let (Some(exclude), Some(ext)) = (&options.exclude_extensions, extension) {
        if exclude
            .iter()
            .any(|denied| denied.eq_ignore_ascii_case(ext))
        {
            return false;
        }
    }

    true
}

/// Extracts a comparable "which filesystem/volume is this on" value from a
/// [`FileId`], regardless of platform.
fn device_component(id: &FileId) -> Option<u64> {
    match id {
        FileId::Inode { device_id, .. } => Some(*device_id),
        FileId::LowRes {
            volume_serial_number,
            ..
        } => Some(*volume_serial_number as u64),
        FileId::HighRes {
            volume_serial_number,
            ..
        } => Some(*volume_serial_number),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Test helper matching `traverse`'s pre-streaming call shape: collect
    /// every candidate into a `Vec`, the way `pipeline::run_scan` did
    /// before folding the collapse step into `on_candidate` directly
    /// (DETECTION-STREAMING-OVERLAP).
    fn traverse_collect(
        root: &Path,
        options: &ScanOptions,
        on_error: impl FnMut(FileError),
    ) -> Vec<Candidate> {
        let mut candidates = Vec::new();
        traverse(root, options, None, on_error, |c| candidates.push(c));
        candidates
    }

    /// One walk of `root` with a walk index stored in `db`, returning
    /// `(file name, size)` sorted by name. Each call opens and finishes its
    /// own index, like one scan.
    fn walk_with_index(root: &Path, db: &Arc<redb::Database>, reuse: bool) -> Vec<(String, u64)> {
        walk_with_index_and(root, db, reuse, &ScanOptions::default())
    }

    fn walk_with_index_and(
        root: &Path,
        db: &Arc<redb::Database>,
        reuse: bool,
        options: &ScanOptions,
    ) -> Vec<(String, u64)> {
        let (index, writer) = WalkIndex::open(db.clone(), reuse).unwrap();
        let mut found = Vec::new();
        traverse(
            root,
            options,
            Some(Arc::new(index)),
            |_| {},
            |c| {
                found.push((c.name.to_string_lossy().into_owned(), c.size));
            },
        );
        writer.finish();
        found.sort();
        found
    }

    /// Backdates `path`'s mtime past the walk index's racy window, so a
    /// directory just populated by the test is eligible to be recorded.
    /// Always the same instant, so a re-backdated directory looks unchanged.
    fn backdate(path: &Path) {
        let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
        fs::File::open(path).unwrap().set_modified(old).unwrap();
    }

    #[test]
    fn walk_index_reuses_an_unchanged_directory() {
        let tree = tempfile::tempdir().unwrap();
        let db_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(redb::Database::create(db_dir.path().join("cache.redb")).unwrap());
        let file = tree.path().join("a.bin");
        fs::write(&file, b"four").unwrap();
        backdate(tree.path());
        assert_eq!(
            walk_with_index(tree.path(), &db, true),
            vec![("a.bin".into(), 4)]
        );

        // Rewritten in place: the directory's mtime doesn't move, so the
        // recorded size is reused -- the documented blind spot (ADR-0034),
        // which proves the second walk didn't stat the file...
        fs::write(&file, b"eight!!!").unwrap();
        backdate(tree.path());
        assert_eq!(
            walk_with_index(tree.path(), &db, true),
            vec![("a.bin".into(), 4)]
        );
        // ...and which a rescan-all walk clears.
        assert_eq!(
            walk_with_index(tree.path(), &db, false),
            vec![("a.bin".into(), 8)]
        );
        assert_eq!(
            walk_with_index(tree.path(), &db, true),
            vec![("a.bin".into(), 8)]
        );
    }

    #[test]
    fn walk_index_sees_files_added_to_a_recorded_directory() {
        let tree = tempfile::tempdir().unwrap();
        let db_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(redb::Database::create(db_dir.path().join("cache.redb")).unwrap());
        fs::write(tree.path().join("a.bin"), b"a").unwrap();
        backdate(tree.path());
        walk_with_index(tree.path(), &db, true);

        fs::write(tree.path().join("b.bin"), b"bb").unwrap();
        assert_eq!(
            walk_with_index(tree.path(), &db, true),
            vec![("a.bin".into(), 1), ("b.bin".into(), 2)]
        );
    }

    fn temp_db() -> (tempfile::TempDir, Arc<redb::Database>) {
        let db_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(redb::Database::create(db_dir.path().join("cache.redb")).unwrap());
        (db_dir, db)
    }

    #[test]
    fn walk_index_skips_listing_an_unchanged_directory() {
        let tree = tempfile::tempdir().unwrap();
        let (_db_dir, db) = temp_db();
        fs::write(tree.path().join("a.bin"), b"a").unwrap();
        backdate(tree.path());
        walk_with_index(tree.path(), &db, true);

        // Added behind the index's back: the directory's mtime is put back,
        // so a walk that still listed it would find the new file.
        fs::write(tree.path().join("b.bin"), b"bb").unwrap();
        backdate(tree.path());
        assert_eq!(
            walk_with_index(tree.path(), &db, true),
            vec![("a.bin".into(), 1)]
        );
        assert_eq!(
            walk_with_index(tree.path(), &db, false),
            vec![("a.bin".into(), 1), ("b.bin".into(), 2)]
        );
    }

    #[test]
    fn reused_directories_still_descend_into_changed_subdirectories() {
        let tree = tempfile::tempdir().unwrap();
        let (_db_dir, db) = temp_db();
        let deep = tree.path().join("x/y");
        fs::create_dir_all(&deep).unwrap();
        fs::write(tree.path().join("top.bin"), b"t").unwrap();
        fs::write(tree.path().join("x/mid.bin"), b"mm").unwrap();
        fs::write(deep.join("low.bin"), b"lll").unwrap();
        for dir in [deep.as_path(), &tree.path().join("x"), tree.path()] {
            backdate(dir);
        }
        assert_eq!(walk_with_index(tree.path(), &db, true).len(), 3);

        fs::write(deep.join("new.bin"), b"nnnn").unwrap();
        assert_eq!(
            walk_with_index(tree.path(), &db, true),
            vec![
                ("low.bin".into(), 3),
                ("mid.bin".into(), 2),
                ("new.bin".into(), 4),
                ("top.bin".into(), 1),
            ]
        );
    }

    #[test]
    fn exclude_paths_apply_to_a_reused_listing() {
        let tree = tempfile::tempdir().unwrap();
        let (_db_dir, db) = temp_db();
        let sub = tree.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(tree.path().join("kept.bin"), b"k").unwrap();
        fs::write(sub.join("inside.bin"), b"ii").unwrap();
        fs::write(tree.path().join("skip.bin"), b"sss").unwrap();
        backdate(&sub);
        backdate(tree.path());
        assert_eq!(walk_with_index(tree.path(), &db, true).len(), 3);

        let options = ScanOptions {
            exclude_paths: vec![sub, tree.path().join("skip.bin")],
            ..ScanOptions::default()
        };
        assert_eq!(
            walk_with_index_and(tree.path(), &db, true, &options),
            vec![("kept.bin".into(), 1)]
        );
        // The excluded walk must not have narrowed the record.
        assert_eq!(walk_with_index(tree.path(), &db, true).len(), 3);
    }

    #[test]
    fn a_file_root_is_its_own_only_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("only.bin");
        fs::write(&file, b"data").unwrap();
        fs::write(dir.path().join("other.bin"), b"other").unwrap();

        let candidates = traverse_collect(&file, &ScanOptions::default(), |_| {});
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].path(), file);
    }

    #[test]
    fn finds_regular_files_only() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"a").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/b.txt"), b"bb").unwrap();

        let options = ScanOptions::default();
        let candidates = traverse_collect(dir.path(), &options, |_| {});

        let mut sizes: Vec<u64> = candidates.iter().map(|c| c.size).collect();
        sizes.sort();
        assert_eq!(sizes, vec![1, 2]);
    }

    #[test]
    fn skips_symlinks_by_default() {
        #[cfg(unix)]
        {
            let dir = tempfile::tempdir().unwrap();
            fs::write(dir.path().join("real.txt"), b"data").unwrap();
            std::os::unix::fs::symlink(dir.path().join("real.txt"), dir.path().join("link.txt"))
                .unwrap();

            let options = ScanOptions::default();
            let candidates = traverse_collect(dir.path(), &options, |_| {});
            assert_eq!(candidates.len(), 1);
            assert_eq!(&*candidates[0].name, "real.txt");
        }
    }

    #[test]
    fn traversal_errors_are_reported_and_do_not_abort_the_scan() {
        #[cfg(unix)]
        {
            let dir = tempfile::tempdir().unwrap();
            fs::write(dir.path().join("real.txt"), b"data").unwrap();
            std::os::unix::fs::symlink(
                dir.path().join("does-not-exist"),
                dir.path().join("dangling.txt"),
            )
            .unwrap();

            // Broken symlinks are only ever stat-ed (and can only fail)
            // when we're configured to follow them.
            let options = ScanOptions {
                follow_symlinks: true,
                ..ScanOptions::default()
            };
            let mut errors = Vec::new();
            let candidates = traverse_collect(dir.path(), &options, |err| errors.push(err));

            assert_eq!(
                candidates.len(),
                1,
                "the broken symlink must not appear as a candidate"
            );
            assert_eq!(&*candidates[0].name, "real.txt");
            assert_eq!(
                errors.len(),
                1,
                "the broken symlink must be reported as a per-file error, not silently dropped"
            );
        }
    }

    /// A symlink cycle (a directory containing a symlink back to one of its
    /// own ancestors) with `follow_symlinks = true` must not hang the scan.
    /// This relies entirely on the walk's ancestor check (ADR-0036); this
    /// test's real job is proving that reliance is justified, with a
    /// bounded timeout so a regression fails the test instead of hanging
    /// the whole suite (and CI) forever.
    #[test]
    fn follow_symlinks_terminates_on_a_cycle() {
        #[cfg(unix)]
        {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().to_path_buf();
            fs::write(root.join("real.txt"), b"data").unwrap();
            fs::create_dir(root.join("sub")).unwrap();
            // sub/loop -> root, so descending into it forever would revisit
            // sub/loop/sub/loop/... without the ancestor check.
            std::os::unix::fs::symlink(&root, root.join("sub").join("loop")).unwrap();

            let options = ScanOptions {
                follow_symlinks: true,
                ..ScanOptions::default()
            };

            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let mut errors = Vec::new();
                let candidates = traverse_collect(&root, &options, |err| errors.push(err));
                let _ = tx.send((candidates.len(), errors.len()));
            });

            match rx.recv_timeout(std::time::Duration::from_secs(10)) {
                Ok((candidate_count, error_count)) => {
                    // real.txt should still be found exactly once despite
                    // the cycle -- the walk must not revisit it repeatedly --
                    // and the refused loop is reported, not hidden.
                    assert_eq!(candidate_count, 1);
                    assert_eq!(error_count, 1);
                }
                Err(_) => panic!(
                    "traverse() did not terminate within 10s on a symlink cycle -- \
                     the walk's loop detection regressed"
                ),
            }
        }
    }

    #[test]
    fn filesystem_boundary_is_not_enforced_when_cross_filesystems_is_set() {
        assert!(!is_excluded_by_filesystem_boundary(true, Some(1), Some(2)));
    }

    #[test]
    fn filesystem_boundary_excludes_a_different_device() {
        assert!(is_excluded_by_filesystem_boundary(false, Some(1), Some(2)));
    }

    #[test]
    fn filesystem_boundary_allows_the_same_device() {
        assert!(!is_excluded_by_filesystem_boundary(false, Some(1), Some(1)));
    }

    #[test]
    fn filesystem_boundary_is_not_enforced_when_a_device_is_unknown() {
        assert!(!is_excluded_by_filesystem_boundary(false, None, Some(2)));
        assert!(!is_excluded_by_filesystem_boundary(false, Some(1), None));
        assert!(!is_excluded_by_filesystem_boundary(false, None, None));
    }

    #[test]
    fn device_component_reads_the_device_id_on_unix() {
        let id = FileId::Inode {
            device_id: 42,
            inode_number: 7,
        };
        assert_eq!(device_component(&id), Some(42));
    }

    #[test]
    fn device_component_reads_the_volume_serial_number_on_windows() {
        let low_res = FileId::LowRes {
            volume_serial_number: 42,
            file_index: 7,
        };
        assert_eq!(device_component(&low_res), Some(42));

        let high_res = FileId::HighRes {
            volume_serial_number: 42,
            file_id: 7,
        };
        assert_eq!(device_component(&high_res), Some(42));
    }

    #[test]
    fn size_allowed_enforces_both_bounds_inclusively() {
        let options = ScanOptions {
            min_size: Some(10),
            max_size: Some(20),
            ..ScanOptions::default()
        };
        assert!(!size_allowed(9, &options));
        assert!(size_allowed(10, &options));
        assert!(size_allowed(20, &options));
        assert!(!size_allowed(21, &options));
    }

    #[test]
    fn size_allowed_with_no_bounds_allows_everything() {
        let options = ScanOptions::default();
        assert!(size_allowed(0, &options));
        assert!(size_allowed(u64::MAX, &options));
    }

    #[test]
    fn extension_allowed_include_list_is_case_insensitive_and_excludes_no_extension() {
        let options = ScanOptions {
            include_extensions: Some(vec!["JPG".to_string(), "png".to_string()]),
            ..ScanOptions::default()
        };
        assert!(extension_allowed(Path::new("a.jpg"), &options));
        assert!(extension_allowed(Path::new("a.PNG"), &options));
        assert!(!extension_allowed(Path::new("a.txt"), &options));
        assert!(!extension_allowed(Path::new("no_extension"), &options));
    }

    #[test]
    fn extension_allowed_exclude_list_never_excludes_a_missing_extension() {
        let options = ScanOptions {
            exclude_extensions: Some(vec!["tmp".to_string()]),
            ..ScanOptions::default()
        };
        assert!(!extension_allowed(Path::new("a.tmp"), &options));
        assert!(!extension_allowed(Path::new("a.TMP"), &options));
        assert!(extension_allowed(Path::new("no_extension"), &options));
        assert!(extension_allowed(Path::new("a.txt"), &options));
    }

    #[test]
    fn extension_allowed_exclude_wins_even_if_include_would_allow_it() {
        let options = ScanOptions {
            include_extensions: Some(vec!["txt".to_string()]),
            exclude_extensions: Some(vec!["txt".to_string()]),
            ..ScanOptions::default()
        };
        assert!(!extension_allowed(Path::new("a.txt"), &options));
    }

    #[test]
    fn min_size_filter_skips_small_files_during_a_real_traversal() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("small.txt"), b"a").unwrap();
        fs::write(dir.path().join("big.txt"), b"aaaaaaaaaa").unwrap();

        let options = ScanOptions {
            min_size: Some(5),
            ..ScanOptions::default()
        };
        let candidates = traverse_collect(dir.path(), &options, |_| {});

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].size, 10);
    }

    #[test]
    fn extension_filter_skips_non_matching_files_during_a_real_traversal() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.jpg"), b"a").unwrap();
        fs::write(dir.path().join("b.txt"), b"b").unwrap();

        let options = ScanOptions {
            include_extensions: Some(vec!["jpg".to_string()]),
            ..ScanOptions::default()
        };
        let candidates = traverse_collect(dir.path(), &options, |_| {});

        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn exclude_paths_prunes_the_whole_subtree_not_just_matching_files() {
        let dir = tempfile::tempdir().unwrap();
        let excluded_dir = dir.path().join("excluded");
        fs::create_dir(&excluded_dir).unwrap();
        fs::write(excluded_dir.join("inside.txt"), b"skip me").unwrap();
        fs::write(dir.path().join("kept.txt"), b"keep me").unwrap();

        let options = ScanOptions {
            exclude_paths: vec![excluded_dir],
            ..ScanOptions::default()
        };
        let candidates = traverse_collect(dir.path(), &options, |_| {});

        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].path().ends_with("kept.txt"));
    }

    #[test]
    fn exclude_paths_can_target_a_single_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("excluded.txt"), b"skip me").unwrap();
        fs::write(dir.path().join("kept.txt"), b"keep me").unwrap();

        let options = ScanOptions {
            exclude_paths: vec![dir.path().join("excluded.txt")],
            ..ScanOptions::default()
        };
        let candidates = traverse_collect(dir.path(), &options, |_| {});

        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].path().ends_with("kept.txt"));
    }
}
