# System Architecture

## Purpose

`rusty_fclone` is a spiritual successor to [fclones](https://github.com/pkolaczk/fclones)
(not "fclone" — a naming correction made early in this project's history):
a duplicate-file finder, starting from the fastest practical detection
engine and growing an action layer (delete/hardlink/reflink) on top of it
later. This document covers what's built so far: the detection engine.

## Product boundary

- **Users**: people/scripts wanting to find duplicate files across a
  directory tree — the same audience as fclones, fdupes, rmlint, jdupes.
- **Platforms**: cross-platform from v1 (Linux, macOS, Windows) via a
  portable blocking-I/O model — see ADR-0002.
- **Non-goals for v1**: an action layer (delete/link duplicates), near-
  duplicate/fuzzy matching, a GUI, network-filesystem-specific handling.

## Detection pipeline

```
root path
   │
   ▼
┌─────────────────────────────┐
│ traversal (jwalk, parallel) │  skip symlinks / stay on filesystem by default
└──────────────┬──────────────┘  (ADR-0003)
               ▼
┌─────────────────────────────┐
│ hardlink pre-dedup           │  group by (device, inode) / file-id — free,
│ (device, inode) collapse     │  stat-only (ADR-0001, ADR-0003)
└──────────────┬───────────────┘
               ▼
┌─────────────────────────────┐
│ group by exact size          │  drop singleton sizes — can't be duplicates
└──────────────┬───────────────┘
               ▼  (per size-group, in parallel via rayon)
    size ≤ threshold? ──yes──┐
               │no            │
               ▼              │
┌─────────────────────────────┐│
│ partial hash (head/mid/tail) ││  xxh3-128, via IoPool (ADR-0001)
│ → subgroup, drop singletons  ││
└──────────────┬───────────────┘│
               ▼                │
┌─────────────────────────────┐ │
│ full hash                    │◄┘  xxh3-128, via IoPool
│ → subgroup, drop singletons  │
└──────────────┬───────────────┘
               ▼
     verify_matches? ──yes──► byte-compare, drop non-matches
               │no
               ▼
     emit DuplicateGroup (streamed via ScanEvent, ADR-0004)
```

## Concurrency model (ADR-0002)

Two separate thread pools, connected by `crossbeam-channel`:

- **I/O pool** (`io_pool::IoPool`): a small, hand-rolled, oversubscribed pool
  of blocking `std::thread` workers. Handles all file reads (partial-range
  and full-file).
- **CPU pool**: rayon's global pool, capped at core count. Used for hashing
  and for parallelizing across size-groups and group members
  (`into_par_iter()`).

Directory traversal parallelizes itself via jwalk's own rayon-backed walker
— no separate pool needed for that stage.

## Data flow / ownership

- `traversal::traverse` produces `Vec<Candidate>` (path, size, file-id) —
  the only stage that walks the filesystem tree structure itself.
- `pipeline::run_scan` owns all grouping (hardlink collapse → size-group →
  hash-subgroup) using plain `HashMap`s (ADR-0004; not yet optimized for
  memory at extreme scale).
- `ScanEvent`s cross a `crossbeam-channel` from the background scan thread
  to whatever's consuming `ScanHandle` (the CLI, or a future embedder).

## Crate boundaries (ADR-0005)

- `rusty_fclone-core` — the engine described above. No CLI concerns.
- `rusty_fclone-cli` — thin `clap`-based binary (`rusty-fclone`) consuming
  `rusty_fclone-core`.

## Where to look next

- Decisions: `docs/decisions/ADR-0001` through `ADR-0006`.
- Detection engine spec: `docs/specifications/detection/FCLONE-DETECTION-001.md`.
- What's built vs. planned: `docs/roadmap/ROADMAP.md`,
  `docs/PROJECT-STATUS.md`.
