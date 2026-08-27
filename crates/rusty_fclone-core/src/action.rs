//! Turning a [`DuplicateGroup`] into disk-space savings (ADR-0009).
//!
//! This module only *plans and applies* actions on an already-confirmed
//! group; it does no detection of its own. `plan` is pure and side-effect
//! free (safe to call in a dry run); `apply` is the only function that
//! touches the filesystem.

use std::fs;
use std::path::{Path, PathBuf};

use file_id::get_file_id;

use crate::error::FileError;
use crate::model::DuplicateGroup;
use crate::select;

/// What to do with every redundant copy in a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    /// Remove the redundant copy outright — permanently, with no recovery
    /// path. Prefer [`ActionKind::Trash`] unless a permanent, unrecoverable
    /// delete is specifically wanted.
    Delete,
    /// Move the redundant copy to the operating system's trash/recycle bin
    /// (`ACTION-TRASH`) instead of deleting it outright — recoverable
    /// through the OS's own trash UI, the same safety net most comparable
    /// tools default to. Uses the `trash` crate's freedesktop.org trash
    /// spec implementation on Linux, the Recycle Bin on Windows, and the
    /// Trash on macOS.
    Trash,
    /// Replace the redundant copy with a hardlink to the kept file, freeing
    /// its storage while every path involved keeps working.
    Hardlink,
    /// Replace the redundant copy with a copy-on-write clone (reflink) of
    /// the kept file: an independent inode that shares the kept file's
    /// data blocks until either is modified, freeing storage today without
    /// coupling the two paths' futures the way a hardlink does. Only
    /// supported on filesystems with CoW cloning (Btrfs, XFS with reflink
    /// enabled, APFS, ZFS on some setups) — fails per-file, not silently,
    /// wherever it isn't (ADR-0014).
    Reflink,
}

/// One redundant copy and what will happen to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAction {
    pub path: PathBuf,
    pub kind: ActionKind,
}

/// What running an [`ActionKind`] over a [`DuplicateGroup`] would do,
/// computed without touching the filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionPlan {
    /// Size (bytes) of one copy — the same for every path in the group.
    pub size: u64,
    /// The path kept as-is (the alphabetically-first path in the group,
    /// matching `DuplicateGroup::paths`'s existing sort order).
    pub kept: PathBuf,
    /// Redundant copies that would be acted on. Paths that are already
    /// hardlink aliases of `kept` are deliberately excluded — they share
    /// its inode already, so there is nothing to reclaim by acting on them
    /// (ADR-0009).
    pub actions: Vec<FileAction>,
    /// Bytes this plan would free: `size * actions.len()`.
    pub bytes_reclaimed: u64,
}

/// The outcome of actually running an [`ActionPlan`].
#[derive(Debug, Default)]
pub struct ApplyReport {
    pub succeeded: Vec<PathBuf>,
    pub failed: Vec<FileError>,
    /// Bytes actually freed — `size * succeeded.len()`, not the plan's
    /// (possibly optimistic, if some actions fail) `bytes_reclaimed`.
    pub bytes_reclaimed: u64,
}

/// Plans `kind` for every redundant copy in `group`, without touching the
/// filesystem. The kept path is `group.paths[0]` (already the
/// alphabetically-first path per [`DuplicateGroup`]'s sort invariant),
/// unless `reference_paths` overrides it — see [`plan_with_keep`].
///
/// A path whose current on-disk identity can't be determined (e.g. it
/// vanished since the scan) is still included in the plan — `apply` will
/// surface that as a per-file failure rather than `plan` silently dropping
/// it, keeping the plan an honest preview of what `apply` will attempt.
pub fn plan(group: &DuplicateGroup, kind: ActionKind, reference_paths: &[PathBuf]) -> ActionPlan {
    plan_with_keep(group, &group.paths[0], kind, reference_paths)
}

/// Like [`plan`], but the caller proposes `keep` instead of always
/// `group.paths[0]` — the entry point for rule-driven bulk selection
/// (`SELECTION-RULES`, `crate::select::choose_keep`). `keep` is expected to
/// be one of `group.paths` (every caller in this codebase gets it from
/// there); passing a path that isn't just means every real path in
/// `group.paths` ends up planned, since none of them equals `keep`.
///
/// `reference_paths` (`ACTION-REFERENCE-FOLDERS`, ADR-0025) is a hard,
/// fails-closed guardrail on top of that proposal: if `group` contains a
/// path under any of them, that path is used as the *actual* kept path
/// instead of the caller's `keep` — a reference folder's contents are
/// never the ones flagged for removal, so this can't be bypassed by an
/// upstream caller (a manual GUI keep-choice click included) proposing a
/// different, unprotected path. Independently, any other protected path
/// still present in `group` (a group can contain more than one, if
/// several reference folders each hold a copy) is filtered out of
/// `actions` too, exactly like an existing hardlink alias of the kept
/// file — never placed in `actions`, regardless of `keep`.
pub fn plan_with_keep(
    group: &DuplicateGroup,
    keep: &Path,
    kind: ActionKind,
    reference_paths: &[PathBuf],
) -> ActionPlan {
    let keep: &Path = match select::protected_member(group, reference_paths) {
        Some(protected) => protected.as_ref(),
        None => keep,
    };
    let keep_id = get_file_id(keep).ok();

    let actions: Vec<FileAction> = group
        .paths
        .iter()
        .filter(|path| path.as_ref() != keep)
        .filter(|path| !select::is_protected(path, reference_paths))
        .filter(|path| {
            let same_file = keep_id
                .as_ref()
                .and_then(|keep_id| get_file_id(path).ok().map(|id| id == *keep_id))
                .unwrap_or(false);
            !same_file
        })
        .map(|path| FileAction {
            path: path.to_path_buf(),
            kind,
        })
        .collect();

    let bytes_reclaimed = group.size * actions.len() as u64;
    ActionPlan {
        size: group.size,
        kept: keep.to_path_buf(),
        actions,
        bytes_reclaimed,
    }
}

/// Executes `plan` against the filesystem. Per-file failures (permission
/// denied, vanished, cross-device hardlink) are collected and don't abort
/// the rest of the plan, matching the detection engine's error-tolerance
/// contract (ADR-0004).
pub fn apply(plan: &ActionPlan) -> ApplyReport {
    let mut report = ApplyReport::default();
    for action in &plan.actions {
        let result = match action.kind {
            ActionKind::Delete => fs::remove_file(&action.path),
            ActionKind::Trash => trash::delete(&action.path).map_err(std::io::Error::other),
            ActionKind::Hardlink => hardlink_over(&plan.kept, &action.path),
            ActionKind::Reflink => reflink_over(&plan.kept, &action.path),
        };
        match result {
            Ok(()) => {
                report.succeeded.push(action.path.clone());
                report.bytes_reclaimed += plan.size;
            }
            Err(source) => report.failed.push(FileError {
                path: action.path.clone().into(),
                source,
            }),
        }
    }
    report
}

/// Replaces `path` with a hardlink to `kept`, safely: link to a temporary
/// name first, then rename over `path`. This means `path` is never
/// momentarily missing if the process is interrupted mid-operation — the
/// same pattern fclones and rmlint use, rather than removing `path` first
/// and linking second (which leaves nothing at `path` if the link step
/// fails).
fn hardlink_over(kept: &Path, path: &Path) -> std::io::Result<()> {
    let tmp = tmp_sibling(path);
    fs::hard_link(kept, &tmp)?;
    fs::rename(&tmp, path)
}

/// Replaces `path` with a reflink (copy-on-write clone) of `kept`, using
/// the same safe temp-then-rename pattern as [`hardlink_over`]. Unlike
/// [`hardlink_over`], the temp file *is* created by the underlying reflink
/// call before the clone ioctl runs, so a failed clone can leave an empty
/// stub behind — cleaned up here rather than left as filesystem litter.
///
/// Deliberately does not fall back to a plain copy when reflink isn't
/// supported: `reflink_copy::reflink` (not `reflink_or_copy`) fails with
/// an `io::Error`, surfaced to the caller as a per-file failure like any
/// other action error (ADR-0014). A silent copy fallback would look like
/// it worked while not actually freeing any space — the one outcome this
/// action exists to produce.
fn reflink_over(kept: &Path, path: &Path) -> std::io::Result<()> {
    let tmp = tmp_sibling(path);
    if let Err(err) = reflink_copy::reflink(kept, &tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    fs::rename(&tmp, path)
}

fn tmp_sibling(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    path.with_file_name(format!(".rusty-fclone-tmp-{file_name}-{unique}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn group(size: u64, paths: Vec<PathBuf>) -> DuplicateGroup {
        DuplicateGroup {
            size,
            paths: paths.into_iter().map(Into::into).collect(),
        }
    }

    #[test]
    fn plans_every_non_kept_path() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        let c = dir.path().join("c.txt");
        for p in [&a, &b, &c] {
            fs::write(p, b"dup").unwrap();
        }

        let plan = plan(
            &group(3, vec![a.clone(), b.clone(), c.clone()]),
            ActionKind::Delete,
            &[],
        );
        assert_eq!(plan.kept, a);
        let planned: Vec<&PathBuf> = plan.actions.iter().map(|a| &a.path).collect();
        assert_eq!(planned, vec![&b, &c]);
        assert_eq!(plan.bytes_reclaimed, 6);
    }

    #[test]
    fn plan_with_keep_honors_an_explicit_non_default_kept_path() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        let c = dir.path().join("c.txt");
        for p in [&a, &b, &c] {
            fs::write(p, b"dup").unwrap();
        }

        let plan = plan_with_keep(
            &group(3, vec![a.clone(), b.clone(), c.clone()]),
            &b,
            ActionKind::Delete,
            &[],
        );
        assert_eq!(plan.kept, b);
        let planned: Vec<&PathBuf> = plan.actions.iter().map(|a| &a.path).collect();
        assert_eq!(planned, vec![&a, &c]);
        assert_eq!(plan.bytes_reclaimed, 6);
    }

    #[test]
    fn plan_skips_existing_hardlink_aliases_of_kept() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let alias = dir.path().join("alias.txt"); // hardlink of a
        let c = dir.path().join("c.txt"); // separate inode, same content
        fs::write(&a, b"dup").unwrap();
        fs::hard_link(&a, &alias).unwrap();
        fs::write(&c, b"dup").unwrap();

        let plan = plan(
            &group(3, vec![a.clone(), alias.clone(), c.clone()]),
            ActionKind::Hardlink,
            &[],
        );
        // alias already shares a's inode -- nothing to reclaim there.
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].path, c);
        assert_eq!(plan.bytes_reclaimed, 3);
    }

    #[test]
    fn apply_delete_removes_redundant_copies_and_keeps_the_kept_file() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let plan = plan(
            &group(3, vec![a.clone(), b.clone()]),
            ActionKind::Delete,
            &[],
        );
        let report = apply(&plan);

        assert_eq!(report.succeeded, vec![b.clone()]);
        assert!(report.failed.is_empty());
        assert_eq!(report.bytes_reclaimed, 3);
        assert!(a.exists());
        assert!(!b.exists());
    }

    #[test]
    fn apply_trash_removes_the_redundant_copy_from_its_original_path_and_keeps_the_kept_file() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let plan = plan(
            &group(3, vec![a.clone(), b.clone()]),
            ActionKind::Trash,
            &[],
        );
        let report = apply(&plan);

        assert_eq!(report.succeeded, vec![b.clone()]);
        assert!(report.failed.is_empty());
        assert_eq!(report.bytes_reclaimed, 3);
        assert!(a.exists(), "the kept file must be untouched");
        assert!(
            !b.exists(),
            "the redundant copy must be gone from its original path (moved to the OS trash)"
        );
    }

    #[test]
    fn apply_hardlink_replaces_redundant_copy_and_preserves_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let plan = plan(
            &group(3, vec![a.clone(), b.clone()]),
            ActionKind::Hardlink,
            &[],
        );
        let report = apply(&plan);

        assert_eq!(report.succeeded, vec![b.clone()]);
        assert!(report.failed.is_empty());
        // b.txt still exists and reads the same content...
        assert_eq!(fs::read(&b).unwrap(), b"dup");
        // ...because it's now the same inode as a.txt, not a separate copy.
        assert_eq!(get_file_id(&a).unwrap(), get_file_id(&b).unwrap());
    }

    #[test]
    fn apply_reflink_succeeds_or_fails_cleanly_depending_on_filesystem_support() {
        // Reflink only works on CoW-capable filesystems (Btrfs, XFS with
        // reflink, APFS, some ZFS setups); most CI runners and this
        // sandbox's tempdir are not one. Both outcomes are correct
        // behavior here -- what must hold either way is ADR-0014's
        // contract: no silent copy fallback, the kept file is untouched,
        // and a failure leaves the redundant copy exactly as it was
        // (no stray temp file, no data loss).
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let plan = plan(
            &group(3, vec![a.clone(), b.clone()]),
            ActionKind::Reflink,
            &[],
        );
        let report = apply(&plan);

        assert!(a.exists(), "the kept file must survive either way");
        assert_eq!(fs::read(&a).unwrap(), b"dup");
        assert!(b.exists(), "the redundant path must never vanish");
        assert_eq!(
            fs::read(&b).unwrap(),
            b"dup",
            "content must be correct whether reflinked or left untouched"
        );

        let stray_temp_files: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains("rusty-fclone-tmp")
            })
            .collect();
        assert!(
            stray_temp_files.is_empty(),
            "a failed reflink must not leave a temp file behind"
        );

        if report.failed.is_empty() {
            assert_eq!(report.succeeded, vec![b.clone()]);
            assert_eq!(report.bytes_reclaimed, 3);
        } else {
            assert_eq!(report.failed.len(), 1);
            assert_eq!(report.failed[0].path.as_ref(), b.as_path());
            assert_eq!(report.bytes_reclaimed, 0);
        }
    }

    #[test]
    fn apply_reports_per_file_failure_without_aborting_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let missing = dir.path().join("missing.txt"); // never created
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let plan = plan(
            &group(3, vec![a.clone(), missing.clone(), b.clone()]),
            ActionKind::Delete,
            &[],
        );
        let report = apply(&plan);

        assert_eq!(report.succeeded, vec![b.clone()]);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].path.as_ref(), missing.as_path());
        assert_eq!(report.bytes_reclaimed, 3);
    }

    #[test]
    fn plan_with_only_hardlink_aliases_of_kept_has_no_actions() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let alias = dir.path().join("alias.txt");
        fs::write(&a, b"dup").unwrap();
        fs::hard_link(&a, &alias).unwrap();

        let plan = plan(&group(3, vec![a, alias]), ActionKind::Delete, &[]);
        assert!(plan.actions.is_empty());
        assert_eq!(plan.bytes_reclaimed, 0);
    }

    #[test]
    fn plan_overrides_an_explicit_keep_when_a_different_path_is_protected() {
        let dir = tempfile::tempdir().unwrap();
        let reference = dir.path().join("reference");
        fs::create_dir_all(&reference).unwrap();
        let protected = reference.join("original.txt");
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&protected, b"dup").unwrap();
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        // `plan_with_keep` is asked to keep `a` -- alphabetically first,
        // and not the protected path -- but the guardrail must win anyway.
        let plan = plan_with_keep(
            &group(3, vec![a.clone(), b.clone(), protected.clone()]),
            &a,
            ActionKind::Delete,
            &[reference],
        );
        assert_eq!(
            plan.kept, protected,
            "the protected path must be kept even though a different path was requested"
        );
        let planned: Vec<&PathBuf> = plan.actions.iter().map(|a| &a.path).collect();
        assert_eq!(
            planned,
            vec![&a, &b],
            "every unprotected copy is still planned for removal"
        );
        assert_eq!(plan.bytes_reclaimed, 6);
    }

    #[test]
    fn plan_never_includes_a_protected_path_in_actions_even_when_it_is_not_kept() {
        let dir = tempfile::tempdir().unwrap();
        let reference_one = dir.path().join("reference-one");
        let reference_two = dir.path().join("reference-two");
        fs::create_dir_all(&reference_one).unwrap();
        fs::create_dir_all(&reference_two).unwrap();
        let protected_one = reference_one.join("a.txt");
        let protected_two = reference_two.join("b.txt");
        fs::write(&protected_one, b"dup").unwrap();
        fs::write(&protected_two, b"dup").unwrap();

        // Two reference folders each hold a copy of the same content --
        // whichever one `select::protected_member` picks as kept, the
        // *other* protected path must still never be planned, exactly
        // like an existing hardlink alias of the kept file.
        let plan = plan(
            &group(3, vec![protected_one.clone(), protected_two.clone()]),
            ActionKind::Delete,
            &[reference_one, reference_two],
        );
        assert_eq!(plan.kept, protected_one);
        assert!(
            plan.actions.is_empty(),
            "the second protected path must not be planned for removal either"
        );
        assert_eq!(plan.bytes_reclaimed, 0);
    }

    #[test]
    fn apply_never_touches_a_protected_path() {
        let dir = tempfile::tempdir().unwrap();
        let reference = dir.path().join("reference");
        fs::create_dir_all(&reference).unwrap();
        let protected = reference.join("original.txt");
        let redundant = dir.path().join("copy.txt");
        fs::write(&protected, b"dup").unwrap();
        fs::write(&redundant, b"dup").unwrap();

        let plan = plan_with_keep(
            &group(3, vec![redundant.clone(), protected.clone()]),
            &redundant,
            ActionKind::Delete,
            &[reference],
        );
        let report = apply(&plan);

        assert_eq!(report.succeeded, vec![redundant.clone()]);
        assert!(protected.exists(), "the protected file must survive");
        assert!(!redundant.exists(), "its unprotected duplicate is removed");
    }
}
