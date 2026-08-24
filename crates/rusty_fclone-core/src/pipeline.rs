use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;

use crossbeam_channel::{unbounded, Receiver, Sender};
use file_id::FileId;
use rayon::prelude::*;

use crate::error::ScanError;
use crate::hash::{hash_chunks, sample_ranges};
use crate::io_pool::IoPool;
use crate::model::{DuplicateGroup, ScanEvent, ScanOptions, ScanSummary};
use crate::traversal::traverse;

/// One distinct file, identified by its representative path (the first
/// alias, alphabetically) plus every path — including hardlink aliases —
/// that shares its content.
type FileGroup = (PathBuf, Vec<PathBuf>);

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

fn run_scan(root: PathBuf, options: ScanOptions, event_tx: Sender<ScanEvent>) {
    let mut summary = ScanSummary::default();

    let candidates = traverse(&root, &options, |err| {
        let _ = event_tx.send(ScanEvent::Error(err));
    });

    summary.files_scanned = candidates.len() as u64;
    summary.bytes_scanned = candidates.iter().map(|c| c.size).sum();

    // Collapse existing hardlinks: files sharing a (device, inode) / file-id
    // already share storage, so hash only one representative per id.
    let mut by_file_id: HashMap<FileId, (u64, Vec<PathBuf>)> = HashMap::new();
    for candidate in candidates {
        by_file_id
            .entry(candidate.file_id)
            .or_insert_with(|| (candidate.size, Vec::new()))
            .1
            .push(candidate.path);
    }

    // Group representatives by size — only sizes shared by 2+ distinct
    // files can possibly be duplicates.
    let mut by_size: HashMap<u64, Vec<FileGroup>> = HashMap::new();
    for (size, mut aliases) in by_file_id.into_values() {
        aliases.sort();
        let representative = aliases[0].clone();
        by_size
            .entry(size)
            .or_default()
            .push((representative, aliases));
    }

    let candidate_groups: Vec<(u64, Vec<FileGroup>)> = by_size
        .into_iter()
        .filter(|(_, members)| members.len() > 1)
        .collect();

    let io_pool = IoPool::new(options.io_threads);
    let duplicate_groups = AtomicU64::new(0);
    let duplicate_files = AtomicU64::new(0);

    candidate_groups
        .into_par_iter()
        .for_each(|(size, members)| {
            for group in process_size_group(size, members, &io_pool, &options) {
                duplicate_groups.fetch_add(1, Ordering::Relaxed);
                duplicate_files.fetch_add(group.paths.len() as u64, Ordering::Relaxed);
                let _ = event_tx.send(ScanEvent::DuplicateGroup(group));
            }
        });

    summary.duplicate_groups = duplicate_groups.load(Ordering::Relaxed);
    summary.duplicate_files = duplicate_files.load(Ordering::Relaxed);
    let _ = event_tx.send(ScanEvent::Finished(summary));
}

/// Runs the staged-hashing pipeline (ADR-0001) for one size-group: partial
/// hash to prune, full hash to confirm, optional byte-verify — returning
/// every subgroup that survives as a [`DuplicateGroup`].
fn process_size_group(
    size: u64,
    members: Vec<FileGroup>,
    io_pool: &IoPool,
    options: &ScanOptions,
) -> Vec<DuplicateGroup> {
    let small_file = size <= options.small_file_threshold;

    let full_hash_input: Vec<FileGroup> = if small_file {
        members
    } else {
        let partial: Vec<(u128, FileGroup)> = members
            .into_par_iter()
            .filter_map(|(representative, aliases)| {
                let ranges = sample_ranges(size, options.small_file_threshold);
                let bytes = io_pool.read_ranges(&representative, ranges).ok()?;
                Some((hash_chunks(&[&bytes]), (representative, aliases)))
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

    let full_hashed: Vec<(u128, FileGroup)> = full_hash_input
        .into_par_iter()
        .filter_map(|(representative, aliases)| {
            let bytes = io_pool.read_full(&representative).ok()?;
            Some((hash_chunks(&[&bytes]), (representative, aliases)))
        })
        .collect();

    let mut by_full: HashMap<u128, Vec<FileGroup>> = HashMap::new();
    for (hash, file_group) in full_hashed {
        by_full.entry(hash).or_default().push(file_group);
    }

    by_full
        .into_values()
        .filter(|group| group.len() > 1)
        .filter_map(|group| {
            let group = if options.verify_matches {
                verify_representatives(group, io_pool)
            } else {
                group
            };
            if group.len() <= 1 {
                return None;
            }
            let mut paths: Vec<PathBuf> =
                group.into_iter().flat_map(|(_, aliases)| aliases).collect();
            paths.sort();
            Some(DuplicateGroup { size, paths })
        })
        .collect()
}

/// Byte-compares every representative in `group` against the first,
/// dropping any that don't actually match (ADR-0001's `--verify` mode).
/// Hardlink aliases are never re-verified — they share an inode with their
/// representative by construction, so they're trivially identical.
fn verify_representatives(group: Vec<FileGroup>, io_pool: &IoPool) -> Vec<FileGroup> {
    let mut with_bytes: Vec<(PathBuf, Vec<PathBuf>, io::Result<Vec<u8>>)> = group
        .into_par_iter()
        .map(|(representative, aliases)| {
            let bytes = io_pool.read_full(&representative);
            (representative, aliases, bytes)
        })
        .collect();

    with_bytes.retain(|(_, _, bytes)| bytes.is_ok());
    if with_bytes.len() < 2 {
        return Vec::new();
    }

    let reference = with_bytes[0].2.as_ref().unwrap().clone();
    with_bytes
        .into_iter()
        .filter(|(_, _, bytes)| bytes.as_ref().unwrap() == &reference)
        .map(|(representative, aliases, _)| (representative, aliases))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
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
            ..ScanOptions::default()
        };
        let groups = collect_groups(dir.path(), options);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].paths.len(), 2);
    }

    #[test]
    fn no_duplicates_when_only_prefix_matches() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = vec![1u8; 4096];
        let mut b = vec![1u8; 4096];
        a[4095] = 0;
        b[4095] = 1;
        fs::write(dir.path().join("a.bin"), &a).unwrap();
        fs::write(dir.path().join("b.bin"), &b).unwrap();

        let options = ScanOptions {
            small_file_threshold: 128,
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
}
