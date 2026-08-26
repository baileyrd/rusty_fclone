use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::FileError;

/// Tunables for a scan. All fields have sensible defaults (see [`Default`]);
/// callers only need to override what they care about.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Follow symbolic links during traversal. Default: `false` (ADR-0003).
    pub follow_symlinks: bool,
    /// Cross filesystem/mount-point boundaries during traversal.
    /// Default: `false` (ADR-0003).
    pub cross_filesystems: bool,
    /// Byte-compare hash-matched files before reporting them as duplicates.
    /// Default: `false` (trust the hash; see ADR-0001).
    pub verify_matches: bool,
    /// Files at or below this size (in bytes) skip the partial-hash stage
    /// entirely and go straight to one full hash (ADR-0001).
    pub small_file_threshold: u64,
    /// Chunk length (in bytes) sampled at the head, middle, and tail of a
    /// file during the partial-hash stage, for files larger than
    /// `small_file_threshold`. Independent of `small_file_threshold` since
    /// ADR-0007 — see its rationale for why sharing one constant between
    /// "should we partial-hash at all" and "how much to sample" cost real
    /// throughput on large files.
    pub partial_hash_sample_size: u64,
    /// Number of worker threads in the I/O-bound read pool. `None` (the
    /// default) auto-detects a sensible value from the scan root's
    /// filesystem at scan time: oversubscribed on a rotational disk (Linux
    /// only, best-effort), core count otherwise — see ADR-0008 for why
    /// ADR-0002's original blanket oversubscription default was revised,
    /// and ADR-0013 for the device-aware default this refines it into.
    /// `Some(n)` pins it explicitly and skips detection.
    pub io_threads: Option<usize>,
    /// Path to a `redb` full-file-hash cache. `None` (the default) disables
    /// caching entirely -- opt in explicitly, since it means writing a file
    /// to disk. When set, a file whose `(size, mtime)` match a cached entry
    /// reuses that hash instead of re-reading and re-hashing it; a
    /// newly-computed hash is written back for next time (ADR-0016).
    pub cache_path: Option<PathBuf>,

    /// Off by default. Path to an existing `fclones --cache` database
    /// (e.g. `~/.cache/fclones`) to import full-file hashes from, so a
    /// tree fclones already scanned with `--hash-fn xxhash` doesn't need
    /// re-hashing here. Independent of `cache_path`: usable on its own for
    /// a one-off import, or alongside it so an imported hash is also
    /// persisted for future rusty-fclone-only re-scans (ADR-0019).
    pub fclones_import_path: Option<PathBuf>,

    /// Files smaller than this (in bytes) are skipped during traversal,
    /// before any hashing. `None` (default) disables the check
    /// (`DETECTION-SCAN-FILTERS`).
    pub min_size: Option<u64>,
    /// Files larger than this (in bytes) are skipped during traversal,
    /// before any hashing. `None` (default) disables the check
    /// (`DETECTION-SCAN-FILTERS`).
    pub max_size: Option<u64>,
    /// If set, only files whose extension (case-insensitive, without the
    /// leading `.`) appears in this list are scanned; a file with no
    /// extension is skipped. An empty (but `Some`) list behaves the same
    /// as `None` -- no filtering. Checked before `exclude_extensions`
    /// (`DETECTION-SCAN-FILTERS`).
    pub include_extensions: Option<Vec<String>>,
    /// Files whose extension (case-insensitive, without the leading `.`)
    /// appears in this list are skipped, even if `include_extensions`
    /// would otherwise allow them. A file with no extension is never
    /// excluded by this list (`DETECTION-SCAN-FILTERS`).
    pub exclude_extensions: Option<Vec<String>>,
    /// Directory subtrees to skip entirely during traversal -- neither
    /// their files nor anything beneath them is visited. Matched as a
    /// literal path prefix against the path as traversed (not
    /// canonicalized), so pass these in the same form (relative/absolute)
    /// as the scan root for reliable matching (`DETECTION-SCAN-FILTERS`).
    pub exclude_paths: Vec<PathBuf>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            follow_symlinks: false,
            cross_filesystems: false,
            verify_matches: false,
            small_file_threshold: 128 * 1024,
            partial_hash_sample_size: 16 * 1024,
            io_threads: None,
            cache_path: None,
            fclones_import_path: None,
            min_size: None,
            max_size: None,
            include_extensions: None,
            exclude_extensions: None,
            exclude_paths: Vec::new(),
        }
    }
}

/// A set of files confirmed to have identical content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateGroup {
    /// Size in bytes shared by every file in the group.
    pub size: u64,
    /// Every path reporting this content, including hardlink aliases.
    /// Sorted for stable, diffable output. `Arc<Path>` rather than
    /// `PathBuf`: the same path is cloned across several internal grouping
    /// stages during a scan (ADR-0004's "path storage" note), and an `Arc`
    /// clone is a refcount bump instead of a fresh heap allocation + copy.
    pub paths: Vec<Arc<Path>>,
}

/// One item of a streaming scan result (see ADR-0004).
#[derive(Debug)]
pub enum ScanEvent {
    /// A confirmed set of duplicate files, emitted as soon as it's found —
    /// not batched until the whole tree has been scanned.
    DuplicateGroup(DuplicateGroup),
    /// A single file couldn't be read or stat-ed; the scan continued.
    Error(FileError),
    /// A traversal progress checkpoint, emitted periodically while the
    /// tree is still being walked (ADR-0015). Purely informational —
    /// consumers that only care about results can ignore it. Only ever
    /// appears before `Finished`, alongside `DuplicateGroup`/`Error`.
    Progress(ScanProgress),
    /// The scan has finished; no further events follow.
    Finished(ScanSummary),
}

/// A traversal progress checkpoint (ADR-0015). Counts are cumulative
/// (not deltas since the last checkpoint) and only cover traversal —
/// there's no way to know the total file count in advance, so this is a
/// running counter, not a percentage.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ScanProgress {
    pub files_scanned: u64,
    pub bytes_scanned: u64,
}

/// Aggregate counters reported once a scan completes.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ScanSummary {
    pub files_scanned: u64,
    pub bytes_scanned: u64,
    pub duplicate_groups: u64,
    pub duplicate_files: u64,
}
