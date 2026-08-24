use std::io;
use std::path::PathBuf;

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
    pub path: PathBuf,
    #[source]
    pub source: io::Error,
}
