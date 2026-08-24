# System Architecture

## Purpose

`rusty_fclone` is a spiritual successor to [fclones](https://github.com/pkolaczk/fclones)
(not "fclone" — a naming correction made early in this project's history):
a duplicate-file finder with a detection engine and an action layer
(delete/hardlink; reflink deferred, see ADR-0009) on top of it. This
document covers both.

## Product boundary

- **Users**: people/scripts wanting to find and reclaim duplicate files
  across a directory tree — the same audience as fclones, fdupes, rmlint,
  jdupes.
- **Platforms**: cross-platform from v1 (Linux, macOS, Windows) via a
  portable blocking-I/O model — see ADR-0002.
- **Non-goals for v1**: reflink support, near-duplicate/fuzzy matching, a
  GUI, network-filesystem-specific handling.

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

## Action layer (ADR-0009)

Independent of the detection pipeline — a pure function of an already-
confirmed `DuplicateGroup` plus a fresh read of current disk state, not
part of `scan()`'s streaming loop:

```
DuplicateGroup ──action::plan(kind)──► ActionPlan (no filesystem mutation)
                                            │
                                            ▼ action::apply(&plan)
                                       ApplyReport (succeeded / failed / bytes_reclaimed)
```

- `plan` keeps `paths[0]` (already alphabetically-first) and, for every
  other path, re-checks its platform file-id against the kept file's —
  paths that already share its inode (existing hardlink aliases) are
  excluded, since there's nothing to reclaim by acting on them.
- `apply`'s hardlink action never leaves a path momentarily missing: it
  links the kept file to a temporary sibling name, then renames that over
  the target path.
- The CLI is the only place a decision to actually mutate the filesystem
  gets made — `--action <kind>` alone only previews; `--apply` is required
  in addition. `rusty_fclone-core::action` itself has no concept of "dry
  run" — `plan`/`apply` are simply two separate calls, and the CLI only
  calls `apply` when both flags are present.

## Crate boundaries (ADR-0005)

- `rusty_fclone-core` — detection (`scan`) and action (`action::plan`/
  `action::apply`). No CLI concerns.
- `rusty_fclone-cli` — thin `clap`-based binary (`rusty-fclone`) consuming
  `rusty_fclone-core`. `main` is a two-line wrapper around a testable
  `run(cli: Cli) -> ExitCode`.

## Where to look next

- Decisions: `docs/decisions/ADR-0001` through `ADR-0009`.
- Specs: `docs/specifications/detection/FCLONE-DETECTION-001.md`,
  `docs/specifications/action/FCLONE-ACTION-001.md`.
- What's built vs. planned: `docs/roadmap/ROADMAP.md`,
  `docs/PROJECT-STATUS.md`.
