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

/// What to do with every redundant copy in a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    /// Remove the redundant copy outright.
    Delete,
    /// Replace the redundant copy with a hardlink to the kept file, freeing
    /// its storage while every path involved keeps working.
    Hardlink,
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
/// alphabetically-first path per [`DuplicateGroup`]'s sort invariant).
///
/// A path whose current on-disk identity can't be determined (e.g. it
/// vanished since the scan) is still included in the plan — `apply` will
/// surface that as a per-file failure rather than `plan` silently dropping
/// it, keeping the plan an honest preview of what `apply` will attempt.
pub fn plan(group: &DuplicateGroup, kind: ActionKind) -> ActionPlan {
    let kept = group.paths[0].clone();
    let kept_id = get_file_id(&kept).ok();

    let actions: Vec<FileAction> = group.paths[1..]
        .iter()
        .filter(|path| {
            let same_file = kept_id
                .as_ref()
                .and_then(|kept_id| get_file_id(path).ok().map(|id| id == *kept_id))
                .unwrap_or(false);
            !same_file
        })
        .map(|path| FileAction {
            path: path.clone(),
            kind,
        })
        .collect();

    let bytes_reclaimed = group.size * actions.len() as u64;
    ActionPlan {
        size: group.size,
        kept,
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
            ActionKind::Hardlink => hardlink_over(&plan.kept, &action.path),
        };
        match result {
            Ok(()) => {
                report.succeeded.push(action.path.clone());
                report.bytes_reclaimed += plan.size;
            }
            Err(source) => report.failed.push(FileError {
                path: action.path.clone(),
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
        DuplicateGroup { size, paths }
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
        );
        assert_eq!(plan.kept, a);
        let planned: Vec<&PathBuf> = plan.actions.iter().map(|a| &a.path).collect();
        assert_eq!(planned, vec![&b, &c]);
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

        let plan = plan(&group(3, vec![a.clone(), b.clone()]), ActionKind::Delete);
        let report = apply(&plan);

        assert_eq!(report.succeeded, vec![b.clone()]);
        assert!(report.failed.is_empty());
        assert_eq!(report.bytes_reclaimed, 3);
        assert!(a.exists());
        assert!(!b.exists());
    }

    #[test]
    fn apply_hardlink_replaces_redundant_copy_and_preserves_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let plan = plan(&group(3, vec![a.clone(), b.clone()]), ActionKind::Hardlink);
        let report = apply(&plan);

        assert_eq!(report.succeeded, vec![b.clone()]);
        assert!(report.failed.is_empty());
        // b.txt still exists and reads the same content...
        assert_eq!(fs::read(&b).unwrap(), b"dup");
        // ...because it's now the same inode as a.txt, not a separate copy.
        assert_eq!(get_file_id(&a).unwrap(), get_file_id(&b).unwrap());
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
        );
        let report = apply(&plan);

        assert_eq!(report.succeeded, vec![b.clone()]);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].path, missing);
        assert_eq!(report.bytes_reclaimed, 3);
    }

    #[test]
    fn plan_with_only_hardlink_aliases_of_kept_has_no_actions() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let alias = dir.path().join("alias.txt");
        fs::write(&a, b"dup").unwrap();
        fs::hard_link(&a, &alias).unwrap();

        let plan = plan(&group(3, vec![a, alias]), ActionKind::Delete);
        assert!(plan.actions.is_empty());
        assert_eq!(plan.bytes_reclaimed, 0);
    }
}
