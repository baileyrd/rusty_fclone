# ADR-0025: Protected/reference-folder guardrail

- Status: Accepted
- Date: 2026-08-27
- Related: ADR-0009 (action-layer safety model, extended here), ADR-0023
  (folder-level action, whose directory-prune step this constrains too),
  `docs/roadmap/DEDUP-GAP-IMPLEMENTATION-PLAN.md` (`ACTION-REFERENCE-
  FOLDERS`, Phase 2)

## Context

Every prior selection mechanism (`Rule` in `select.rs`, a manual keep
choice in the CLI/GUI) picks which copy of a duplicate to keep *among the
copies a scan found* — none of them can guarantee a specific folder (a
"master" photo library, a backup archive, a folder synced from another
machine) is simply never touched, no matter what rule or manual choice is
in play. The competitive research behind the gap-analysis plan found a
protected/reference-folder concept in most comparable tools and called it
out as a real safety gap here: without it, an unlucky `Rule::Oldest` pick
or a `--keep-rule` default could plan to delete the one copy the user
most needed kept, with no way to say "never this folder" up front.

The plan calls for this as "a hard block inside `plan`/`plan_folder` —
fails closed (a protected path is never placed in `actions`, not just
flagged in the UI)". Two design questions came up while implementing
that:

1. Is excluding protected paths from `actions` enough on its own?
2. Folder-level actions prune the emptied directory after every planned
   file succeeds (ADR-0023) — does protecting a file inside that
   directory also need to change *that*?

## Decision

- **`reference_paths: &[PathBuf]` is a prefix-match list, not part of
  `ScanOptions`.** It's threaded as an explicit parameter to
  `select::choose_keep`, `action::plan`/`plan_with_keep`, and
  `folder_action::plan_folder` — not folded into the scan-time options
  struct, because it governs *what an action is allowed to do*, not what
  a scan discovers. A path is protected if it starts with any configured
  reference path (`select::is_protected`); every existing caller that
  doesn't need the guardrail passes `&[]`, the same "empty means off"
  convention `ScanOptions`'s own optional filters already use.
- **The protected path always wins as "keep", not just as an exclusion.**
  `select::choose_keep` checks `protected_member(group, reference_paths)`
  before ever consulting `rule`, and returns that path (reason: "in a
  protected/reference folder") if one exists in the group — overriding
  `AlphabeticallyFirst`/`Newest`/`Oldest`/`ShortestPath`/`LongestPath`
  alike. This was deliberate, not just convenient: a design that only
  *excluded* protected paths from `actions` without also fixing `keep`
  would have a real failure mode — a two-file group with one protected
  and one unprotected copy, where the unprotected copy happens to be
  picked as `keep` (e.g. by alphabetical default), reclaims *zero* bytes,
  since the sole removable candidate (the protected one) gets filtered
  out. Making the guardrail choose `keep` first is what makes "protect
  this folder" and "still reclaim space" compatible.
- **`action::plan_with_keep` re-applies the same override independently
  of `choose_keep`**, as defense-in-depth: it overrides a caller-supplied
  `keep` to the protected path if the group contains one, and separately
  filters `select::is_protected` paths out of `actions` regardless of
  what `keep` ended up being. This means the guarantee — a protected path
  is never in `actions` — holds even for a caller that bypasses
  `choose_keep` entirely and hands `plan_with_keep` a manual `keep`
  choice (exactly how the CLI's manual keep-choice path and the GUI's
  keep-choice badge both work).
- **`folder_action::plan_folder` skips a protected candidate file before
  pairing it, and counts it** in a new
  `FolderActionPlan::protected_files_skipped: u64` rather than silently
  leaving it out of `pairs`. That count exists for one reason:
  **`apply_folder`'s directory-prune step (ADR-0023) now also requires
  `protected_files_skipped == 0`.** Without that guard, a protected file
  left inside `removed` after every *planned* pair succeeds would still
  get deleted by `fs::remove_dir_all` — the prune step has no way to
  distinguish "genuinely empty" from "empty except for the file we
  deliberately didn't touch". This was caught during implementation, not
  in the original plan text, and is load-bearing: it's the difference
  between "never touched" and "touched anyway, one directory removal
  later."

## Consequences

- `select::choose_keep`, `action::plan`, `action::plan_with_keep`, and
  `folder_action::plan_folder` all gained a trailing `reference_paths`
  parameter — every existing call site (core tests, the CLI, the GUI)
  needed updating, caught at compile time.
- `FolderActionPlan` gained `protected_files_skipped`; any caller
  constructing one directly (the GUI's payload tests did) needed
  updating too.
- CLI: a new repeatable `--reference <path>` flag, threaded into both the
  per-file and folder-level action paths.
- GUI: a new "Protected folders" field on Scan Setup (`state.
  referencePaths`, comma-separated, mirroring the existing "skip these
  folders" scan filter's shape) passed to the `run_action`, `choose_keep`,
  and `run_folder_action` IPC commands — not `start_scan` or
  `find_duplicate_folders`, since detection itself doesn't need it. The
  frontend's rule-preview lookup (`ensureRuleKeepChoice`) now also runs
  under the default "alphabetical" rule whenever a reference folder is
  configured, so the Review screen's "keeping this file" badge reflects
  the guardrail before Apply rather than only after.
- A group can contain more than one protected path (several reference
  folders each holding a copy); `plan_with_keep` filters every protected
  path other than `keep` out of `actions`, not just the first one found.
- This is a prefix match on the path as given (`Path::starts_with`), not
  a canonicalized/symlink-resolved comparison — consistent with how the
  rest of this codebase already treats configured paths (`exclude_paths`,
  `--reference`'s CLI counterpart), and documented as such rather than
  silently assumed.
