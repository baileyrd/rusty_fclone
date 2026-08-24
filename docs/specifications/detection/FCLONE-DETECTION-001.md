# FCLONE-DETECTION-001 — Duplicate File Detection Engine
- Version: 0.1.3
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
- A set of paths that are *only* hardlink aliases of one inode, with no
  other file matching their content, is never reported as a
  `DuplicateGroup` — they already share storage, so there is nothing
  actionable to report (see `pipeline::tests::standalone_hardlink_pair_is_not_reported_as_duplicate`).

## Errors, failure, recovery, and observability

- Traversal errors (unreadable directory, broken entry) and per-file read
  errors — during partial-hash, full-hash, *and* `--verify`'s byte-compare
  read — all surface as `ScanEvent::Error(FileError { path, source })`. (The
  hashing/verification-stage reporting was added in the traceability
  gap-closure pass; earlier code silently dropped those failures instead of
  reporting them — see change history.)
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
pipeline via `pipeline::tests`). A Criterion benchmark suite
(`crates/rusty_fclone-core/benches/detection.rs`, run via `cargo bench -p
rusty_fclone-core`) covers four synthetic scenarios — many small
duplicates, many unique small files, few large duplicates, and a mixed
realistic tree — reporting files/sec or bytes/sec. These are relative/
regression benchmarks against this crate's own history. A separate,
documented head-to-head comparison against upstream fclones exists at
`docs/benchmarks/FCLONES-COMPARISON.md`
(`DETECTION-BENCHMARK-VS-FCLONES` on the roadmap): rusty_fclone wins
~1.9–2.0x on small-file-heavy trees but loses ~1.2x on a large-file
scenario, motivating the new `DETECTION-ADAPTIVE-SAMPLE-SIZE` roadmap unit.
The naive `HashMap`-based data model (ADR-0004) is meant to be revisited
only once a benchmark demonstrates it's the bottleneck.

## Traceability

See `docs/traceability/TRACEABILITY.md`.

## Open questions

- Symlink-cycle handling when `follow_symlinks = true` relies entirely on
  jwalk's own loop detection; the gap-closure pass added a test for the
  broken-symlink error-reporting case
  (`traversal_errors_are_reported_and_do_not_abort_the_scan`), but no
  dedicated test exercises an actual symlink *cycle* yet.
- "Fastest possible" is now a measured, not just architectural, claim —
  but a nuanced one: `docs/benchmarks/FCLONES-COMPARISON.md` shows
  rusty_fclone winning on small-file-heavy trees and losing on a large-file
  scenario. Closing that gap is `DETECTION-ADAPTIVE-SAMPLE-SIZE` on the
  roadmap, not yet started.
- The `few_large_duplicates` benchmark scenario reads the same files
  repeatedly across iterations; after the first iteration these reads are
  served from the OS page cache, so its reported throughput reflects warm-
  cache performance, not raw disk I/O speed. This is intentional (repeat
  scans of an unchanged tree are a realistic use case) but worth reading
  the numbers with that caveat in mind.
- Streaming full-file hashing (avoiding buffering an entire large file
  before hashing it) is not implemented; see ADR-0002's implementation note.
- `FCLONE-DETECTION-001-NFR-001`'s test verifies the streaming *contract*
  (groups always precede `Finished`) but not actual wall-clock overlap
  between traversal and hashing — see `DETECTION-STREAMING-OVERLAP` on the
  roadmap.

## Change history

- 0.1.3 (2026-08-24): Added a documented head-to-head benchmark comparison
  against upstream fclones 0.35.0 (`DETECTION-BENCHMARK-VS-FCLONES`) — see
  `docs/benchmarks/FCLONES-COMPARISON.md`. rusty_fclone wins ~1.9–2.0x on
  small-file-heavy trees, loses ~1.2x on a large-file scenario, motivating a
  new `DETECTION-ADAPTIVE-SAMPLE-SIZE` roadmap unit. Also fixed a bug the
  comparison surfaced: `benches/detection.rs`'s synthetic "unique files"
  content generator only varied by `seed mod 256`, so the 2,000-file unique
  scenario silently contained 256 duplicate groups instead of zero; fixed
  by encoding the seed into each file's first 8 bytes. No production-code
  change — the bug was in benchmark fixtures only.
- 0.1.2 (2026-08-24): Added a Criterion benchmark suite
  (`DETECTION-BENCHMARK` on the roadmap) covering four synthetic scan
  scenarios. No behavior change; verification-plan and open-questions
  sections updated to reflect it, and to split off the still-open
  comparison against fclones as its own roadmap unit.
- 0.1.1 (2026-08-24): Closed all traceability gaps flagged "needs dedicated
  unit test" with direct tests (FR-005, FR-006, FR-008, FR-009, NFR-001).
  While closing FR-009, found and fixed a real gap: read failures during the
  partial-hash, full-hash, and `--verify` stages were silently dropped
  instead of being reported via `ScanEvent::Error` — only traversal-stage
  failures were reported before this fix. No architectural decision changed
  (ADR-0004 already required this); this was a bug relative to an existing
  requirement, not a new decision.
- 0.1.0 (2026-08-24): Initial specification, written against the v1
  baseline implementation landed alongside ADR-0001 through ADR-0006.
