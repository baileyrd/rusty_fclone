use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;

use crossbeam_channel::{unbounded, Receiver, Sender};
use file_id::FileId;
use rayon::prelude::*;

use crate::error::{FileError, ScanError};
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

#[tracing::instrument(skip(options, event_tx), fields(root = %root.display()))]
fn run_scan(root: PathBuf, options: ScanOptions, event_tx: Sender<ScanEvent>) {
    let mut summary = ScanSummary::default();

    let candidates = traverse(&root, &options, |err| {
        let _ = event_tx.send(ScanEvent::Error(err));
    });

    summary.files_scanned = candidates.len() as u64;
    summary.bytes_scanned = candidates.iter().map(|c| c.size).sum();
    tracing::info!(
        files_scanned = summary.files_scanned,
        bytes_scanned = summary.bytes_scanned,
        "traversal complete"
    );

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
    tracing::debug!(
        size_groups = candidate_groups.len(),
        "size-grouping complete, starting staged hashing"
    );

    let io_pool = IoPool::new(options.io_threads);
    let duplicate_groups = AtomicU64::new(0);
    let duplicate_files = AtomicU64::new(0);

    candidate_groups
        .into_par_iter()
        .for_each(|(size, members)| {
            for group in process_size_group(size, members, &io_pool, &options, &event_tx) {
                duplicate_groups.fetch_add(1, Ordering::Relaxed);
                duplicate_files.fetch_add(group.paths.len() as u64, Ordering::Relaxed);
                let _ = event_tx.send(ScanEvent::DuplicateGroup(group));
            }
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

/// Runs the staged-hashing pipeline (ADR-0001) for one size-group: partial
/// hash to prune, full hash to confirm, optional byte-verify — returning
/// every subgroup that survives as a [`DuplicateGroup`].
#[tracing::instrument(skip(members, io_pool, options, event_tx), fields(members = members.len()))]
fn process_size_group(
    size: u64,
    members: Vec<FileGroup>,
    io_pool: &IoPool,
    options: &ScanOptions,
    event_tx: &Sender<ScanEvent>,
) -> Vec<DuplicateGroup> {
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

    let full_hashed: Vec<(u128, FileGroup)> = full_hash_input
        .into_par_iter()
        .filter_map(
            |(representative, aliases)| match io_pool.hash_full_file(&representative) {
                Ok(hash) => Some((hash, (representative, aliases))),
                Err(source) => {
                    report_error(event_tx, representative, source);
                    None
                }
            },
        )
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
                verify_representatives(group, io_pool, event_tx)
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

/// Sends a [`ScanEvent::Error`] for a file that failed to read during
/// hashing or verification (FR-009: per-file errors never abort the scan).
fn report_error(event_tx: &Sender<ScanEvent>, path: PathBuf, source: io::Error) {
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
        let mut expected = vec![a, b, c];
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
        let group = vec![
            (a.clone(), vec![a.clone()]),
            (b.clone(), vec![b.clone()]),
            (c.clone(), vec![c.clone()]),
        ];

        let survivors = verify_representatives(group, &io_pool, &tx);
        let survivor_paths: Vec<PathBuf> = survivors.into_iter().map(|(rep, _)| rep).collect();
        assert_eq!(survivor_paths.len(), 2);
        assert!(survivor_paths.contains(&a));
        assert!(survivor_paths.contains(&b));
        assert!(!survivor_paths.contains(&c));
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
        let members = vec![
            (missing.clone(), vec![missing.clone()]),
            (dup1.clone(), vec![dup1.clone()]),
            (dup2.clone(), vec![dup2.clone()]),
        ];

        let groups = process_size_group(3, members, &io_pool, &options, &tx);
        drop(tx);

        assert_eq!(
            groups.len(),
            1,
            "the two real duplicates must still be found"
        );
        let mut paths = groups[0].paths.clone();
        paths.sort();
        let mut expected = vec![dup1, dup2];
        expected.sort();
        assert_eq!(paths, expected);

        let errors: Vec<ScanEvent> = rx.try_iter().collect();
        assert_eq!(
            errors.len(),
            1,
            "the missing file must be reported exactly once"
        );
        match &errors[0] {
            ScanEvent::Error(err) => assert_eq!(err.path, missing),
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
}
