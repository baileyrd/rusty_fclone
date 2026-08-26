# FCLONE-ACTION-001 — Duplicate Action Layer
- Version: 0.5.0
- Status: Implemented (v1)
- Owners: baileyrd
- Depends on: `FCLONE-DETECTION-001`
- Supersedes: none

## Purpose and scope

Turn a confirmed `DuplicateGroup` (from `FCLONE-DETECTION-001`) into actual
disk-space savings: delete or hardlink redundant copies, safely, with a
mandatory dry-run-by-default posture given the feature is destructive by
nature.

## Non-goals

- Configurable keep-strategy beyond `select::Rule`'s five options (by
  mtime, by path depth, or the original alphabetically-first default) —
  reversed by `SELECTION-RULES`, the same way the GUI's own "v1 non-goal"
  was reversed by `GUI` (ADR-0020). A rule based on file size specifically
  remains out of scope, and always will be for `DuplicateGroup`s: every
  path in one already shares the exact same size by definition (this
  project's detection is hash-verified exact-content matching), so a
  size-based rule could never distinguish anything.
- An interactive confirmation prompt — v1's safety model is the two-flag
  (`--action` + `--apply`) CLI requirement instead (ADR-0009).
- Moving files (as opposed to deleting or hardlinking) — not requested,
  not implemented.
- Choosing which folder to keep in a `FolderMatch::Exact` cluster of 3+
  mutually-identical folders beyond the alphabetically-first convention
  `plan`/`FR-001` already establish for files — `folder_action::plan_folder`
  acts on exactly one `removed`/`kept` folder pair; a caller decides which
  folder to keep and calls it once per remaining folder (ADR-0023). Still
  true after `SELECTION-RULES`: `select::Rule` applies to `DuplicateGroup`s
  (files) only, not `FolderMatch`es — a folder's own timestamp is a
  weaker, less obviously meaningful signal than a single file's, and
  extending rule-based selection there would widen this unit's scope
  beyond what the research behind it actually asked for (no ADR for this
  scoping call — see the change history entry below).

## Context and terminology

- **Kept file**: the one path in a group left untouched — `plan`'s default
  is `DuplicateGroup.paths[0]` (already alphabetically-first per
  `FCLONE-DETECTION-001`'s sort invariant); `plan_with_keep` and
  `select::choose_keep` let a caller choose a different one by rule
  (`SELECTION-RULES`).
- **Redundant copy**: any other path in the group that isn't already a
  hardlink alias of the kept file.
- **Plan**: a pure, side-effect-free description of what an action would
  do (`action::plan`), safe to compute and print without touching the
  filesystem.
- **Apply**: actually executing a plan (`action::apply`).
- **Folder action**: the same plan/apply split, one level up — acting on
  every file inside one folder ("removed") against its confirmed partner
  file in another ("kept"), for a `FolderMatch` (`FCLONE-DETECTION-001`
  FR-010) rather than one `DuplicateGroup` (`folder_action::plan_folder`/
  `apply_folder`, ADR-0023).

## Requirements

- `FCLONE-ACTION-001-FR-001`: Given a `DuplicateGroup` and an `ActionKind`
  (`Delete`, `Trash`, `Hardlink`, or `Reflink`), `plan` SHALL identify the kept file
  as `paths[0]` and SHALL include every other path not already sharing the
  kept file's platform file-id as a planned action.
- `FCLONE-ACTION-001-FR-002`: `plan` SHALL NOT perform any filesystem
  mutation — it only reads file identity to determine hardlink aliases.
- `FCLONE-ACTION-001-FR-003`: `apply` with `ActionKind::Delete` SHALL
  remove every planned path and SHALL leave the kept file untouched.
- `FCLONE-ACTION-001-FR-004`: `apply` with `ActionKind::Hardlink` SHALL
  replace every planned path with a hardlink to the kept file via a
  link-to-temporary-name-then-rename sequence, such that the target path
  is never observably missing partway through the operation.
- `FCLONE-ACTION-001-FR-005`: A per-file failure during `apply` (permission
  denied, vanished, cross-device link) SHALL be recorded in
  `ApplyReport::failed` and SHALL NOT prevent the remaining planned actions
  from being attempted.
- `FCLONE-ACTION-001-FR-008`: `apply` with `ActionKind::Reflink` SHALL
  replace every planned path with a copy-on-write clone of the kept file
  via the same temp-name-then-rename sequence as FR-004, and SHALL record
  a per-file failure (FR-005) rather than silently falling back to a plain
  copy when the underlying filesystem doesn't support cloning.
- `FCLONE-ACTION-001-FR-006`: The CLI SHALL NOT mutate the filesystem when
  `--action` is `delete` or `hardlink` unless `--apply` is also passed;
  without `--apply` it SHALL print the same plan information (kept path,
  paths that would be acted on, bytes that would be reclaimed) that a real
  run would report.
- `FCLONE-ACTION-001-FR-007`: The CLI's default behavior (`--action`
  omitted, equivalently `--action report`) SHALL be identical to the
  detection-only CLI behavior that existed before this capability was
  added.
- `FCLONE-ACTION-001-FR-009`: Given a `removed` folder, a `kept` folder,
  the `DuplicateGroup`s a scan produced, and an `ActionKind`,
  `folder_action::plan_folder` SHALL re-derive, for every file currently
  under `removed`, whether it has a confirmed partner at the matching
  relative path under `kept` — present in the same `DuplicateGroup` and
  matching that group's recorded size. A nonexistent/non-directory
  `removed`, or any file lacking a confirmed live partner, SHALL cause
  `plan_folder` to return an error instead of a plan — never a plan
  silently missing that file.
- `FCLONE-ACTION-001-FR-010`: `folder_action::apply_folder` SHALL execute
  `kind` for every planned file pair by constructing a single-action
  `action::ActionPlan` per pair and running it through the existing
  `action::apply` (FR-001 through FR-005, FR-008) — no separate
  delete/hardlink/reflink implementation. A per-file failure SHALL be
  recorded in `FolderApplyReport::failed` and SHALL NOT prevent the
  remaining planned pairs from being attempted.
- `FCLONE-ACTION-001-FR-011`: After an `ActionKind::Delete`/`ActionKind::Trash`
  folder action completes with zero per-file failures, `apply_folder`
  SHALL prune the now file-less `removed` directory tree and record the
  outcome in `FolderApplyReport::directory_removed`. `ActionKind::Hardlink`/
  `Reflink` SHALL NOT attempt any directory removal — every file stays in
  place under `removed`.
- `FCLONE-ACTION-001-FR-012`: `apply` with `ActionKind::Trash` SHALL move
  every planned path to the operating system's trash/recycle bin (via the
  `trash` crate) rather than removing it permanently, and SHALL leave the
  kept file untouched. A trash-provider failure SHALL be recorded as a
  per-file failure (FR-005), converted to an `io::Error` via
  `io::Error::other` (ADR-0024).
- `FCLONE-ACTION-001-FR-013`: `plan_with_keep(group, keep, kind)` SHALL
  behave identically to `plan(group, kind)` except that the kept path
  SHALL be `keep` instead of always `group.paths[0]`; `plan(group, kind)`
  SHALL be defined as `plan_with_keep(group, &group.paths[0], kind)`, so
  every existing caller's behavior is unchanged (`SELECTION-RULES`).
- `FCLONE-ACTION-001-FR-014`: `select::choose_keep(group, rule)` SHALL
  choose one path from `group.paths` under `rule` (`AlphabeticallyFirst`,
  `Newest`, `Oldest`, `ShortestPath`, `LongestPath`) and SHALL return it
  together with a one-line, human-readable reason for the choice.
  `Rule::AlphabeticallyFirst` SHALL always return `group.paths[0]`. Every
  other rule SHALL break a tie — including when a path's metadata can't be
  read at all — toward the earliest path in `group.paths`' existing sorted
  order, so it degrades to `AlphabeticallyFirst`'s exact choice whenever it
  can't actually distinguish two paths.

## Architecture and interfaces

Public API (`crates/rusty_fclone-core/src/action.rs`):

```rust
pub enum ActionKind { Delete, Trash, Hardlink, Reflink }
pub struct FileAction { pub path: PathBuf, pub kind: ActionKind }
pub struct ActionPlan { pub size: u64, pub kept: PathBuf,
                         pub actions: Vec<FileAction>, pub bytes_reclaimed: u64 }
pub struct ApplyReport { pub succeeded: Vec<PathBuf>, pub failed: Vec<FileError>,
                          pub bytes_reclaimed: u64 }

pub fn plan(group: &DuplicateGroup, kind: ActionKind) -> ActionPlan;
pub fn plan_with_keep(group: &DuplicateGroup, keep: &Path, kind: ActionKind) -> ActionPlan;
pub fn apply(plan: &ActionPlan) -> ApplyReport;
```

`Reflink` uses the `reflink-copy` crate's strict `reflink` function (not
`reflink_or_copy`) — see ADR-0014. `Trash` uses the `trash` crate's
`trash::delete` — see ADR-0024.

Folder-level public API (`crates/rusty_fclone-core/src/folder_action.rs`,
ADR-0023), reusing `action::apply` internally rather than duplicating it:

```rust
pub struct FolderFilePair { pub remove: PathBuf, pub keep: PathBuf, pub size: u64 }
pub struct FolderActionPlan { pub kind: ActionKind, pub kept: PathBuf, pub removed: PathBuf,
                               pub pairs: Vec<FolderFilePair>, pub bytes_reclaimed: u64 }
pub struct FolderApplyReport { pub succeeded: Vec<PathBuf>, pub failed: Vec<FileError>,
                                pub bytes_reclaimed: u64, pub directory_removed: bool }

pub fn plan_folder(removed: &Path, kept: &Path, groups: &[DuplicateGroup],
                    options: &ScanOptions, kind: ActionKind) -> Result<FolderActionPlan, FolderActionError>;
pub fn apply_folder(plan: &FolderActionPlan) -> FolderApplyReport;
```

Selection public API (`crates/rusty_fclone-core/src/select.rs`,
`SELECTION-RULES`):

```rust
pub enum Rule { AlphabeticallyFirst, Newest, Oldest, ShortestPath, LongestPath }
pub fn choose_keep(group: &DuplicateGroup, rule: Rule) -> (Arc<Path>, String);
```

CLI (`rusty_fclone-cli`): `--action <report|delete|trash|hardlink|reflink>`
(default `report`), `--keep-rule <alphabetical|newest|oldest|shortest-path|
longest-path>` (default `alphabetical`), and `--apply` (bool). The CLI's
`Action` enum is a thin wrapper adding `Report` — kept CLI-side rather than
in core, since core stays CLI-agnostic (ADR-0005).

## Data/state and invariants

- `ActionPlan::actions` never includes the kept file, and never includes a
  path that shares the kept file's platform file-id at plan time.
- `ApplyReport::bytes_reclaimed` reflects only *successful* actions —
  distinct from `ActionPlan::bytes_reclaimed`, which is what a plan would
  reclaim if every action succeeded. A caller comparing the two after
  `apply` can tell whether anything failed without inspecting `failed`.
- `FolderActionPlan::pairs` uses the *current* on-disk size for each file
  (re-derived by `plan_folder`'s own traversal), not whatever size the
  originating `DuplicateGroup` recorded — the two are required to match
  as part of confirming the pairing (FR-009), but `bytes_reclaimed` and
  each pair's `size` reflect what's on disk right now.

## Errors, failure, recovery, and observability

- Every per-file failure surfaces as a `FileError` (the same type detection
  uses), keeping error handling consistent across both capabilities.
- No structured logging; CLI prints warnings to stderr, matching detection's
  existing convention.

## Security, privacy, and compatibility

- `Delete`, `Hardlink`, and `Reflink` are all irreversible or hard-to-reverse
  operations on the user's filesystem — this is the first genuinely
  destructive capability in the codebase. The dry-run-by-default,
  two-flag-to-apply design (ADR-0009) is the primary safeguard. `Trash`
  (ADR-0024) is the one recoverable exception — the redundant copy is
  moved to the OS trash/recycle bin, not removed outright — but is not
  itself a substitute for the dry-run/`--apply` safeguard, which still
  gates it the same as every other `ActionKind`.
- Hardlinking requires the kept file and the target path to be on the same
  filesystem; a cross-device attempt fails per-file (FR-005) rather than
  aborting the whole run.
- Reflinking requires a CoW-capable filesystem (Btrfs, XFS with reflink
  enabled, APFS, some ZFS setups); anywhere else it fails per-file (FR-005,
  FR-008) rather than silently degrading to a full copy — a copy wouldn't
  free any space, defeating the point of choosing reflink over hardlink.
- A folder action is a strictly larger blast radius than a single-group
  action — every file under `removed` in one call, plus (on a successful
  `Delete`) the directory tree itself. `plan_folder`'s fail-closed
  re-verification (FR-009) is the primary safeguard against acting on a
  folder whose contents changed, or were never actually confirmed
  duplicates, since whatever computed the `FolderMatch` this plan is for.

## Acceptance criteria

- All functional requirements above are exercised by a dedicated test:
  FR-001 through FR-005 and FR-008 in `crates/rusty_fclone-core/src/action.rs`;
  FR-006/FR-007 (CLI-level dry-run/apply/default gating) in
  `crates/rusty_fclone-cli/src/main.rs`.
- Manual CLI smoke tests additionally confirmed real stdout/stderr output
  shape (preview lines, reclaimed-bytes summary) for `delete`, `hardlink`,
  and `reflink`, in dry-run and `--apply` modes. The `reflink` smoke test
  ran on this environment's non-CoW filesystem and confirmed the clean
  per-file-failure path (FR-008): a reported warning, zero bytes
  reclaimed, both files left with correct, unmodified content, no stray
  temp file.
- FR-009 through FR-011 (folder actions) are exercised by
  `folder_action::tests::*` (7 tests, `crates/rusty_fclone-core`):
  pairing every file with its confirmed kept-side partner; rejecting a
  file with no confirmed partner in `groups`; rejecting a file whose
  on-disk size no longer matches its recorded group size (a stale-scan
  guard); rejecting a nonexistent `removed` folder; `Delete` removing
  every file and pruning the now-empty folder; `Hardlink` replacing
  files in place without touching the folder; a per-file failure
  (the file vanishes between plan and apply) reported without pruning
  the directory. No CLI/GUI wiring exists yet — see Open questions.

## Verification plan

Unit tests in `action::tests` (core crate) cover: planning every non-kept
path, skipping existing hardlink aliases of the kept file, a plan whose
only non-kept paths are aliases (empty action list), `apply`'s delete and
hardlink paths (including that hardlinked files verifiably share an inode
afterward), per-file failure tolerance, and `apply`'s reflink path
(tolerant of both possible outcomes — CoW support present or absent —
since that depends on the filesystem running the test; see FR-008).

Unit tests in `main::tests` (CLI crate) cover: default (`Action::Report`)
never mutates the filesystem; `--action <kind>` without `--apply` is a true
dry run (nothing on disk changes); `--action <kind> --apply` actually
performs delete and hardlink; a nonexistent root exits non-zero. These
required extracting a testable `run(cli: Cli) -> ExitCode` from `main` —
the CLI crate had no test suite before this.

No benchmark exists for this capability yet — not requested, and its cost
is dominated by I/O syscalls already covered by the detection engine's
benchmarks.

Unit tests in `folder_action::tests` (core crate) cover `plan_folder`'s
pairing and fail-closed rejections (missing partner, stale size,
nonexistent `removed`) and `apply_folder`'s three outcomes (successful
delete with directory pruning, successful hardlink without pruning, a
per-file failure without pruning).

## Traceability

See `docs/traceability/TRACEABILITY.md`.

## Open questions

- No test exercises a genuine cross-device hardlink failure (would need a
  second mounted filesystem in the test environment); the per-file error
  path is tested via a vanished-file scenario instead, which exercises the
  same `ApplyReport::failed` mechanism.
- Whether a confirmation prompt should exist in addition to `--apply` is
  left to `CLI-UX` on the roadmap — not required for this unit's safety
  model to be sound.
- `folder_action::plan_folder`/`apply_folder` have no CLI or GUI caller
  yet — this version adds the core capability, tested on its own;
  `--find-duplicate-folders`'s action-layer counterpart and the GUI's
  disabled "Delete Duplicate Folder" button are a follow-up (ADR-0023).
- No test exercises `apply_folder`'s directory-prune race (something
  else creates a file under `removed` between the last successful delete
  and `fs::remove_dir_all`) — narrow enough that reproducing it
  deterministically would need real concurrency, not attempted here; the
  `directory_removed: false` outcome it would produce is still a defined,
  handled result, just not exercised by a test.

## Change history

- 0.5.0 (2026-08-26): Reversed the "configurable keep-strategy" v1
  non-goal (`SELECTION-RULES`, third and final Phase 1 unit of
  `docs/roadmap/DEDUP-GAP-IMPLEMENTATION-PLAN.md`). New `select` module
  (`FR-014`) — `Rule::{AlphabeticallyFirst, Newest, Oldest, ShortestPath,
  LongestPath}` plus `choose_keep`, returning a one-line reason alongside
  the chosen path (the playbook's cheap "why this one" explainability
  win). `plan` refactored into a thin wrapper over new `plan_with_keep`
  (`FR-013`), which takes an explicit kept path instead of always
  `group.paths[0]` — `DuplicateGroup.paths`' own sorted order and every
  existing caller's behavior are both unchanged. Deliberately does not
  cover folder-level `FolderMatch::Exact` selection or a size-based rule
  (every path in a `DuplicateGroup` shares the same size by definition) —
  see the updated Non-goals above. CLI gained `--keep-rule`; GUI's
  previously-fake "Keep newest copy" toggle is now real (new `choose_keep`
  Tauri command), applied live to every group in Review. No ADR — routine
  implementation, no architecture-level decision.
- 0.4.0 (2026-08-26): Added `ActionKind::Trash` (FR-012) — moves a
  redundant copy to the OS trash/recycle bin via the `trash` crate instead
  of deleting it permanently, closing the largest table-stakes safety gap
  found in `docs/roadmap/DEDUP-GAP-IMPLEMENTATION-PLAN.md` (`ACTION-TRASH`,
  Phase 1). `folder_action::apply_folder`'s directory-prune gate (FR-011)
  now also fires for `Trash`, matching `Delete`'s post-condition (every
  file gone from `removed`). CLI gained `--action trash`; GUI's action
  selector gained a "Trash" option and now defaults to it instead of
  `Delete`, with permanent delete kept as an explicit choice. ADR-0024.
- 0.3.0 (2026-08-25): Added folder-level actions (FR-009 through FR-011,
  `folder_action` module) — `plan_folder`/`apply_folder` act on every
  file in one folder against its confirmed partner in another, reusing
  `action::apply` per file pair rather than a new delete/hardlink/reflink
  implementation. Fails closed: `plan_folder` re-verifies every file's
  partner and size against the supplied `DuplicateGroup`s before
  producing a plan, refusing rather than risking a stale or mismatched
  pairing. `Delete` prunes the emptied folder on full success; `Hardlink`/
  `Reflink` never do. No existing `action` type or function changed.
  ADR-0023 — asked for directly ("Enable the folder-level delete action
  for real") after `FCLONE-DETECTION-001`'s folder-level *detection*
  (ADR-0021) and the GUI redesign (ADR-0022) shipped with the
  corresponding button disabled pending this decision. CLI/GUI wiring is
  a follow-up (see Open questions).
- 0.2.0 (2026-08-24): Added `ActionKind::Reflink` (FR-008,
  `ACTION-REFLINK`) — copy-on-write clone as a third action kind, using
  the `reflink-copy` crate's strict `reflink` (not `reflink_or_copy`) so
  an unsupported filesystem fails per-file rather than silently
  degrading to a full copy. Same temp-name-then-rename safety pattern as
  `Hardlink` (FR-004), with cleanup of the temp file on a failed clone.
  No change to FR-001 through FR-007's behavior. ADR-0014.
- 0.1.1 (2026-08-24): Added dedicated unit tests for FR-006/FR-007
  (CLI-level dry-run/apply/default gating), which had only been manually
  smoke-tested. Required extracting `rusty_fclone-cli::run` from `main` —
  no behavior change, `main` is now a two-line wrapper around it.
- 0.1.0 (2026-08-24): Initial implementation and specification.
