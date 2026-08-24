# ADR-0004: Engine API contract — streaming events, failure policy, path storage

- Status: Accepted
- Date: 2026-08-24

## Context

Three related decisions shape `rusty_fclone-core`'s public contract:

1. **Result delivery.** Return one collected `Vec<DuplicateGroup>` only after
   the entire tree is scanned, or stream results incrementally so a consumer
   can act on the first duplicates found while a huge tree is still being
   processed?
2. **Per-file failure policy.** A scan over millions of files will hit
   permission-denied, vanished-mid-scan, or I/O-error files. Abort the whole
   scan on the first such error, or record it and continue?
3. **Path/group storage at scale.** Millions of files means a naive
   `HashMap<u64, Vec<PathBuf>>` duplicates every path as a full owned
   `PathBuf`. Optimize (e.g. prefix-compressed path storage, as fclones
   does) from day one, or start naive and revisit only with benchmark
   evidence?

## Decision

- **Streaming API**: `rusty_fclone_core::scan()` returns a `ScanHandle` that
  implements `Iterator<Item = ScanEvent>` over an unbounded
  `crossbeam-channel`. `ScanEvent` is `DuplicateGroup(..)`, `Error(..)`, or a
  terminal `Finished(ScanSummary)`. Groups are sent as soon as
  `process_size_group` confirms them (per size-group, running in parallel via
  rayon), not batched until the whole tree finishes.
  - **Scope note**: v1's traversal phase runs to completion before any
    hashing begins (see `pipeline::run_scan`). The *contract* is streaming —
    a consumer sees the first `DuplicateGroup` well before the last — but
    full pipeline overlap (hashing starting while traversal is still walking
    unvisited subtrees) is a documented roadmap item, not part of v1.
- **Failure policy**: per-file errors (permission denied, vanished, I/O
  error) are recorded as `ScanEvent::Error(FileError)` and the scan
  continues. Nothing aborts a multi-hour scan because of one bad file.
- **Path storage**: v1 uses the naive `HashMap<u64, Vec<(PathBuf, Vec<PathBuf>)>>`
  structure throughout (`pipeline::FileGroup` and friends). Prefix-compressed
  storage is deferred until a benchmark on a real million-file tree shows
  it's the actual bottleneck, per the "don't add dependencies/complexity
  speculatively" principle.

## Consequences

- Consumers (the CLI, or any future embedder of `rusty_fclone-core`) must be
  written against a streaming, possibly-interleaved-with-errors event
  sequence, not a simple `Result<Vec<_>, _>`. `ScanHandle`'s `Drop` impl
  joins the background scan thread, so simply letting a `for event in handle`
  loop run to completion (or dropping the handle early) is always safe.
- Memory usage scales with total path bytes on very large trees. Acceptable
  for v1; flagged in the roadmap as the first thing to revisit if real-world
  scans on multi-million-file trees show memory pressure.
