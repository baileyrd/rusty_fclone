//! Duplicate-file detection and deduplication engine for rusty_fclone.
//!
//! Detection's public surface is deliberately small: [`scan`] starts a scan
//! and returns a [`ScanHandle`] that streams [`ScanEvent`]s as duplicate
//! groups are confirmed, rather than collecting everything before returning
//! anything. [`action::plan`] and [`action::apply`] turn a confirmed
//! [`DuplicateGroup`] into disk-space savings (delete or hardlink redundant
//! copies) — see `docs/decisions/` for the architectural decisions
//! (ADR-0001 through ADR-0009) this crate implements.

pub mod action;
mod device;
mod error;
mod hash;
mod io_pool;
mod model;
mod pipeline;
mod traversal;

pub use error::{FileError, ScanError};
pub use model::{DuplicateGroup, ScanEvent, ScanOptions, ScanProgress, ScanSummary};
pub use pipeline::{scan, ScanHandle};
