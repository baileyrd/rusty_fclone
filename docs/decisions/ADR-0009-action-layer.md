# ADR-0009: Action layer — delete/hardlink redundant copies

- Status: Accepted
- Date: 2026-08-24
- Related: `FCLONE-ACTION-001`, ADR-0001 (hardlink pre-dedup), ADR-0005
  (crate structure)

## Context

Detection alone (`scan` → `DuplicateGroup`) only reports duplicates; a
"duplicate file finder" that never lets you reclaim space is half a tool
(the README has framed the action layer as the next stage since the
project's first commit). Turning a confirmed group into disk-space savings
raises several decisions with real consequences if gotten wrong — this is
the first genuinely destructive capability in the codebase.

## Decision

- **Where it lives**: a new `action` module in `rusty_fclone-core` (not a
  new crate) — it's still core engine functionality operating on
  `DuplicateGroup`, just a different capability. No CLI concerns (argument
  parsing, confirmation prompts) leak into it, consistent with ADR-0005.
- **Actions supported in v1**: `Delete` and `Hardlink` only. `Reflink`
  (copy-on-write clone) is deliberately deferred — it's platform/filesystem
  -specific (Btrfs/XFS/APFS, via an ioctl on Linux) and would need a new
  dependency or unsafe FFI, which doesn't fit today's cross-platform-first
  scope (ADR-0002) without more design work than this unit warrants. Tracked
  as `ACTION-REFLINK` on the roadmap.
- **Which file survives**: `group.paths[0]` — the alphabetically-first path,
  which is already how `DuplicateGroup` orders its paths (ADR-0004's sort
  invariant). No configurable keep-strategy (by mtime, by directory
  priority) in v1; deterministic and simple beats configurable for a first
  cut of a destructive feature. Tracked as a roadmap follow-up if requested.
- **Hardlink aliases of the kept file are skipped**, not acted on — they
  already share its inode, so deleting or "hardlinking" them again reclaims
  nothing and would just be wasted I/O (or, for delete, actively wrong: it
  would remove a path pointing at the *kept* file's data). Determined by
  re-checking each path's platform file-id (`file-id` crate, already a
  dependency) at plan time — not by trusting the detection engine's earlier
  hardlink-collapse bookkeeping, since a plan is meant to be a fresh,
  correct read of current disk state even if it's created a while after the
  scan that produced the group.
- **Hardlink implementation is a safe rename, not delete-then-link**: link
  the kept file to a temporary sibling name, then `rename` that temp file
  over the target path. This means the target path is never momentarily
  missing if the process is interrupted mid-operation. The alternative
  (remove the target, then hardlink) leaves nothing at that path if the
  link step fails for any reason (e.g. cross-device). Same pattern fclones
  and rmlint use.
- **Default is dry-run; two flags required to actually act**: the CLI's
  `--action <report|delete|hardlink>` defaults to `report` (identical to
  pre-action-layer behavior — this is a purely additive CLI change). Passing
  `--action delete` or `--action hardlink` *alone* only prints a preview
  (paths, bytes that would be reclaimed) and touches nothing; the separate
  `--apply` flag is required to actually execute. A single mistyped or
  missing flag can never cause data loss — both `--action <kind>` and
  `--apply` must be present and correct.
- **Per-file failures don't abort the plan**: consistent with the
  detection engine's error-tolerance contract (ADR-0004/FR-009). A file
  that vanished, lost permissions, or lives on a different filesystem
  (hardlink fails cross-device) is reported and skipped; the rest of the
  plan still runs.

## Consequences

- The action layer never needs its own duplicate-detection logic or state —
  it's a pure function of a `DuplicateGroup` plus a re-check of current
  disk state, which keeps it simple and easy to reason about independent of
  the (much more complex) detection pipeline.
- `ActionPlan`/`ApplyReport` are new public types; this is the first
  addition to `rusty_fclone-core`'s public surface since the original
  `scan`/`ScanEvent` API (ADR-0004) — additive only, no existing type
  changed.
- No confirmation prompt (e.g. "are you sure? [y/N]") exists at the CLI
  layer; the two-flag (`--action` + `--apply`) requirement is considered
  sufficient friction for v1. An interactive prompt is a reasonable future
  addition (`CLI-UX` on the roadmap) but isn't required for the feature to
  be safe to ship.
- Reflink support, configurable keep-strategy, and a confirmation prompt
  are the three concrete things this ADR explicitly deferred rather than
  building speculatively.
