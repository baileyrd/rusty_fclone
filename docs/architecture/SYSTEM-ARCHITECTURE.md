# System Architecture

## Purpose

`rusty_fclone` is a spiritual successor to [fclones](https://github.com/pkolaczk/fclones)
(not "fclone" — a naming correction made early in this project's history):
a duplicate-file finder with a detection engine and an action layer
(delete/trash/hardlink/reflink, see ADR-0009, ADR-0014, and ADR-0024) on top of it,
consumed by both a CLI and a desktop GUI (see ADR-0020). This document
covers all three.

## Product boundary

- **Users**: people/scripts wanting to find and reclaim duplicate files
  across a directory tree — the same audience as fclones, fdupes, rmlint,
  jdupes.
- **Platforms**: cross-platform from v1 (Linux, macOS, Windows) via a
  portable blocking-I/O model — see ADR-0002.
- **Non-goals for v1**: network-filesystem-specific handling. (A GUI was
  a v1 non-goal too, until ADR-0020 reversed it, and near-duplicate/fuzzy
  matching of images specifically was too, until ADR-0030's opt-in
  `find_similar_images` reversed it — both marked reversible here from
  the start; see the Crate boundaries section below. Fuzzy matching of
  anything other than images remains a non-goal.)

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

Not shown above: when `--cache` (ADR-0016) and/or `--import-fclones-cache`
(ADR-0019) are set, the full-hash box is preceded by a cache lookup keyed
on `(path, size, mtime)` — a hit reuses the stored hash and skips the real
read entirely; a miss falls through to hashing as drawn. Both are off by
default and never change what gets reported, only whether a file gets
re-read.

Deliberately not shown above at all: `find_similar_images` (ADR-0030,
`perceptual` module) is not a stage of this pipeline and never runs as
part of it — it's a wholly separate, opt-in entry point with its own
traversal, decoding real image pixel content (via the `image` crate) and
clustering by perceptual (dHash) similarity rather than exact-hash
grouping. Its output (`SimilarGroup`) never enters this diagram's
`DuplicateGroup`/`ScanEvent` flow at any point — the architectural
separation the plan behind ADR-0030 required is enforced by keeping it
structurally outside this pipeline, not just documented as a caveat on
top of it.

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

- `traversal::traverse` is the only stage that walks the filesystem tree
  structure itself. It doesn't collect a `Vec<Candidate>` — each
  `Candidate` (path, size, file-id) is handed to an `on_candidate` callback
  as soon as jwalk produces it, so `pipeline::run_scan` can fold hardlink
  collapse into that same streaming pass instead of looping over a
  materialized list afterward (ADR-0012).
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
  `plan_with_keep` generalizes this to an explicit, caller-chosen kept
  path; `select::choose_keep` picks one by a named rule (newest, oldest,
  shortest/longest path) instead of always alphabetically-first
  (`SELECTION-RULES`). A caller-supplied `reference_paths` list overrides
  both: a path under a configured reference folder always wins as kept,
  and is never placed in `actions`, regardless of `Rule` or an explicit
  `keep` argument — a hard block, not a dismissible warning
  (`ACTION-REFERENCE-FOLDERS`, ADR-0025).
- `apply`'s hardlink action never leaves a path momentarily missing: it
  links the kept file to a temporary sibling name, then renames that over
  the target path.
- `ActionKind::Move(archive_dir)`/`Copy(archive_dir)` carry their archive
  destination as data on the variant itself, not a separate parameter —
  every function taking `kind: ActionKind` already had one, so this adds
  zero new parameters to the API surface. `Move` relocates a redundant
  copy into `archive_dir` (mirroring its original path, so same-named
  files from different directories never collide) and reclaims space at
  the scanned location like `Delete`/`Trash`; `Copy` does the same but
  leaves the original untouched and reclaims nothing — a
  consolidate-for-review action, not a cleanup one (`ACTION-MOVE-COPY`,
  ADR-0026).
- The CLI is the only place a decision to actually mutate the filesystem
  gets made — `--action <kind>` alone only previews; `--apply` is required
  in addition. `rusty_fclone-core::action` itself has no concept of "dry
  run" — `plan`/`apply` are simply two separate calls, and the CLI only
  calls `apply` when both flags are present.

## Crate boundaries (ADR-0005, extended by ADR-0020)

- `rusty_fclone-core` — detection (`scan`) and action (`action::plan`/
  `action::apply`). No CLI or GUI concerns — no `serde`, no awareness that
  either consumer exists.
- `rusty_fclone-cli` — thin `clap`-based binary (`rusty-fclone`) consuming
  `rusty_fclone-core`. `main` is a two-line wrapper around a testable
  `run(cli: Cli) -> ExitCode`.
- `rusty_fclone-gui` — Tauri (v2) desktop app consuming
  `rusty_fclone-core` (ADR-0020). Rust backend (`src/commands.rs`'s
  `start_scan`/`run_action`, `src/payload.rs`'s wire DTOs) plus a plain
  HTML/CSS/JS frontend (`ui/`, no bundler). The one dependency in this
  workspace needing a C toolchain and system webview libraries at build
  time — see ADR-0020 and the C-toolchain note in `AGENTS.md`.

## Where to look next

- Decisions: `docs/decisions/` (ADR-0001 onward).
- Specs: `docs/specifications/detection/FCLONE-DETECTION-001.md`,
  `docs/specifications/action/FCLONE-ACTION-001.md`,
  `docs/specifications/cli-ux/CLI-UX-001.md`,
  `docs/specifications/gui-ux/GUI-UX-001.md`.
- What's built vs. planned: `docs/roadmap/ROADMAP.md`,
  `docs/PROJECT-STATUS.md`.
