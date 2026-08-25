use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use thiserror::Error;

/// A failure that prevents a scan from starting at all.
#[derive(Debug, Error)]
pub enum ScanError {
    #[error("root path does not exist or is not a directory: {0}")]
    InvalidRoot(PathBuf),
}

/// A failure reading or stat-ing a single file encountered mid-scan.
///
/// Per-file failures never abort a scan (see ADR-0004): they are collected
/// and surfaced as `ScanEvent::Error` alongside whatever duplicate groups
/// were still found.
#[derive(Debug, Error)]
#[error("{path}: {source}")]
pub struct FileError {
    pub path: Arc<Path>,
    #[source]
    pub source: io::Error,
}

/// A failure that prevents a [`crate::folder_action::plan_folder`] call
/// from producing a plan at all (ADR-0023). Fails closed: any of these
/// means no plan is returned, not a partial one.
#[derive(Debug, Error)]
pub enum FolderActionError {
    #[error("{0} is not a directory")]
    NotADirectory(PathBuf),
    /// `path` (inside the folder being acted on) has no confirmed
    /// duplicate at `expected_partner` (inside the folder being kept) in
    /// the `DuplicateGroup`s supplied to `plan_folder` — either the scan
    /// that produced them is stale (something on disk changed since), or
    /// the caller passed a `removed`/`kept` pair that doesn't actually
    /// hold the folder-match relationship it claims to.
    #[error("{path} has no confirmed duplicate at {expected_partner}")]
    NoConfirmedDuplicate {
        path: PathBuf,
        expected_partner: PathBuf,
    },
}
