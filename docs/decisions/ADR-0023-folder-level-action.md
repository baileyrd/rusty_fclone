# ADR-0023: Folder-level action (delete/hardlink/reflink a duplicate folder)

- Status: Accepted
- Date: 2026-08-25
- Related: ADR-0009 (per-file action safety model, extended here to
  folders), ADR-0021 (folder-level *detection*, which deliberately left
  this out — "detection and reporting only... A folder-level action is a
  real, separate, more dangerous capability... out of scope here, not
  attempted"), ADR-0022 (GUI redesign, which shipped the "Delete
  Duplicate Folder" button disabled pending this decision)

## Context

ADR-0021 gave `rusty_fclone` real folder-level duplicate *detection*
(`FolderMatch::Exact`/`Contained`) but explicitly no way to *act* on a
match — the GUI's "Delete Duplicate Folder" button ships disabled,
because there was nothing behind it. Asked for directly: "Enable the
folder-level delete action for real."

A `FolderMatch` already guarantees the relationship needed to act
safely: every file under a `Contained` match's `subset` has a
byte-identical twin at the same relative path under its `superset`; two
`Exact` folders have the identical relationship in both directions. So
"delete the duplicate folder" has an unambiguous meaning — remove (or
hardlink/reflink) every file under the redundant side, each against its
specific confirmed partner — but it acts on many files at once through
directories, not the one-group-at-a-time shape `action::plan`/`apply`
were built for.

## Decision

- **New `rusty_fclone_core::folder_action` module**, parallel to
  `action`: `plan_folder(removed, kept, groups, options, kind) ->
  Result<FolderActionPlan, FolderActionError>` (pure, no filesystem
  writes) and `apply_folder(plan) -> FolderApplyReport` (the only
  function that touches the filesystem) — same plan/apply split as
  `action`, same reason (ADR-0009: a caller can always preview before
  mutating).
- **Reuses `action::apply` per file pair, not new delete/hardlink/reflink
  code.** `plan_folder` pairs every file under `removed` with its exact
  partner under `kept` (by relative path); `apply_folder` turns each pair
  into its own single-action `ActionPlan { kept: <that pair's partner>,
  actions: [FileAction { path: <that pair's file>, kind }], .. }` and
  runs it through the existing, already-tested `action::apply`. No new
  low-level filesystem-mutation code exists for this — `hardlink_over`/
  `reflink_over`/the safe temp-then-rename pattern are exercised exactly
  as they already were for single-group actions.
- **`plan_folder` re-verifies the pairing itself — it does not trust a
  `FolderMatch` computed earlier.** It re-walks `removed` (a second,
  lightweight, stat-only traversal, same as `find_folder_duplicates`'s
  own contract) and, for every file found, requires `groups` to contain
  it in the same `DuplicateGroup` as its expected partner under `kept`,
  *and* the file's current on-disk size to match that group's recorded
  size. Fails closed: one file without a live, matching confirmation
  means no plan is produced at all, not a partial one missing that file.
  This catches both "the caller passed folders that don't actually hold
  the claimed relationship" and "something on disk changed since the
  scan that produced `groups`" — a stale plan silently deleting a file
  that no longer has a real duplicate would be exactly the kind of wrong
  destructive action this project has avoided everywhere else (ADR-0001
  through ADR-0009's whole safety posture).
- **`removed`/`kept` are exactly one folder pair, not a whole
  `FolderMatch`.** `plan_folder`/`apply_folder` don't know about
  `FolderMatch::Exact`/`Contained` at all — a caller with a `Contained`
  match calls this once (`removed = subset`, `kept = superset`); a
  caller with an `Exact` match of 2+ folders picks one to keep (by
  convention, matching `action::plan`'s existing "alphabetically-first
  path is kept" rule, the shallowest/first folder is kept) and calls this
  once per remaining folder. This mirrors how `action::plan`/`apply`
  already work per-`DuplicateGroup`, with the caller looping over groups
  — the same shape, one level up.
- **Directory cleanup only follows a fully successful `Delete`.** After
  every planned file is actually removed (no failures), `apply_folder`
  prunes the now file-less `removed` directory tree with
  `fs::remove_dir_all`, reported via `FolderApplyReport::directory_removed`
  rather than as a per-file failure — the file-level actions already all
  succeeded by that point; only the empty-directory cleanup is a
  separate, best-effort step. A partial failure leaves `removed`
  untouched beyond whichever files did get deleted (ADR-0004's
  error-tolerance contract, same as everywhere else). `Hardlink`/
  `Reflink` never prune anything — every file stays in place by design,
  the folder is meant to look untouched from outside while sharing
  storage underneath.
- **CLI and GUI wiring are a separate follow-up.** This ADR lands the
  core capability, tested on its own; `--find-duplicate-folders`'s
  action-layer counterpart in `rusty_fclone-cli`, and enabling the GUI's
  disabled "Delete Duplicate Folder" button (with the same Delete/
  Hardlink/Reflink choice and confirmation-dialog safety gate the
  file-level Review screen already has, per ADR-0022), follow in their
  own change — same "capability lands before the UI that surfaces it"
  pattern this project has used throughout (ADR-0021 → its own CLI flag
  → the GUI redesign; the `find_duplicate_folders` GUI command before
  the frontend that calls it).

## Consequences

- `rusty_fclone_core`'s public surface gains `folder_action::{plan_folder,
  apply_folder, FolderActionPlan, FolderFilePair, FolderApplyReport}` and
  a new top-level error, `FolderActionError`. No existing type or
  function's signature changed — `action::plan`/`apply` are untouched,
  called by `folder_action` exactly as any other caller would.
- A folder-level action still can't do anything a sequence of per-file
  actions on the same paths couldn't already do through the existing
  Review screen/CLI — this ADR is about doing that sequence safely and
  conveniently for a whole folder at once, not a new class of mutation.
- The re-verification traversal means `plan_folder` costs a second,
  lightweight walk of `removed` beyond whatever produced `groups` — the
  same cost `find_folder_duplicates` already pays for the same reason,
  and for the same reason: honesty about current disk state beats
  trusting an earlier snapshot for a destructive operation.
- `directory_removed: false` after a fully successful `Delete` (the
  prune itself failed — e.g. another process created a file in `removed`
  between the last delete and the prune) is a real, if narrow, race
  window. It's surfaced, not hidden: the caller sees every individual
  file action's real outcome plus whether the directory itself ended up
  gone, and can decide what to do about a `removed` folder that's now
  empty of the files this plan knew about but still exists.
