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

    let walker = WalkDir::new(root)
        .follow_links(options.follow_symlinks)
        .skip_hidden(false);

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
}
