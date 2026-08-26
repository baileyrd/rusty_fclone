# FCLONE-DETECTION-001 — Duplicate File Detection Engine
- Version: 0.2.1
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
  That's a future capability area consuming this engine's output. This
  extends to folder-level matches (FR-010 onward) too: no new "delete/
  replace this whole folder" action exists or is planned here — a folder
  match is acted on today via the existing per-file actions on the
  `DuplicateGroup`s it's built from (ADR-0021).
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
  narrow candidates with a partial hash (multi-point: head/middle/tail,
  each `ScanOptions::partial_hash_sample_size` bytes) before computing a
  full hash, except for files at or below `ScanOptions::small_file_threshold`,
  which SHALL go directly to a full hash. The two options are independent
  (ADR-0007) — `small_file_threshold` alone decides whether the partial
  stage runs at all.
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
  worker pool (`ScanOptions::io_threads`; `None` auto-detects from the scan
  root's device type at scan time, oversubscribed on a rotational disk or
  core count otherwise — see ADR-0008, ADR-0013), so that I/O latency and
  hashing CPU cost are not serialized through the same thread pool (see
  ADR-0002).
- `FCLONE-DETECTION-001-NFR-004`: When `ScanOptions::cache_path` is set, a
  file whose `(size, mtime)` match a cached full-hash entry SHALL reuse
  that hash rather than being re-read and re-hashed; a newly-computed full
  hash SHALL be persisted to the cache for future scans (ADR-0016). Caching
  SHALL be off by default and SHALL NOT change detection results — a
  cache hit and a freshly-computed hash for the same unchanged file are
  interchangeable.
- `FCLONE-DETECTION-001-NFR-005`: When `ScanOptions::fclones_import_path`
  is set, a file whose full-content hash was already computed by an
  external `fclones --cache` run (using its `xxhash3` algorithm, at a
  size/mtime that still match) SHALL reuse that hash rather than being
  re-read and re-hashed (ADR-0019). Import SHALL be off by default,
  independent of `cache_path`, and SHALL NOT change detection results —
  any import miss (wrong hash function, no matching entry, stale
  size/mtime, non-Unix platform, unreadable database) SHALL fall through
  to computing the hash normally rather than erroring.
- `FCLONE-DETECTION-001-FR-010`: Given a completed scan's `DuplicateGroup`s,
  `find_folder_duplicates` SHALL identify directories whose entire
  recursive file content is pairwise identical to another directory's
  (`FolderMatch::Exact`, two or more directories) or a strict subset of
  another directory's (`FolderMatch::Contained`, `subset`/`superset`),
  using each directory's complete recursive file listing — including
  files with no duplicate anywhere, which `scan()` never surfaces —
  obtained via its own second, stat-only, no-hashing traversal (ADR-0021).
- `FCLONE-DETECTION-001-FR-011`: A directory SHALL only be eligible as an
  `Exact` participant or a `Contained` subset when every file in its
  recursive subtree has at least one duplicate elsewhere in the tree; a
  single unmatched file anywhere in its subtree SHALL disqualify it. A
  directory with unmatched files of its own MAY still be eligible as a
  `Contained` match's superset — a superset is only required to contain
  the subset's files, not to consist entirely of them.
- `FCLONE-DETECTION-001-FR-012`: Once a directory is claimed by a
  reported `Exact` cluster or as a `Contained` subset, none of its
  descendants SHALL be separately reported as their own match — a match
  among them is already fully implied by the claimed ancestor's match.
  Superset directories SHALL NOT be claimed this way, since the same
  directory can legitimately be the superset for several unrelated
  subset matches.
- `FCLONE-DETECTION-001-FR-013`: `find_folder_duplicates` SHALL reject a
  nonexistent or non-directory root with `ScanError::InvalidRoot`,
  matching `scan()`'s existing root-validation contract.
- `FCLONE-DETECTION-001-NFR-006`: Candidate superset discovery for a
  subset directory SHALL be bounded by the total membership of the
  `DuplicateGroup`s its files belong to (path-suffix matching against
  each group's other paths), not by comparing every pair of directories
  in the tree.
- `FCLONE-DETECTION-001-FR-014`: The engine SHALL support include/exclude
  scan filters, applied during traversal before any file content is read:
  `ScanOptions::min_size`/`max_size` (inclusive byte bounds, either
  optional), `ScanOptions::include_extensions`/`exclude_extensions`
  (case-insensitive, without the leading `.`; `exclude_extensions` is
  checked after `include_extensions` and wins if both would otherwise
  apply), and `ScanOptions::exclude_paths` (directories or individual
  files skipped entirely, matched as a literal path prefix against the
  path as traversed, not canonicalized). A file excluded by any of these
  SHALL be silently omitted from candidates — this is not a per-file
  error and SHALL NOT be reported via `ScanEvent::Error`
  (`DETECTION-SCAN-FILTERS`).
- `FCLONE-DETECTION-001-NFR-007`: A directory subtree covered by
  `ScanOptions::exclude_paths` SHALL NOT be descended into at all —
  pruned before traversal reads its contents, not filtered out of
  results afterward (`DETECTION-SCAN-FILTERS`).

## Architecture and interfaces

See `docs/architecture/SYSTEM-ARCHITECTURE.md` for the full pipeline diagram.
Public API (`crates/rusty_fclone-core/src/lib.rs`):

```rust
pub fn scan(root: impl Into<PathBuf>, options: ScanOptions) -> Result<ScanHandle, ScanError>;

pub struct ScanHandle { /* impl Iterator<Item = ScanEvent> */ }
pub enum ScanEvent { DuplicateGroup(DuplicateGroup), Error(FileError),
                      Progress(ScanProgress), Finished(ScanSummary) }
pub struct ScanProgress { pub files_scanned: u64, pub bytes_scanned: u64 }
pub struct DuplicateGroup { pub size: u64, pub paths: Vec<Arc<Path>> }
pub struct ScanOptions { pub follow_symlinks: bool, pub cross_filesystems: bool,
                          pub verify_matches: bool, pub small_file_threshold: u64,
                          pub partial_hash_sample_size: u64, pub io_threads: Option<usize>,
                          pub cache_path: Option<PathBuf>,
                          pub fclones_import_path: Option<PathBuf>,
                          pub min_size: Option<u64>, pub max_size: Option<u64>,
                          pub include_extensions: Option<Vec<String>>,
                          pub exclude_extensions: Option<Vec<String>>,
                          pub exclude_paths: Vec<PathBuf> }

pub fn find_folder_duplicates(root: &Path, groups: &[DuplicateGroup], options: &ScanOptions)
    -> Result<Vec<FolderMatch>, ScanError>;
pub enum FolderMatch { Exact { folders: Vec<PathBuf>, file_count: u64, bytes: u64 },
                        Contained { subset: PathBuf, superset: PathBuf,
                                    file_count: u64, bytes: u64 } }
```

Internal modules: `traversal` (jwalk-based walk + file-id; also applies
`min_size`/`max_size`/`include_extensions`/`exclude_extensions` per file and
prunes `exclude_paths` subtrees via jwalk's `process_read_dir` before
descending, `DETECTION-SCAN-FILTERS`), `io_pool`
(hand-rolled blocking read workers), `hash` (xxh3-128 + sampling), `device`
(Linux rotational-disk detection for `io_threads` auto-sizing), `cache`
(`redb`-backed full-hash cache, ADR-0016), `fclones_import` (reads an
existing fclones `sled`/`bincode` cache database, ADR-0019), `pipeline`
(orchestration: hardlink collapse → size-group → partial hash → full hash →
optional verify → emit), `folder_dedup` (post-scan folder-level duplicate
detection consuming a completed scan's `DuplicateGroup`s, ADR-0021).

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
- `tracing` spans/events cover the traversal and pipeline stages
  (ADR-0010); `ScanEvent::Progress` (0.1.7, ADR-0015) gives consumers a
  progress signal independent of whatever logging level they've enabled.

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
(`DETECTION-BENCHMARK-VS-FCLONES` on the roadmap): after ADR-0007 and
ADR-0008, rusty_fclone wins ~2.6–2.7x on small-file-heavy trees and is
within measurement noise of fclones' best-tuned configuration on a
large-file scenario (beating its default configuration outright).
The naive `HashMap`-based data model (ADR-0004) is meant to be revisited
only once a benchmark demonstrates it's the bottleneck.

## Traceability

See `docs/traceability/TRACEABILITY.md`.

## Open questions

- "Fastest possible" is now a measured claim across all four benchmark
  scenarios: rusty_fclone wins ~2.6–2.7x on small-file-heavy trees and is
  within measurement noise of (fractionally behind fclones' best-tuned
  configuration, ahead of its default) on the large-file scenario — see
  `docs/benchmarks/FCLONES-COMPARISON.md`. Getting there took two ADRs
  (0007, 0008); the first hypothesis tested (partial-hash sample size) was
  wrong for the specific benchmark scenario that motivated it, which the
  comparison doc documents rather than quietly correcting.
- The `few_large_duplicates` benchmark scenario reads the same files
  repeatedly across iterations; after the first iteration these reads are
  served from the OS page cache, so its reported throughput reflects warm-
  cache performance, not raw disk I/O speed. This is intentional (repeat
  scans of an unchanged tree are a realistic use case) but worth reading
  the numbers with that caveat in mind.
- `FCLONE-DETECTION-001-NFR-001`'s test verifies the streaming *contract*
  (groups always precede `Finished`) but not actual wall-clock overlap
  between traversal and hashing — see `DETECTION-STREAMING-OVERLAP` on the
  roadmap.

## Change history

- 0.2.1 (2026-08-26): Added include/exclude scan filters (FR-014, NFR-007)
  — `ScanOptions::min_size`/`max_size`, `include_extensions`/
  `exclude_extensions`, and `exclude_paths`, all applied during traversal
  before any hashing. `exclude_paths` prunes whole subtrees via jwalk's
  `process_read_dir` rather than filtering results after the fact —
  matching directories are never descended into
  (`DETECTION-SCAN-FILTERS`, first unit of the phased gap-closure plan in
  `docs/roadmap/DEDUP-GAP-IMPLEMENTATION-PLAN.md`). No existing behavior
  changed: every new field defaults to `None`/empty (no filtering).
- 0.2.0 (2026-08-25): Added folder-level duplicate detection (FR-010
  through FR-013, NFR-006) — asked for directly ("is it possible to
  identify if folders of files are duplicates?", both exact and
  partial/subset matching wanted). New `folder_dedup` module and public
  `find_folder_duplicates(root, groups, options) -> Result<Vec<FolderMatch>,
  ScanError>` function: a post-scan analysis (not a `scan()`/`ScanEvent`
  streaming extension, since a folder verdict needs the whole tree's
  picture) that runs its own lightweight second, stat-only traversal to
  learn the complete file set per directory, then reports `Exact` clusters
  and `Contained` subset/superset pairs, with a "fully duplicated subtree"
  eligibility gate and shallowest-first redundancy suppression so a
  top-level folder match doesn't flood the output with every implied
  nested subdirectory match. No new destructive action — detection and
  reporting only (ADR-0021). New CLI `--find-duplicate-folders` flag
  (`CLI-UX-001`); GUI surfacing scoped as a separate follow-up, not
  bundled into this change.
- 0.1.9 (2026-08-24): Added
  `ScanOptions::fclones_import_path: Option<PathBuf>` (NFR-005) and a new
  `fclones_import` module: reads an existing `fclones --cache` `sled`
  database and reuses a file's already-computed full hash when fclones
  used the same `xxhash3` algorithm and the entry isn't stale. Tried after
  a `--cache` miss, before any real I/O; an imported hit is also written
  to `--cache` if set. Off by default, independent of `cache_path`; no
  change to detection results either way. ADR-0019, new CLI
  `--import-fclones-cache <path>` flag.
- 0.1.8 (2026-08-24): Added `ScanOptions::cache_path: Option<PathBuf>`
  (NFR-004) and a new `cache` module: an opt-in, `redb`-backed cache of
  each file's full hash, keyed by path and invalidated by `(size, mtime)`.
  A cache hit skips both the partial-hash and full-hash stages for that
  file entirely. Off by default; no change to detection results either
  way. ADR-0016, new CLI `--cache <path>` flag.
- 0.1.7 (2026-08-24): Added `ScanEvent::Progress(ScanProgress)`, a
  traversal progress checkpoint emitted every 256 files scanned, always
  before `Finished`. Consumed by the CLI's new `--format json`/live
  progress line (`CLI-UX-001`, ADR-0015) but is a core-crate API addition,
  not CLI-specific — any `ScanHandle` consumer sees it. No change to
  detection behavior or the existing `Finished`-is-always-last invariant.
- 0.1.6 (2026-08-24): `ScanOptions::io_threads` changes from `usize` to
  `Option<usize>`: `None` (the new default) auto-detects a device-aware
  default at scan time via the new `device` module (oversubscribed on a
  rotational disk, Linux-only best-effort detection via
  `/proc/self/mountinfo` + `/sys/dev/block/*/queue/rotational`; `cores`
  otherwise, matching the prior default) instead of always using `cores`
  regardless of storage type (ADR-0013, `DETECTION-DEVICE-AWARE-IO-SIZING`).
  Also fused traversal and hardlink-collapse into one streaming pass —
  `traversal::traverse` takes an `on_candidate` callback instead of
  returning a `Vec<Candidate>` (ADR-0012, `DETECTION-TRAVERSAL-COLLAPSE-FUSION`)
  — and switched internal/public path storage from `PathBuf` to `Arc<Path>`
  to make cloning a path through the grouping stages a refcount bump
  instead of a fresh allocation (ADR-0011); `DuplicateGroup::paths` and
  `FileError::path` are now `Vec<Arc<Path>>`/`Arc<Path>`. No detection
  algorithm change in any of the three; NFR-003 and the public API surface
  above are updated to match.
- 0.1.5 (2026-08-24): Closed two known gaps. Added
  `traversal::tests::follow_symlinks_terminates_on_a_cycle`, a real symlink
  cycle under `--follow-symlinks` with a bounded timeout, confirming
  jwalk's loop detection actually protects the scan rather than just being
  assumed to. `IoPool::hash_full_file`/`files_equal` now stream full-file
  hashing and `--verify` byte-comparison in fixed 1 MiB chunks instead of
  buffering whole files — `--verify`'s peak memory no longer scales with
  duplicate-group size at all. ADR-0002 addendum; no requirement text
  changed, both were already-implied behavior gaps, not new requirements.
- 0.1.4 (2026-08-24): Closed the large-file benchmark gap found in 0.1.3.
  Split `ScanOptions::small_file_threshold` from a new
  `partial_hash_sample_size` field (ADR-0007, `DETECTION-ADAPTIVE-SAMPLE-SIZE`)
  — a genuine improvement, but re-measuring showed it didn't move the
  `few_large_duplicates` scenario, since every file there is a real
  duplicate and nothing gets pruned by partial hashing regardless of sample
  size. The actual fix was `ScanOptions::io_threads`'s default changing from
  an oversubscribed `cores * 4` to plain `cores` (ADR-0008), after
  benchmarking showed oversubscription hurting throughput on every tested
  scenario, not just the large-file one. Added `--partial-hash-sample-size`
  and `--io-threads` CLI flags. `docs/benchmarks/FCLONES-COMPARISON.md`
  has the full investigation and final numbers.
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
