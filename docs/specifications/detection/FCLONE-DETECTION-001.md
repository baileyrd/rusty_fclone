# FCLONE-DETECTION-001 — Duplicate File Detection Engine
- Version: 0.1.0
- Status: Implemented (v1 baseline)
- Owners: baileyrd
- Depends on: none
- Supersedes: none

## Purpose and scope

Detect sets of files with byte-identical content within a directory tree, as
fast as practical, and report them as a stream of duplicate groups. This is
the core capability `rusty_fclone` is built around; everything else (an
action layer that deletes/hardlinks/reflinks duplicates, richer CLI
reporting, a GUI) is out of scope for this specification and depends on it.

## Non-goals

- Deciding what to *do* about duplicates (delete, hardlink, reflink, move).
  That's a future capability area consuming this engine's output.
- Near-duplicate / fuzzy / perceptual matching (e.g. similar images, similar
  text). This engine only detects byte-identical content.
- Distributed or network-filesystem-aware scanning beyond what a portable
  blocking-I/O model gets for free.
- Linux-specific I/O fast paths (io_uring, `FIEMAP` extent ordering) — see
  ADR-0002; tracked on the roadmap as a possible v2 direction, not promised
  here.

## Context and terminology

- **Candidate**: a regular file found during traversal, stat-ed but not yet
  read.
- **Representative**: one path chosen (the alphabetically-first) to stand in
  for a set of paths that are already known to be the same file (hardlink
  aliases share a `(device, inode)` / file-id).
- **Size-group**: all representatives sharing an exact file size.
- **Partial hash**: an xxh3-128 hash of three sampled byte ranges (head,
  middle, tail) of a file, used to prune a size-group before paying for a
  full hash.
- **Full hash**: an xxh3-128 hash of a file's entire content.
- **Duplicate group**: the final, reported unit — every path (including
  hardlink aliases) confirmed to share identical content.

## Requirements

- `FCLONE-DETECTION-001-FR-001`: Given a root directory, the engine SHALL
  traverse it and identify every regular file reachable from it, subject to
  the symlink and filesystem-boundary options in FR-004/FR-005.
- `FCLONE-DETECTION-001-FR-002`: The engine SHALL group files by exact byte
  size before any content is read, and SHALL NOT read the content of a file
  whose size is unique within the scanned tree.
- `FCLONE-DETECTION-001-FR-003`: For files sharing a size, the engine SHALL
  narrow candidates with a partial hash (multi-point: head/middle/tail)
  before computing a full hash, except for files at or below
  `ScanOptions::small_file_threshold`, which SHALL go directly to a full
  hash.
- `FCLONE-DETECTION-001-FR-004`: The engine SHALL skip symbolic links during
  traversal by default, and SHALL follow them only when
  `ScanOptions::follow_symlinks` is `true`.
- `FCLONE-DETECTION-001-FR-005`: The engine SHALL stay on the filesystem the
  scan root resides on by default, and SHALL cross filesystem/mount
  boundaries only when `ScanOptions::cross_filesystems` is `true`.
- `FCLONE-DETECTION-001-FR-006`: Files sharing a `(device, inode)` / platform
  file-id (i.e. existing hardlinks) SHALL be hashed at most once, with every
  alias path carried through to any duplicate group the representative ends
  up in.
- `FCLONE-DETECTION-001-FR-007`: The engine SHALL report a set of paths as a
  `DuplicateGroup` only when it contains two or more paths.
- `FCLONE-DETECTION-001-FR-008`: When `ScanOptions::verify_matches` is
  `true`, every hash-confirmed group SHALL additionally be byte-compared
  before being reported; any representative that doesn't byte-match the
  group is excluded from the reported group rather than causing the whole
  group to be dropped.
- `FCLONE-DETECTION-001-FR-009`: A per-file error (permission denied,
  vanished, I/O error) SHALL be reported via `ScanEvent::Error` and SHALL NOT
  abort the scan.
- `FCLONE-DETECTION-001-NFR-001`: Results SHALL be delivered as a stream
  (`ScanEvent`s over a channel) such that a consumer can observe the first
  `DuplicateGroup` before the entire tree has finished being processed.
- `FCLONE-DETECTION-001-NFR-002`: The engine SHALL NOT read the full content
  of a file whose size or partial hash is unique within its size-group.
- `FCLONE-DETECTION-001-NFR-003`: Hashing work SHALL be bounded by available
  CPU parallelism, and blocking file reads SHALL run on a separately sized
  worker pool, so that I/O latency and hashing CPU cost are not serialized
  through the same thread pool (see ADR-0002).

## Architecture and interfaces

See `docs/architecture/SYSTEM-ARCHITECTURE.md` for the full pipeline diagram.
Public API (`crates/rusty_fclone-core/src/lib.rs`):

```rust
pub fn scan(root: impl Into<PathBuf>, options: ScanOptions) -> Result<ScanHandle, ScanError>;

pub struct ScanHandle { /* impl Iterator<Item = ScanEvent> */ }
pub enum ScanEvent { DuplicateGroup(DuplicateGroup), Error(FileError), Finished(ScanSummary) }
pub struct DuplicateGroup { pub size: u64, pub paths: Vec<PathBuf> }
pub struct ScanOptions { pub follow_symlinks: bool, pub cross_filesystems: bool,
                          pub verify_matches: bool, pub small_file_threshold: u64,
                          pub io_threads: usize }
```

Internal modules: `traversal` (jwalk-based walk + file-id), `io_pool`
(hand-rolled blocking read workers), `hash` (xxh3-128 + sampling), `pipeline`
(orchestration: hardlink collapse → size-group → partial hash → full hash →
optional verify → emit).

## Data/state and invariants

- A `DuplicateGroup.paths` list is always sorted and always has length ≥ 2.
- Every path in a `DuplicateGroup` shares exactly `DuplicateGroup.size` bytes
  on disk (trivially true for hardlink aliases; hash-confirmed, optionally
  byte-verified, for distinct inodes).
- `ScanSummary.duplicate_files` counts every path across every emitted
  group, including hardlink aliases.

## Errors, failure, recovery, and observability

- Traversal errors (unreadable directory, broken entry) and per-file read
  errors both surface as `ScanEvent::Error(FileError { path, source })`.
- A non-directory or non-existent root is rejected synchronously by `scan()`
  as `ScanError::InvalidRoot`, before any background work starts.
- No structured logging/tracing exists yet in v1 — see roadmap.

## Security, privacy, and compatibility

- No adversarial-input hardening: xxh3-128 is non-cryptographic by design
  (ADR-0001). Do not reuse this engine as-is for content-addressing untrusted
  uploads without revisiting the hash choice.
- No special handling of file permissions/ACLs/extended attributes beyond
  what's needed to read file content; those are out of scope for detection.

## Acceptance criteria

- All functional requirements above are exercised by an automated test in
  `crates/rusty_fclone-core/src/*.rs` (`#[cfg(test)]` modules), or explicitly
  listed as an open question below if not yet covered.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  and `cargo test --workspace` all pass on the pinned toolchain.

## Verification plan

Unit/integration tests per module (traversal, hashing, I/O pool, end-to-end
pipeline via `pipeline::tests`). No benchmark suite yet — tracked on the
roadmap; the naive `HashMap`-based data model (ADR-0004) is meant to be
revisited only once a benchmark demonstrates it's the bottleneck.

## Traceability

See `docs/traceability/TRACEABILITY.md`.

## Open questions

- Symlink-cycle handling when `follow_symlinks = true` relies entirely on
  jwalk's own loop detection; no dedicated test exercises this path yet.
- No benchmark exists yet to validate the "fastest possible" goal against
  fclones or a synthetic large-tree workload.
- Streaming full-file hashing (avoiding buffering an entire large file
  before hashing it) is not implemented; see ADR-0002's implementation note.

## Change history

- 0.1.0 (2026-08-24): Initial specification, written against the v1
  baseline implementation landed alongside ADR-0001 through ADR-0006.
