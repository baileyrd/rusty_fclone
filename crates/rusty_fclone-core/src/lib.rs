//! Duplicate-file detection engine for rusty_fclone.
//!
//! The public surface is deliberately small: [`scan`] starts a scan and
//! returns a [`ScanHandle`] that streams [`ScanEvent`]s as duplicate groups
//! are confirmed, rather than collecting everything before returning
//! anything. See `docs/decisions/` in the repository for the architectural
//! decisions (ADR-0001 through ADR-0006) this crate implements.

mod error;
mod hash;
mod io_pool;
mod model;
mod pipeline;
mod traversal;

pub use error::{FileError, ScanError};
pub use model::{DuplicateGroup, ScanEvent, ScanOptions, ScanSummary};
pub use pipeline::{scan, ScanHandle};
