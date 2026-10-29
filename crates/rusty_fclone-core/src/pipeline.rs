use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{bounded, unbounded, Receiver, RecvTimeoutError, Sender};
use rayon::prelude::*;

use crate::cache::{self, HashCache};
use crate::candidate_sort::{CandidateSorter, Sorted, DEFAULT_BUFFER as SORT_BUFFER};
use crate::device::default_io_threads;
use crate::error::{FileError, ScanError};
use crate::fclones_import::{self, FclonesImportCache};
use crate::hash::{hash_chunks, sample_ranges};
use crate::io_pool::{IoPool, IoStats};
use crate::model::{
    DuplicateGroup, HashProgress, ScanEvent, ScanOptions, ScanProgress, ScanSummary,
};
use crate::traversal::traverse;
use crate::walk_index::WalkIndex;

/// How often (in files scanned) to emit a [`ScanEvent::Progress`]
/// checkpoint during traversal (ADR-0015). A count-based threshold rather
/// than a wall-clock timer: deterministic, no extra dependency, and cheap
/// enough (a modulo check per file) not to matter next to the actual I/O.
const PROGRESS_INTERVAL_FILES: u64 = 256;

/// How often to emit a [`ScanEvent::HashProgress`] while hashing. Time-based
/// rather than count-based: on a slow disk a few hundred reads can take
/// minutes, and the point is to show the scan is alive.
const HASH_PROGRESS_INTERVAL: Duration = Duration::from_secs(1);

/// One distinct file, identified by its representative path (the first
/// alias, alphabetically) plus every path — including hardlink aliases —
/// that shares its content. `Arc<Path>` so the representative and each
/// alias can be cloned cheaply as they move through the size/partial/full
/// hash grouping stages below, instead of re-allocating a `PathBuf` at
/// every stage (ADR-0004's "path storage" note).
type FileGroup = (Arc<Path>, Vec<Arc<Path>>);

/// One file's full-hash result, plus whether (and what) to persist to the
/// hash cache -- `None` when caching is disabled or this was already a
/// cache hit (nothing new to write back).
struct FullHashResult {
    hash: u128,
    group: FileGroup,
    cache_write: Option<cache::FileStat>,
}

/// A running (or finished) scan. Yields [`ScanEvent`]s as they're found —
/// consumers don't wait for the whole tree to finish before seeing the
/// first duplicate group (ADR-0004).
///
/// Dropping a `ScanHandle` blocks until the background scan thread exits.
pub struct ScanHandle {
    events: Receiver<ScanEvent>,
    join: Option<JoinHandle<()>>,
}

impl ScanHandle {
    /// The receiving end of the event channel, for consumers that want
    /// `select!`/`try_recv` rather than plain iteration.
    pub fn events(&self) -> &Receiver<ScanEvent> {
        &self.events
    }
}

impl Iterator for ScanHandle {
    type Item = ScanEvent;

    fn next(&mut self) -> Option<ScanEvent> {
        self.events.recv().ok()
    }
}

impl Drop for ScanHandle {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Starts scanning `root` for duplicate files in the background, per
/// `options`. Returns immediately with a [`ScanHandle`] to consume as
/// results stream in.
pub fn scan(root: impl Into<PathBuf>, options: ScanOptions) -> Result<ScanHandle, ScanError> {
    let root = root.into();
    if !root.is_dir() {
        return Err(ScanError::InvalidRoot(root));
    }

    let (event_tx, event_rx) = unbounded();
    let join = std::thread::spawn(move || run_scan(root, options, event_tx));
    Ok(ScanHandle {
        events: event_rx,
        join: Some(join),
    })
}

#[tracing::instrument(skip(options, event_tx), fields(root = %root.display()))]
fn run_scan(root: PathBuf, options: ScanOptions, event_tx: Sender<ScanEvent>) {
    let mut summary = ScanSummary::default();

    // Opened before traversal (not after) so the file exists, and a bad
    // path or permissions problem surfaces, immediately rather than after a
    // potentially hours-long walk of a large tree (ADR-0016 addendum).
    let cache = options.cache_path.as_ref().and_then(|path| {
        match HashCache::open(path) {
            Ok(cache) => Some(cache),
            Err(err) => {
                tracing::warn!(path = %path.display(), error = %err, "failed to open hash cache, continuing without it");
                None
            }
        }
    });

    // Traversal only records compact candidates (shared parent dir + file
    // name); full paths are materialized afterwards, and only for files
    // whose size is shared by another file -- on a typical tree most sizes
    // are unique, so most files never get a full-path allocation at all.
    let walk_index = cache.as_ref().and_then(|cache| {
        match WalkIndex::open(cache.database(), !options.rescan_all) {
            Ok((index, writer)) => Some((Arc::new(index), writer)),
            Err(err) => {
                tracing::warn!(error = %err, "failed to open walk index, walking without it");
                None
            }
        }
    });
    let (index, index_writer) = walk_index.unzip();

    let spill_dir = spill_dir(&options);
    let mut sorter = CandidateSorter::new(spill_dir.clone(), SORT_BUFFER);
    let mut spill_failed = false;
    traverse(
        &root,
        &options,
        index.clone(),
        |err| {
            let _ = event_tx.send(ScanEvent::Error(err));
        },
        |candidate| {
            summary.files_scanned += 1;
            summary.bytes_scanned += candidate.size;
            if let Err(err) = sorter.push(candidate) {
                if !spill_failed {
                    tracing::warn!(error = %err, "failed to spill candidates to disk, keeping them in memory");
                }
                spill_failed = true;
            }
            if summary.files_scanned % PROGRESS_INTERVAL_FILES == 0 {
                let _ = event_tx.send(ScanEvent::Progress(ScanProgress {
                    files_scanned: summary.files_scanned,
                    bytes_scanned: summary.bytes_scanned,
                }));
            }
        },
    );

    tracing::info!(
        files_scanned = summary.files_scanned,
        bytes_scanned = summary.bytes_scanned,
        "traversal complete"
    );
    if let Some(index) = &index {
        let (reused, walked) = index.reuse_counts();
        tracing::info!(reused, walked, "walk index: directories reused vs. stat-ed");
    }
    if let Some(writer) = index_writer {
        writer.finish();
    }

    let sorted = match sorter.finish() {
        Ok(sorted) => sorted,
        Err(err) => {
            report_spill_error(&event_tx, &spill_dir, err);
            let _ = event_tx.send(ScanEvent::Finished(summary));
            return;
        }
    };
    let spill_tx = event_tx.clone();
    let candidate_groups = SizeGroups::new(sorted.map_while(move |record| {
        record
            .map_err(|err| report_spill_error(&spill_tx, &spill_dir, err))
            .ok()
    }));

    let io_threads = options.io_threads.unwrap_or_else(|| {
        let detected = default_io_threads(&root);
        tracing::debug!(
            io_threads = detected,
            "auto-detected io_threads from device type"
        );
        detected
    });
    let io_pool = IoPool::new(io_threads);
    let duplicate_groups = AtomicU64::new(0);
    let duplicate_files = AtomicU64::new(0);

    let fclones_import = options
        .fclones_import_path
        .as_ref()
        .and_then(|path| FclonesImportCache::open(path));

    let io_stats = io_pool.stats();
    let (stop_progress, progress_stopped) = bounded::<()>(0);
    std::thread::scope(|scope| {
        scope.spawn(|| report_hash_progress(&io_stats, &progress_stopped, &event_tx));
        candidate_groups.par_bridge().for_each(|(size, members)| {
            for group in process_size_group(
                size,
                members,
                &io_pool,
                &options,
                cache.as_ref(),
                fclones_import.as_ref(),
                &event_tx,
            ) {
                duplicate_groups.fetch_add(1, Ordering::Relaxed);
                duplicate_files.fetch_add(group.paths.len() as u64, Ordering::Relaxed);
                let _ = event_tx.send(ScanEvent::DuplicateGroup(group));
            }
        });
        drop(stop_progress);
    });

    summary.duplicate_groups = duplicate_groups.load(Ordering::Relaxed);
    summary.duplicate_files = duplicate_files.load(Ordering::Relaxed);
    tracing::info!(
        duplicate_groups = summary.duplicate_groups,
        duplicate_files = summary.duplicate_files,
        "scan finished"
    );
    let _ = event_tx.send(ScanEvent::Finished(summary));
}

/// Where sorted candidate runs are spilled: next to the hash cache when
/// there is one (so a tmpfs `/tmp` isn't filled with them), otherwise the
/// system temp directory.
fn spill_dir(options: &ScanOptions) -> PathBuf {
    options
        .cache_path
        .as_deref()
        .and_then(Path::parent)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map_or_else(std::env::temp_dir, Path::to_path_buf)
}

/// Emits [`ScanEvent::HashProgress`] now, every [`HASH_PROGRESS_INTERVAL`],
/// and once more with the final totals when `stop` disconnects.
fn report_hash_progress(stats: &IoStats, stop: &Receiver<()>, event_tx: &Sender<ScanEvent>) {
    let emit = || {
        let (files_read, bytes_read) = stats.totals();
        let _ = event_tx.send(ScanEvent::HashProgress(HashProgress {
            files_read,
            bytes_read,
        }));
    };
    emit();
    while let Err(RecvTimeoutError::Timeout) = stop.recv_timeout(HASH_PROGRESS_INTERVAL) {
        emit();
    }
    emit();
}

fn report_spill_error(event_tx: &Sender<ScanEvent>, spill_dir: &Path, source: io::Error) {
    tracing::error!(error = %source, "reading spilled candidates failed, results are incomplete");
    let _ = event_tx.send(ScanEvent::Error(FileError {
        path: Arc::from(spill_dir),
        source,
    }));
}

/// Size-groups of 2+ distinct files, from candidates sorted by
/// `(size, file_id)`, one group at a time: hardlink aliases (same
/// `FileId`) are collapsed into one [`FileGroup`], since they already share
/// storage and only one representative per id needs hashing. Sizes held by
/// a single distinct file can't be duplicates and are skipped.
///
/// Lazy, so only the groups being hashed hold full paths (ADR-0035).
struct SizeGroups<I: Iterator<Item = Sorted>> {
    sorted: std::iter::Peekable<I>,
}

impl<I: Iterator<Item = Sorted>> SizeGroups<I> {
    fn new(sorted: I) -> Self {
        Self {
            sorted: sorted.peekable(),
        }
    }

    /// Every record of the next size, in order.
    fn next_size(&mut self) -> Option<Vec<Sorted>> {
        let first = self.sorted.next()?;
        let size = first.size;
        let mut same_size = vec![first];
        while let Some(record) = self.sorted.next_if(|r| r.size == size) {
            same_size.push(record);
        }
        Some(same_size)
    }
}

impl<I: Iterator<Item = Sorted>> Iterator for SizeGroups<I> {
    type Item = (u64, Vec<FileGroup>);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let same_size = self.next_size()?;
            let members: Vec<FileGroup> = same_size
                .chunk_by(|a, b| a.file_id == b.file_id)
                .map(file_group)
                .collect();
            if members.len() > 1 {
                return Some((same_size[0].size, members));
            }
        }
    }
}

/// One distinct file's [`FileGroup`] from every record sharing its
/// `FileId` (its hardlink aliases), with the alphabetically-first alias
/// as representative.
fn file_group(aliases: &[Sorted]) -> FileGroup {
    let mut paths: Vec<Arc<Path>> = aliases
        .iter()
        .map(|r| Arc::from(r.path.as_path()))
        .collect();
    paths.sort();
    (paths[0].clone(), paths)
}

/// Runs the staged-hashing pipeline (ADR-0001) for one size-group: partial
/// hash to prune, full hash to confirm, optional byte-verify — returning
/// every subgroup that survives as a [`DuplicateGroup`]. `cache`, when
/// present, lets the full-hash stage reuse a previous scan's hash for a
/// file whose `(size, mtime)` haven't changed (ADR-0016). `fclones_import`,
/// when present, is tried after a `cache` miss, before any real hashing
/// (ADR-0019).
#[tracing::instrument(skip(members, io_pool, options, cache, fclones_import, event_tx), fields(members = members.len()))]
fn process_size_group(
    size: u64,
    members: Vec<FileGroup>,
    io_pool: &IoPool,
    options: &ScanOptions,
    cache: Option<&HashCache>,
    fclones_import: Option<&FclonesImportCache>,
    event_tx: &Sender<ScanEvent>,
) -> Vec<DuplicateGroup> {
    if size == 0 {
        // Empty files are identical by definition: reading them proves
        // nothing and, on a slow disk, costs a seek per file.
        let mut paths: Vec<Arc<Path>> = members
            .into_iter()
            .flat_map(|(_, aliases)| aliases)
            .collect();
        paths.sort();
        return vec![DuplicateGroup { size, paths }];
    }
    let small_file = size <= options.small_file_threshold;

    let full_hash_input: Vec<FileGroup> = if small_file {
        members
    } else {
        let partial: Vec<(u128, FileGroup)> = members
            .into_par_iter()
            .filter_map(|(representative, aliases)| {
                let ranges = sample_ranges(size, options.partial_hash_sample_size);
                match io_pool.read_ranges(&representative, ranges) {
                    Ok(bytes) => Some((hash_chunks(&[&bytes]), (representative, aliases))),
                    Err(source) => {
                        report_error(event_tx, representative, source);
                        None
                    }
                }
            })
            .collect();

        let mut by_partial: HashMap<u128, Vec<FileGroup>> = HashMap::new();
        for (hash, file_group) in partial {
            by_partial.entry(hash).or_default().push(file_group);
        }

        by_partial
            .into_iter()
            .filter(|(_, group)| group.len() > 1)
            .flat_map(|(_, group)| group)
            .collect()
    };

    // Cache lookups/misses are collected as plain data rather than written
    // inside the parallel closure below: redb write transactions are
    // exclusive, so batching misses into a transaction (after each parallel
    // chunk completes) avoids serializing hashing threads on a per-file
    // write lock. The group is hashed in chunks, committing after each, so
    // an interrupted scan keeps every completed chunk's hashes instead of
    // losing the whole size group (ADR-0016 addendum).
    let mut full_hashed: Vec<FullHashResult> = Vec::new();
    for chunk in full_hash_input.chunks(cache_chunk_len(size)) {
        let chunk_results: Vec<FullHashResult> = chunk
            .to_vec()
            .into_par_iter()
            .filter_map(|(representative, aliases)| {
            let stat = (cache.is_some() || fclones_import.is_some())
                .then(|| cache::stat(&representative))
                .flatten();

            if let (Some(cache), Some(stat)) = (cache, stat) {
                if let Some(hash) = cache.get(&representative, stat) {
                    tracing::trace!(path = %representative.display(), "full-hash cache hit");
                    return Some(FullHashResult {
                        hash,
                        group: (representative, aliases),
                        cache_write: None,
                    });
                }
            }

            if let (Some(fclones_import), Some(stat)) = (fclones_import, stat) {
                let modified_ms = fclones_import::to_millis(stat.1);
                if let Some(hash) =
                    fclones_import.lookup_full_hash(&representative, stat.0, modified_ms)
                {
                    tracing::trace!(path = %representative.display(), "fclones cache import hit");
                    return Some(FullHashResult {
                        hash,
                        group: (representative, aliases),
                        cache_write: cache.is_some().then_some(stat),
                    });
                }
            }

            match io_pool.hash_full_file(&representative) {
                Ok(hash) => Some(FullHashResult {
                    hash,
                    group: (representative, aliases),
                    cache_write: if cache.is_some() { stat } else { None },
                }),
                Err(source) => {
                    report_error(event_tx, representative, source);
                    None
                }
            }
        })
            .collect();

        if let Some(cache) = cache {
            let updates: Vec<(Arc<Path>, cache::FileStat, u128)> = chunk_results
                .iter()
                .filter_map(|r| r.cache_write.map(|stat| (r.group.0.clone(), stat, r.hash)))
                .collect();
            cache.put_batch(&updates);
        }
        full_hashed.extend(chunk_results);
    }

    let mut by_full: HashMap<u128, Vec<FileGroup>> = HashMap::new();
    for result in full_hashed {
        by_full.entry(result.hash).or_default().push(result.group);
    }

    by_full
        .into_values()
        .filter(|group| group.len() > 1)
        .filter_map(|group| {
            let group = if options.verify_matches {
                verify_representatives(group, io_pool, event_tx)
            } else {
                group
            };
            if group.len() <= 1 {
                return None;
            }
            let mut paths: Vec<Arc<Path>> =
                group.into_iter().flat_map(|(_, aliases)| aliases).collect();
            paths.sort();
            Some(DuplicateGroup { size, paths })
        })
        .collect()
}

/// Max files hashed between cache commits within one size group.
const CACHE_CHUNK_MAX_FILES: usize = 256;
/// Max bytes hashed between cache commits (every file in a size group has
/// the same size, so this bounds work lost to an interrupt for huge files).
const CACHE_CHUNK_MAX_BYTES: u64 = 1 << 30;

/// Files per cache-commit chunk for a size group of `size`-byte files.
fn cache_chunk_len(size: u64) -> usize {
    let by_bytes = (CACHE_CHUNK_MAX_BYTES / size.max(1)).max(1);
    usize::try_from(by_bytes)
        .unwrap_or(CACHE_CHUNK_MAX_FILES)
        .min(CACHE_CHUNK_MAX_FILES)
}

/// Sends a [`ScanEvent::Error`] for a file that failed to read during
/// hashing or verification (FR-009: per-file errors never abort the scan).
fn report_error(event_tx: &Sender<ScanEvent>, path: Arc<Path>, source: io::Error) {
    tracing::warn!(path = %path.display(), error = %source, "file error during hashing/verification");
    let _ = event_tx.send(ScanEvent::Error(FileError { path, source }));
}

/// Byte-compares every representative in `group` against the first,
/// dropping any that don't actually match (ADR-0001's `--verify` mode).
/// Hardlink aliases are never re-verified — they share an inode with their
/// representative by construction, so they're trivially identical.
///
/// Streams each comparison through [`IoPool::files_equal`] rather than
/// buffering every file in the group into memory at once (ADR-0002
/// addendum). The reference file's own readability is checked exactly
/// once up front: if every comparison independently re-opened it and it
/// happened to be unreadable, that single failure would be reported once
/// per candidate instead of once, total.
fn verify_representatives(
    group: Vec<FileGroup>,
    io_pool: &IoPool,
    event_tx: &Sender<ScanEvent>,
) -> Vec<FileGroup> {
    if group.len() < 2 {
        return Vec::new();
    }

    let (reference_path, reference_aliases) = group[0].clone();
    if let Err(err) = io_pool.files_equal(&reference_path, &reference_path) {
        report_error(event_tx, reference_path, err);
        return Vec::new();
    }

    let mut survivors = vec![(reference_path.clone(), reference_aliases)];
    let compared: Vec<(FileGroup, io::Result<bool>)> = group[1..]
        .to_vec()
        .into_par_iter()
        .map(|(candidate, aliases)| {
            let equal = io_pool.files_equal(&reference_path, &candidate);
            ((candidate, aliases), equal)
        })
        .collect();

    for ((candidate, aliases), equal) in compared {
        match equal {
            Ok(true) => survivors.push((candidate, aliases)),
            Ok(false) => {} // legitimately different content -- not an error
            Err(err) => report_error(event_tx, candidate, err),
        }
    }

    if survivors.len() < 2 {
        Vec::new()
    } else {
        survivors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use file_id::FileId;
    use std::fs;

    fn collect_groups(dir: &std::path::Path, options: ScanOptions) -> Vec<DuplicateGroup> {
        scan(dir, options)
            .unwrap()
            .filter_map(|event| match event {
                ScanEvent::DuplicateGroup(group) => Some(group),
                _ => None,
            })
            .collect()
    }

    fn record(name: &str, size: u64, inode: u64) -> Sorted {
        Sorted {
            size,
            file_id: FileId::new_inode(1, inode),
            path: Path::new("/root").join(name),
        }
    }

    #[test]
    fn size_groups_skip_unique_sizes_and_collapse_hardlinks() {
        let mut records = vec![
            record("unique", 7, 1),
            record("b-link", 5, 2),
            record("c", 5, 3),
            record("a-link", 5, 2),
            record("lonely-x", 9, 4),
            record("lonely-y", 9, 4),
        ];
        records.sort_by_key(|r| (r.size, r.file_id));
        let groups: Vec<_> = SizeGroups::new(records.into_iter()).collect();

        // Size 7 is unique; size 9 is one inode under two names, so only
        // one distinct file -- neither can hold a duplicate.
        assert_eq!(groups.len(), 1);
        let (size, members) = &groups[0];
        assert_eq!(*size, 5);
        assert_eq!(members.len(), 2);
        let linked = members
            .iter()
            .find(|(_, aliases)| aliases.len() == 2)
            .unwrap();
        assert_eq!(&*linked.0, Path::new("/root/a-link"));
        assert_eq!(
            linked.1,
            vec![
                Arc::from(Path::new("/root/a-link")),
                Arc::from(Path::new("/root/b-link"))
            ]
        );
    }

    #[test]
    fn groups_empty_files_and_reports_hash_progress_before_finishing() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a"), b"").unwrap();
        fs::write(dir.path().join("b"), b"").unwrap();
        fs::write(dir.path().join("c"), b"xy").unwrap();
        fs::write(dir.path().join("d"), b"xy").unwrap();

        let events: Vec<ScanEvent> = scan(dir.path(), ScanOptions::default()).unwrap().collect();
        let groups: Vec<&DuplicateGroup> = events
            .iter()
            .filter_map(|e| match e {
                ScanEvent::DuplicateGroup(g) => Some(g),
                _ => None,
            })
            .collect();
        let mut sizes: Vec<u64> = groups.iter().map(|g| g.size).collect();
        sizes.sort_unstable();
        assert_eq!(sizes, vec![0, 2]);

        let progress = events
            .iter()
            .rposition(|e| matches!(e, ScanEvent::HashProgress(_)));
        let finished = events
            .iter()
            .position(|e| matches!(e, ScanEvent::Finished(_)));
        assert!(progress.unwrap() < finished.unwrap());
        // Only the two 2-byte files were read; the empty ones never were.
        let Some(ScanEvent::HashProgress(last)) = progress.map(|i| &events[i]) else {
            unreachable!()
        };
        assert_eq!((last.files_read, last.bytes_read), (2, 4));
    }

    #[test]
    fn finds_duplicate_small_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"same content").unwrap();
        fs::write(dir.path().join("b.txt"), b"same content").unwrap();
        fs::write(dir.path().join("c.txt"), b"different").unwrap();

        let groups = collect_groups(dir.path(), ScanOptions::default());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].size, 12);
        assert_eq!(groups[0].paths.len(), 2);
    }

    #[test]
    fn finds_duplicates_larger_than_sample_size() {
        let dir = tempfile::tempdir().unwrap();
        let content = vec![7u8; 4096];
        fs::write(dir.path().join("a.bin"), &content).unwrap();
        fs::write(dir.path().join("b.bin"), &content).unwrap();

        let options = ScanOptions {
            small_file_threshold: 128,
            partial_hash_sample_size: 128,
            ..ScanOptions::default()
        };
        let groups = collect_groups(dir.path(), options);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].paths.len(), 2);
    }

    #[test]
    fn no_duplicates_when_only_prefix_matches() {
        // A small partial_hash_sample_size (128) keeps the head/mid/tail
        // windows narrow relative to the 4096-byte file, so this actually
        // exercises the tail sample catching a difference a prefix-only
        // check would miss -- not just a coincidence of the sample
        // clamping to the whole file (ADR-0007 decoupled sample size from
        // small_file_threshold, so both must be set explicitly here to
        // keep that true).
        let dir = tempfile::tempdir().unwrap();
        let mut a = vec![1u8; 4096];
        let mut b = vec![1u8; 4096];
        a[4095] = 0;
        b[4095] = 1;
        fs::write(dir.path().join("a.bin"), &a).unwrap();
        fs::write(dir.path().join("b.bin"), &b).unwrap();

        let options = ScanOptions {
            small_file_threshold: 128,
            partial_hash_sample_size: 128,
            ..ScanOptions::default()
        };
        let groups = collect_groups(dir.path(), options);
        assert!(groups.is_empty());
    }

    #[test]
    fn unique_files_produce_no_groups() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"one").unwrap();
        fs::write(dir.path().join("b.txt"), b"two").unwrap();

        let groups = collect_groups(dir.path(), ScanOptions::default());
        assert!(groups.is_empty());
    }

    #[test]
    fn rejects_non_directory_root() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f.txt");
        fs::write(&file, b"x").unwrap();
        assert!(scan(file, ScanOptions::default()).is_err());
    }

    #[test]
    fn hardlink_aliases_are_included_when_content_matches_another_file() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        let c = dir.path().join("c.txt");
        fs::write(&a, b"shared content").unwrap();
        fs::hard_link(&a, &b).unwrap();
        fs::write(&c, b"shared content").unwrap();

        let groups = collect_groups(dir.path(), ScanOptions::default());
        assert_eq!(groups.len(), 1);
        let mut paths = groups[0].paths.clone();
        paths.sort();
        let mut expected: Vec<Arc<Path>> = vec![a.into(), b.into(), c.into()];
        expected.sort();
        assert_eq!(paths, expected);
    }

    #[test]
    fn standalone_hardlink_pair_is_not_reported_as_duplicate() {
        // Two paths to the same inode already share storage; with nothing
        // else matching their content, there's nothing to report — they
        // aren't "duplicates" in any actionable sense (ADR-0001).
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"only one file's worth of content").unwrap();
        fs::hard_link(&a, &b).unwrap();

        let groups = collect_groups(dir.path(), ScanOptions::default());
        assert!(groups.is_empty());
    }

    #[test]
    fn verify_matches_true_still_reports_real_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"identical").unwrap();
        fs::write(dir.path().join("b.txt"), b"identical").unwrap();

        let options = ScanOptions {
            verify_matches: true,
            ..ScanOptions::default()
        };
        let groups = collect_groups(dir.path(), options);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].paths.len(), 2);
    }

    #[test]
    fn verify_representatives_drops_entries_that_do_not_byte_match() {
        // Exercises the --verify byte-compare path directly: even if two
        // files reached this stage with the same hash, verification must
        // still drop one whose actual bytes differ rather than trusting the
        // hash blindly.
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        let c = dir.path().join("c.txt");
        fs::write(&a, b"match").unwrap();
        fs::write(&b, b"match").unwrap();
        fs::write(&c, b"MATCH").unwrap(); // same length, different bytes

        let io_pool = IoPool::new(2);
        let (tx, _rx) = unbounded();
        let group: Vec<FileGroup> = vec![
            (a.clone().into(), vec![a.clone().into()]),
            (b.clone().into(), vec![b.clone().into()]),
            (c.clone().into(), vec![c.clone().into()]),
        ];

        let survivors = verify_representatives(group, &io_pool, &tx);
        let survivor_paths: Vec<Arc<Path>> = survivors.into_iter().map(|(rep, _)| rep).collect();
        assert_eq!(survivor_paths.len(), 2);
        assert!(survivor_paths.iter().any(|p| p.as_ref() == a.as_path()));
        assert!(survivor_paths.iter().any(|p| p.as_ref() == b.as_path()));
        assert!(!survivor_paths.iter().any(|p| p.as_ref() == c.as_path()));
    }

    #[test]
    fn read_failures_during_hashing_are_reported_and_do_not_abort_the_group() {
        let dir = tempfile::tempdir().unwrap();
        let dup1 = dir.path().join("dup1.txt");
        let dup2 = dir.path().join("dup2.txt");
        fs::write(&dup1, b"dup").unwrap();
        fs::write(&dup2, b"dup").unwrap();
        let missing = dir.path().join("vanished.txt"); // never created

        let io_pool = IoPool::new(2);
        let options = ScanOptions::default();
        let (tx, rx) = unbounded();
        let members: Vec<FileGroup> = vec![
            (missing.clone().into(), vec![missing.clone().into()]),
            (dup1.clone().into(), vec![dup1.clone().into()]),
            (dup2.clone().into(), vec![dup2.clone().into()]),
        ];

        let groups = process_size_group(3, members, &io_pool, &options, None, None, &tx);
        drop(tx);

        assert_eq!(
            groups.len(),
            1,
            "the two real duplicates must still be found"
        );
        let mut paths = groups[0].paths.clone();
        paths.sort();
        let mut expected: Vec<Arc<Path>> = vec![dup1.into(), dup2.into()];
        expected.sort();
        assert_eq!(paths, expected);

        let errors: Vec<ScanEvent> = rx.try_iter().collect();
        assert_eq!(
            errors.len(),
            1,
            "the missing file must be reported exactly once"
        );
        match &errors[0] {
            ScanEvent::Error(err) => assert_eq!(err.path.as_ref(), missing.as_path()),
            other => panic!("expected ScanEvent::Error, got {other:?}"),
        }
    }

    #[test]
    fn finished_event_is_always_last_and_reports_every_group() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a1.txt"), b"aa").unwrap();
        fs::write(dir.path().join("a2.txt"), b"aa").unwrap();
        fs::write(dir.path().join("b1.txt"), b"bbb").unwrap();
        fs::write(dir.path().join("b2.txt"), b"bbb").unwrap();

        let events: Vec<ScanEvent> = scan(dir.path(), ScanOptions::default()).unwrap().collect();

        let finished_positions: Vec<usize> = events
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e, ScanEvent::Finished(_)))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            finished_positions,
            vec![events.len() - 1],
            "Finished must be the only, final event"
        );

        let group_count = events
            .iter()
            .filter(|e| matches!(e, ScanEvent::DuplicateGroup(_)))
            .count();
        assert_eq!(
            group_count, 2,
            "both duplicate pairs must be streamed before Finished"
        );

        match events.last().unwrap() {
            ScanEvent::Finished(summary) => assert_eq!(summary.duplicate_groups, 2),
            other => panic!("expected ScanEvent::Finished, got {other:?}"),
        }
    }

    #[test]
    fn cached_scan_produces_identical_results_to_an_uncached_one() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.bin"), vec![7u8; 4096]).unwrap();
        fs::write(dir.path().join("b.bin"), vec![7u8; 4096]).unwrap();
        fs::write(dir.path().join("c.bin"), vec![9u8; 4096]).unwrap();
        let cache_dir = tempfile::tempdir().unwrap();
        let cache_path = cache_dir.path().join("cache.redb");

        let uncached = collect_groups(dir.path(), ScanOptions::default());

        let cached_options = ScanOptions {
            cache_path: Some(cache_path.clone()),
            ..ScanOptions::default()
        };
        let first_cached_run = collect_groups(dir.path(), cached_options.clone());
        // Second run actually exercises cache hits (the first run populated
        // the cache with every file's full hash).
        let second_cached_run = collect_groups(dir.path(), cached_options);

        assert_eq!(uncached, first_cached_run);
        assert_eq!(uncached, second_cached_run);
    }

    #[test]
    fn cache_chunk_len_is_bounded_by_files_and_bytes() {
        assert_eq!(cache_chunk_len(0), CACHE_CHUNK_MAX_FILES);
        assert_eq!(cache_chunk_len(1), CACHE_CHUNK_MAX_FILES);
        assert_eq!(cache_chunk_len(CACHE_CHUNK_MAX_BYTES / 4), 4);
        assert_eq!(cache_chunk_len(CACHE_CHUNK_MAX_BYTES * 10), 1);
    }

    #[test]
    fn a_size_group_spanning_several_cache_chunks_is_fully_cached() {
        // More same-size files than one chunk, so the group is hashed and
        // committed in multiple transactions; every file must still be
        // grouped correctly and cached for the next run.
        let dir = tempfile::tempdir().unwrap();
        let count = CACHE_CHUNK_MAX_FILES * 2 + 5;
        for i in 0..count {
            fs::write(dir.path().join(format!("f{i}.bin")), vec![5u8; 64]).unwrap();
        }
        let cache_dir = tempfile::tempdir().unwrap();
        let cache_path = cache_dir.path().join("cache.redb");
        let options = ScanOptions {
            cache_path: Some(cache_path.clone()),
            ..ScanOptions::default()
        };

        let groups = collect_groups(dir.path(), options);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].paths.len(), count);

        let cache = HashCache::open(&cache_path).unwrap();
        for path in &groups[0].paths {
            let stat = cache::stat(path).unwrap();
            assert!(cache.get(path, stat).is_some(), "{path:?} not cached");
        }
    }

    #[test]
    fn cache_file_is_created_even_when_the_scan_finds_nothing() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("only.txt"), b"alone").unwrap();
        let cache_dir = tempfile::tempdir().unwrap();
        let cache_path = cache_dir.path().join("cache.redb");
        let options = ScanOptions {
            cache_path: Some(cache_path.clone()),
            ..ScanOptions::default()
        };

        assert!(collect_groups(dir.path(), options).is_empty());
        assert!(cache_path.exists());
    }

    #[test]
    fn a_changed_file_is_not_served_a_stale_cached_hash() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.bin");
        let b = dir.path().join("b.bin");
        fs::write(&a, vec![1u8; 4096]).unwrap();
        fs::write(&b, vec![1u8; 4096]).unwrap();
        let cache_dir = tempfile::tempdir().unwrap();
        let options = ScanOptions {
            cache_path: Some(cache_dir.path().join("cache.redb")),
            ..ScanOptions::default()
        };

        let first = collect_groups(dir.path(), options.clone());
        assert_eq!(first.len(), 1, "a and b start out identical");

        // Change b's content (and therefore its mtime). A stale cache hit
        // would keep reporting a and b as duplicates. The sleep is
        // generous (some filesystems only track mtime to 1s resolution).
        std::thread::sleep(std::time::Duration::from_millis(1100));
        fs::write(&b, vec![2u8; 4096]).unwrap();

        let second = collect_groups(dir.path(), options);
        assert!(
            second.is_empty(),
            "the changed file must not be served a stale cached hash: {second:?}"
        );
    }
}
