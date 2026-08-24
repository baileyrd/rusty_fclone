use std::path::{Path, PathBuf};

use file_id::{get_file_id, FileId};
use jwalk::WalkDir;

use crate::error::FileError;
use crate::model::ScanOptions;

/// A file found during traversal, stat-ed but not yet read.
pub(crate) struct Candidate {
    pub path: PathBuf,
    pub size: u64,
    pub file_id: FileId,
}

/// Walks `root` in parallel (via jwalk's built-in rayon-backed walker),
/// stat-ing every regular file it finds.
///
/// Symlinks are skipped unless `options.follow_symlinks` is set, and the
/// walk stays on the filesystem `root` lives on unless
/// `options.cross_filesystems` is set (ADR-0003). Per-file errors are
/// reported through `on_error` and otherwise ignored — one bad file never
/// aborts the whole traversal (ADR-0004).
pub(crate) fn traverse(
    root: &Path,
    options: &ScanOptions,
    mut on_error: impl FnMut(FileError),
) -> Vec<Candidate> {
    let root_device = get_file_id(root).ok().and_then(|id| device_component(&id));

    let walker = WalkDir::new(root)
        .follow_links(options.follow_symlinks)
        .skip_hidden(false);

    let mut candidates = Vec::new();
    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                let path = err
                    .path()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| root.to_path_buf());
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

        let path = entry.path();

        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(err) => {
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
                on_error(FileError {
                    path: path.clone(),
                    source: err,
                });
                continue;
            }
        };

        if !options.cross_filesystems {
            if let (Some(root_dev), Some(entry_dev)) = (root_device, device_component(&file_id)) {
                if entry_dev != root_dev {
                    continue;
                }
            }
        }

        candidates.push(Candidate {
            path,
            size: metadata.len(),
            file_id,
        });
    }

    candidates
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

    #[test]
    fn finds_regular_files_only() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"a").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/b.txt"), b"bb").unwrap();

        let options = ScanOptions::default();
        let candidates = traverse(dir.path(), &options, |_| {});

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
            let candidates = traverse(dir.path(), &options, |_| {});
            assert_eq!(candidates.len(), 1);
            assert_eq!(candidates[0].path.file_name().unwrap(), "real.txt");
        }
    }
}
