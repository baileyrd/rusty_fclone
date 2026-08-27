//! Turning a folder-level duplicate match ([`crate::FolderMatch`],
//! ADR-0021) into disk-space savings, by acting on every file in one
//! folder ("removed") against its confirmed partner file in another
//! ("kept") — ADR-0023.
//!
//! Deliberately reuses [`crate::action`]'s existing, already-tested
//! per-file primitives ([`crate::action::apply`]) rather than duplicating
//! their delete/trash/hardlink/reflink logic: each file pair becomes its own
//! single-action [`crate::action::ActionPlan`], planned and applied
//! through the exact same code path a regular file-level `DuplicateGroup`
//! action uses.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::action::{self, ActionKind, ActionPlan, FileAction};
use crate::error::{FileError, FolderActionError};
use crate::model::{DuplicateGroup, ScanOptions};
use crate::select;
use crate::traversal;

/// One file inside a folder-match's "removed" side, paired with the exact
/// partner path under "kept" that holds identical content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderFilePair {
    pub remove: PathBuf,
    pub keep: PathBuf,
    pub size: u64,
}

/// What running `kind` over every file in `removed` (against its confirmed
/// partner in `kept`) would do, computed without touching the filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderActionPlan {
    pub kind: ActionKind,
    pub kept: PathBuf,
    pub removed: PathBuf,
    pub pairs: Vec<FolderFilePair>,
    pub bytes_reclaimed: u64,
    /// Count of files under `removed` that were excluded from `pairs`
    /// because they're protected by a reference folder
    /// (`ACTION-REFERENCE-FOLDERS`, ADR-0025). Nonzero here means
    /// `removed` will *not* be empty even after every planned pair
    /// succeeds — `apply_folder` never attempts to prune the directory
    /// when this is nonzero, since doing so would delete the protected
    /// files still sitting in it.
    pub protected_files_skipped: u64,
}

/// The outcome of actually running a [`FolderActionPlan`].
#[derive(Debug, Default)]
pub struct FolderApplyReport {
    pub succeeded: Vec<PathBuf>,
    pub failed: Vec<FileError>,
    pub bytes_reclaimed: u64,
    /// `true` once every planned file was successfully removed *and* the
    /// now file-less `removed` directory tree was pruned. Only ever set
    /// for [`ActionKind::Delete`]/[`ActionKind::Trash`] — hardlink/reflink
    /// replace each file in place, so `removed` stays fully populated by
    /// design.
    pub directory_removed: bool,
}

/// Plans `kind` for every file `removed` contains, pairing each with the
/// path under `kept` that must hold identical content for the same
/// relative layout. `removed`/`kept` and `groups`/`options` must match the
/// scan that originally produced the `FolderMatch` this is planning for —
/// like [`crate::find_folder_duplicates`] itself, this trusts `groups`
/// rather than re-hashing (a second, lightweight, stat-only traversal of
/// `removed` alone, no full-file hashing).
///
/// Fails closed: if any file under `removed` doesn't have its expected
/// partner recorded in `groups` at the matching size, no plan is returned
/// at all — not a partial one missing that file. This re-derives the
/// folder match's "every file has a confirmed duplicate" guarantee
/// independently at planning time, rather than trusting a `FolderMatch`
/// computed earlier (and potentially stale by now, if anything on disk
/// changed since).
///
/// `reference_paths` (`ACTION-REFERENCE-FOLDERS`, ADR-0025) excludes any
/// file under `removed` that's itself protected from `pairs` entirely —
/// never planned for removal, the same hard-block guarantee
/// `action::plan_with_keep` gives individual files. Skipped files are
/// counted in `FolderActionPlan::protected_files_skipped` rather than
/// silently vanishing from the plan, since that count is what tells
/// `apply_folder` the directory can't safely be pruned even after every
/// *planned* pair succeeds.
pub fn plan_folder(
    removed: &Path,
    kept: &Path,
    groups: &[DuplicateGroup],
    options: &ScanOptions,
    kind: ActionKind,
    reference_paths: &[PathBuf],
) -> Result<FolderActionPlan, FolderActionError> {
    if !removed.is_dir() {
        return Err(FolderActionError::NotADirectory(removed.to_path_buf()));
    }

    let mut path_to_group: HashMap<&Path, usize> = HashMap::new();
    for (idx, group) in groups.iter().enumerate() {
        for p in &group.paths {
            path_to_group.insert(p.as_ref(), idx);
        }
    }

    let mut pairs = Vec::new();
    let mut bytes_reclaimed = 0u64;
    let mut protected_files_skipped = 0u64;
    let mut mismatch: Option<FolderActionError> = None;

    traversal::traverse(
        removed,
        options,
        |_err| {
            // A per-file traversal error here means that file can't even
            // be identified, let alone confirmed as a duplicate -- it
            // simply never reaches on_candidate below, which already
            // fails the plan closed for it (NoConfirmedDuplicate would be
            // wrong wording for "couldn't even be read", but the effect --
            // no plan produced -- is the same; see the `mismatch` check
            // after `traverse` returns for how an entirely-absent
            // candidate is handled: it isn't paired, so it can't slip
            // through as if it were confirmed).
        },
        |candidate| {
            if mismatch.is_some() {
                return;
            }
            let path = candidate.path.to_path_buf();

            if select::is_protected(&path, reference_paths) {
                protected_files_skipped += 1;
                return;
            }

            let rel = path
                .strip_prefix(removed)
                .expect("traversal always yields paths under `removed`");
            let expected_partner = kept.join(rel);

            let group = path_to_group.get(path.as_path()).map(|&idx| &groups[idx]);
            let confirmed = group.is_some_and(|g| {
                g.size == candidate.size
                    && g.paths
                        .iter()
                        .any(|p| p.as_ref() == expected_partner.as_path())
            });

            if !confirmed {
                mismatch = Some(FolderActionError::NoConfirmedDuplicate {
                    path,
                    expected_partner,
                });
                return;
            }

            bytes_reclaimed += candidate.size;
            pairs.push(FolderFilePair {
                remove: path,
                keep: expected_partner,
                size: candidate.size,
            });
        },
    );

    if let Some(err) = mismatch {
        return Err(err);
    }

    Ok(FolderActionPlan {
        kind,
        kept: kept.to_path_buf(),
        removed: removed.to_path_buf(),
        pairs,
        bytes_reclaimed,
        protected_files_skipped,
    })
}

/// Executes `plan` against the filesystem, one file pair at a time, by
/// building a single-file [`ActionPlan`] per pair and running it through
/// [`action::apply`] — the exact same tested delete/hardlink/reflink code
/// path a regular `DuplicateGroup` action uses. Per-file failures don't
/// abort the rest (ADR-0004's error-tolerance contract).
///
/// After a fully successful [`ActionKind::Delete`]/[`ActionKind::Trash`]
/// (every pair succeeded) *and* no file under `removed` was skipped for
/// being protected (`plan.protected_files_skipped == 0`), the now
/// file-less `removed` directory tree is pruned via `fs::remove_dir_all`.
/// Skipping the prune whenever a protected file was excluded from the plan
/// is load-bearing, not just tidy: `remove_dir_all` doesn't know or care
/// which files inside `removed` are protected — pruning anyway would
/// delete them right along with everything else, silently defeating the
/// entire guarantee `ACTION-REFERENCE-FOLDERS` exists to provide. A failed
/// prune for an unrelated reason (e.g. something else was added to
/// `removed` after this plan was made) is reported via
/// `directory_removed: false`, not as a per-file failure — every actual
/// file action already succeeded by that point.
pub fn apply_folder(plan: &FolderActionPlan) -> FolderApplyReport {
    let mut report = FolderApplyReport::default();
    for pair in &plan.pairs {
        let single = ActionPlan {
            size: pair.size,
            kept: pair.keep.clone(),
            actions: vec![FileAction {
                path: pair.remove.clone(),
                kind: plan.kind,
            }],
            bytes_reclaimed: pair.size,
        };
        let sub_report = action::apply(&single);
        report.succeeded.extend(sub_report.succeeded);
        report.failed.extend(sub_report.failed);
        report.bytes_reclaimed += sub_report.bytes_reclaimed;
    }

    let prunes_directory = matches!(plan.kind, ActionKind::Delete | ActionKind::Trash);
    if prunes_directory && plan.protected_files_skipped == 0 && report.failed.is_empty() {
        report.directory_removed = fs::remove_dir_all(&plan.removed).is_ok();
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;

    fn group(size: u64, paths: &[&Path]) -> DuplicateGroup {
        DuplicateGroup {
            size,
            paths: paths.iter().map(|p| Arc::from(*p)).collect(),
        }
    }

    #[test]
    fn plan_folder_pairs_every_file_with_its_kept_side_partner() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        let big = dir.path().join("big");
        fs::create_dir_all(&small).unwrap();
        fs::create_dir_all(&big).unwrap();
        fs::write(small.join("1.txt"), b"one").unwrap();
        fs::write(big.join("1.txt"), b"one").unwrap();
        fs::write(small.join("2.txt"), b"two").unwrap();
        fs::write(big.join("2.txt"), b"two").unwrap();
        fs::write(big.join("extra.txt"), b"only in big").unwrap();

        let groups = vec![
            group(3, &[&small.join("1.txt"), &big.join("1.txt")]),
            group(3, &[&small.join("2.txt"), &big.join("2.txt")]),
        ];

        let plan = plan_folder(
            &small,
            &big,
            &groups,
            &ScanOptions::default(),
            ActionKind::Delete,
            &[],
        )
        .expect("every file in small has a confirmed partner in big");

        assert_eq!(plan.pairs.len(), 2);
        assert_eq!(plan.bytes_reclaimed, 6);
        let mut pairs = plan.pairs.clone();
        pairs.sort_by(|a, b| a.remove.cmp(&b.remove));
        assert_eq!(pairs[0].remove, small.join("1.txt"));
        assert_eq!(pairs[0].keep, big.join("1.txt"));
        assert_eq!(pairs[1].remove, small.join("2.txt"));
        assert_eq!(pairs[1].keep, big.join("2.txt"));
    }

    #[test]
    fn plan_folder_rejects_a_file_with_no_confirmed_partner() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        let big = dir.path().join("big");
        fs::create_dir_all(&small).unwrap();
        fs::create_dir_all(&big).unwrap();
        fs::write(small.join("1.txt"), b"one").unwrap();
        fs::write(big.join("1.txt"), b"one").unwrap();
        // Nothing in `groups` mentions 2.txt at all -- e.g. it appeared
        // after the scan that produced `groups` ran.
        fs::write(small.join("2.txt"), b"two").unwrap();
        fs::write(big.join("2.txt"), b"two").unwrap();

        let groups = vec![group(3, &[&small.join("1.txt"), &big.join("1.txt")])];

        let err = plan_folder(
            &small,
            &big,
            &groups,
            &ScanOptions::default(),
            ActionKind::Delete,
            &[],
        )
        .expect_err("2.txt has no confirmed partner in `groups`");
        assert!(matches!(
            err,
            FolderActionError::NoConfirmedDuplicate { .. }
        ));
    }

    #[test]
    fn plan_folder_rejects_a_stale_size() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        let big = dir.path().join("big");
        fs::create_dir_all(&small).unwrap();
        fs::create_dir_all(&big).unwrap();
        // The file on disk is bigger than what `groups` recorded -- as if
        // it were edited after the scan that produced `groups` ran.
        fs::write(small.join("1.txt"), b"one-but-longer-now").unwrap();
        fs::write(big.join("1.txt"), b"one").unwrap();

        let groups = vec![group(3, &[&small.join("1.txt"), &big.join("1.txt")])];

        let err = plan_folder(
            &small,
            &big,
            &groups,
            &ScanOptions::default(),
            ActionKind::Delete,
            &[],
        )
        .expect_err("the on-disk size no longer matches the recorded group size");
        assert!(matches!(
            err,
            FolderActionError::NoConfirmedDuplicate { .. }
        ));
    }

    #[test]
    fn plan_folder_rejects_a_nonexistent_removed_folder() {
        let err = plan_folder(
            Path::new("/does/not/exist"),
            Path::new("/also/does/not/exist"),
            &[],
            &ScanOptions::default(),
            ActionKind::Delete,
            &[],
        )
        .expect_err("a nonexistent removed folder must be rejected");
        assert!(matches!(err, FolderActionError::NotADirectory(_)));
    }

    #[test]
    fn apply_folder_delete_removes_every_file_and_prunes_the_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        let big = dir.path().join("big");
        fs::create_dir_all(&small).unwrap();
        fs::create_dir_all(&big).unwrap();
        fs::write(small.join("1.txt"), b"one").unwrap();
        fs::write(big.join("1.txt"), b"one").unwrap();

        let groups = vec![group(3, &[&small.join("1.txt"), &big.join("1.txt")])];
        let plan = plan_folder(
            &small,
            &big,
            &groups,
            &ScanOptions::default(),
            ActionKind::Delete,
            &[],
        )
        .unwrap();
        let report = apply_folder(&plan);

        assert_eq!(report.succeeded, vec![small.join("1.txt")]);
        assert!(report.failed.is_empty());
        assert_eq!(report.bytes_reclaimed, 3);
        assert!(report.directory_removed);
        assert!(!small.exists(), "the emptied removed folder must be pruned");
        assert!(
            big.join("1.txt").exists(),
            "the kept side must be untouched"
        );
    }

    #[test]
    fn apply_folder_trash_removes_every_file_and_prunes_the_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        let big = dir.path().join("big");
        fs::create_dir_all(&small).unwrap();
        fs::create_dir_all(&big).unwrap();
        fs::write(small.join("1.txt"), b"one").unwrap();
        fs::write(big.join("1.txt"), b"one").unwrap();

        let groups = vec![group(3, &[&small.join("1.txt"), &big.join("1.txt")])];
        let plan = plan_folder(
            &small,
            &big,
            &groups,
            &ScanOptions::default(),
            ActionKind::Trash,
            &[],
        )
        .unwrap();
        let report = apply_folder(&plan);

        assert_eq!(report.succeeded, vec![small.join("1.txt")]);
        assert!(report.failed.is_empty());
        assert_eq!(report.bytes_reclaimed, 3);
        assert!(
            report.directory_removed,
            "trash prunes the emptied directory tree just like delete"
        );
        assert!(!small.exists(), "the emptied removed folder must be pruned");
        assert!(
            big.join("1.txt").exists(),
            "the kept side must be untouched"
        );
    }

    #[test]
    fn apply_folder_hardlink_replaces_files_in_place_and_keeps_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        let big = dir.path().join("big");
        fs::create_dir_all(&small).unwrap();
        fs::create_dir_all(&big).unwrap();
        fs::write(small.join("1.txt"), b"one").unwrap();
        fs::write(big.join("1.txt"), b"one").unwrap();

        let groups = vec![group(3, &[&small.join("1.txt"), &big.join("1.txt")])];
        let plan = plan_folder(
            &small,
            &big,
            &groups,
            &ScanOptions::default(),
            ActionKind::Hardlink,
            &[],
        )
        .unwrap();
        let report = apply_folder(&plan);

        assert_eq!(report.succeeded, vec![small.join("1.txt")]);
        assert!(
            !report.directory_removed,
            "hardlink never prunes the folder"
        );
        assert!(small.exists() && small.join("1.txt").exists());
        assert_eq!(fs::read(small.join("1.txt")).unwrap(), b"one");
    }

    #[test]
    fn apply_folder_reports_a_per_file_failure_without_pruning_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        let big = dir.path().join("big");
        fs::create_dir_all(&small).unwrap();
        fs::create_dir_all(&big).unwrap();
        fs::write(small.join("1.txt"), b"one").unwrap();
        fs::write(big.join("1.txt"), b"one").unwrap();

        let groups = vec![group(3, &[&small.join("1.txt"), &big.join("1.txt")])];
        let plan = plan_folder(
            &small,
            &big,
            &groups,
            &ScanOptions::default(),
            ActionKind::Delete,
            &[],
        )
        .unwrap();
        // Remove the file out from under the plan before applying it, so
        // the delete itself fails.
        fs::remove_file(small.join("1.txt")).unwrap();

        let report = apply_folder(&plan);

        assert!(report.succeeded.is_empty());
        assert_eq!(report.failed.len(), 1);
        assert!(
            !report.directory_removed,
            "a failed action must not prune the directory"
        );
    }

    #[test]
    fn plan_folder_excludes_a_protected_file_from_pairs_and_counts_it_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        let big = dir.path().join("big");
        fs::create_dir_all(&small).unwrap();
        fs::create_dir_all(&big).unwrap();
        fs::write(small.join("1.txt"), b"one").unwrap();
        fs::write(big.join("1.txt"), b"one").unwrap();
        fs::write(small.join("2.txt"), b"two").unwrap();
        fs::write(big.join("2.txt"), b"two").unwrap();

        let groups = vec![
            group(3, &[&small.join("1.txt"), &big.join("1.txt")]),
            group(3, &[&small.join("2.txt"), &big.join("2.txt")]),
        ];

        // Protect 1.txt specifically (not the whole `small` folder) --
        // only that one file should be excluded from the plan.
        let plan = plan_folder(
            &small,
            &big,
            &groups,
            &ScanOptions::default(),
            ActionKind::Delete,
            &[small.join("1.txt")],
        )
        .unwrap();

        assert_eq!(plan.pairs.len(), 1);
        assert_eq!(plan.pairs[0].remove, small.join("2.txt"));
        assert_eq!(plan.protected_files_skipped, 1);
        assert_eq!(plan.bytes_reclaimed, 3);
    }

    #[test]
    fn apply_folder_never_prunes_the_directory_when_a_protected_file_was_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        let big = dir.path().join("big");
        fs::create_dir_all(&small).unwrap();
        fs::create_dir_all(&big).unwrap();
        fs::write(small.join("1.txt"), b"one").unwrap();
        fs::write(big.join("1.txt"), b"one").unwrap();
        fs::write(small.join("2.txt"), b"two").unwrap();
        fs::write(big.join("2.txt"), b"two").unwrap();

        let groups = vec![
            group(3, &[&small.join("1.txt"), &big.join("1.txt")]),
            group(3, &[&small.join("2.txt"), &big.join("2.txt")]),
        ];

        let plan = plan_folder(
            &small,
            &big,
            &groups,
            &ScanOptions::default(),
            ActionKind::Delete,
            &[small.join("1.txt")],
        )
        .unwrap();
        let report = apply_folder(&plan);

        assert_eq!(report.succeeded, vec![small.join("2.txt")]);
        assert!(report.failed.is_empty());
        assert!(
            !report.directory_removed,
            "the directory must never be pruned while a protected file still lives in it -- \
             remove_dir_all cannot tell protected files from anything else"
        );
        assert!(
            small.exists() && small.join("1.txt").exists(),
            "the protected file, and the directory holding it, must survive"
        );
        assert!(
            !small.join("2.txt").exists(),
            "the unprotected duplicate is still removed"
        );
    }
}
