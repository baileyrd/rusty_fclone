use std::path::Path;
use std::sync::Arc;

use file_id::{get_file_id, FileId};
use jwalk::WalkDir;

use crate::error::FileError;
use crate::model::ScanOptions;

/// A file found during traversal, stat-ed but not yet read.
pub(crate) struct Candidate {
    pub path: Arc<Path>,
    pub size: u64,
    pub file_id: FileId,
}

/// Walks `root` in parallel (via jwalk's built-in rayon-backed walker),
/// stat-ing every regular file it finds and handing each one to
/// `on_candidate` as soon as it's ready, rather than collecting a `Vec`
/// the caller has to loop over separately (DETECTION-STREAMING-OVERLAP:
/// this merges the traversal and hardlink-collapse stages into one pass —
/// see `pipeline::run_scan`'s caller for the collapse step now folded into
/// `on_candidate`).
///
/// Symlinks are skipped unless `options.follow_symlinks` is set, and the
/// walk stays on the filesystem `root` lives on unless
/// `options.cross_filesystems` is set (ADR-0003). Per-file errors are
/// reported through `on_error` and otherwise ignored — one bad file never
/// aborts the whole traversal (ADR-0004).
#[tracing::instrument(skip(options, on_error, on_candidate), fields(root = %root.display()))]
pub(crate) fn traverse(
    root: &Path,
    options: &ScanOptions,
    mut on_error: impl FnMut(FileError),
    mut on_candidate: impl FnMut(Candidate),
) {
    let root_device = get_file_id(root).ok().and_then(|id| device_component(&id));

    let exclude_paths = options.exclude_paths.clone();
    let walker = WalkDir::new(root)
        .follow_links(options.follow_symlinks)
        .skip_hidden(false)
        .process_read_dir(move |_depth, _dir_path, _read_dir_state, children| {
            if exclude_paths.is_empty() {
                return;
            }
            children.retain(|entry_result| {
                let Ok(entry) = entry_result else {
                    return true;
                };
                let entry_path = entry.parent_path.join(&entry.file_name);
                !exclude_paths
                    .iter()
                    .any(|excluded| entry_path.starts_with(excluded))
            });
        });

    let mut candidate_count = 0u64;
    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                let path: Arc<Path> = err.path().map(Arc::from).unwrap_or_else(|| Arc::from(root));
                tracing::warn!(path = %path.display(), error = %err, "traversal entry error");
                on_error(FileError {
                    path,
                    source: err.into(),
                });
                continue;
            }
        };

        if !entry.file_type().is_file() {
            continue;
        }

        let path: Arc<Path> = Arc::from(entry.path());

        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(err) => {
                tracing::warn!(path = %path.display(), error = %err, "failed to stat file");
                on_error(FileError {
                    path: path.clone(),
                    source: err.into(),
                });
                continue;
            }
        };

        if !size_allowed(metadata.len(), options) {
            tracing::trace!(path = %path.display(), size = metadata.len(), "skipped -- size filter");
            continue;
        }
        if !extension_allowed(&path, options) {
            tracing::trace!(path = %path.display(), "skipped -- extension filter");
            continue;
        }

        let file_id = match get_file_id(&path) {
            Ok(id) => id,
            Err(err) => {
                tracing::warn!(path = %path.display(), error = %err, "failed to read file id");
                on_error(FileError {
                    path: path.clone(),
                    source: err,
                });
                continue;
            }
        };

        if is_excluded_by_filesystem_boundary(
            options.cross_filesystems,
            root_device,
            device_component(&file_id),
        ) {
            tracing::trace!(path = %path.display(), "skipped -- different filesystem");
            continue;
        }

        candidate_count += 1;
        on_candidate(Candidate {
            path,
            size: metadata.len(),
            file_id,
        });
    }

    tracing::debug!(candidates = candidate_count, "traversal finished");
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
        traverse(root, options, on_error, |c| candidates.push(c));
        candidates
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
            assert_eq!(candidates[0].path.file_name().unwrap(), "real.txt");
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
            assert_eq!(candidates[0].path.file_name().unwrap(), "real.txt");
            assert_eq!(
                errors.len(),
                1,
                "the broken symlink must be reported as a per-file error, not silently dropped"
            );
        }
    }

    /// A symlink cycle (a directory containing a symlink back to one of its
    /// own ancestors) with `follow_symlinks = true` must not hang the scan.
    /// This relies entirely on jwalk's own loop detection (ADR-0003); this
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
            // sub/loop/sub/loop/... without jwalk's cycle detection.
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
                Ok((candidate_count, _error_count)) => {
                    // real.txt should still be found exactly once despite
                    // the cycle -- jwalk must not revisit it repeatedly.
                    assert_eq!(candidate_count, 1);
                }
                Err(_) => panic!(
                    "traverse() did not terminate within 10s on a symlink cycle -- \
                     jwalk's loop detection regressed or was never actually protecting us"
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
        assert!(candidates[0].path.ends_with("kept.txt"));
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
        assert!(candidates[0].path.ends_with("kept.txt"));
    }
}
