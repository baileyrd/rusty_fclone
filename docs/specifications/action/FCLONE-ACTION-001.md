# FCLONE-ACTION-001 — Duplicate Action Layer
- Version: 0.2.0
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

- Configurable keep-strategy (by mtime, by directory priority, interactive
  per-group choice) — v1 always keeps the alphabetically-first path.
- An interactive confirmation prompt — v1's safety model is the two-flag
  (`--action` + `--apply`) CLI requirement instead (ADR-0009).
- Moving files (as opposed to deleting or hardlinking) — not requested,
  not implemented.

## Context and terminology

- **Kept file**: the one path in a group left untouched —
  `DuplicateGroup.paths[0]` (already alphabetically-first per
  `FCLONE-DETECTION-001`'s sort invariant).
- **Redundant copy**: any other path in the group that isn't already a
  hardlink alias of the kept file.
- **Plan**: a pure, side-effect-free description of what an action would
  do (`action::plan`), safe to compute and print without touching the
  filesystem.
- **Apply**: actually executing a plan (`action::apply`).

## Requirements

- `FCLONE-ACTION-001-FR-001`: Given a `DuplicateGroup` and an `ActionKind`
  (`Delete`, `Hardlink`, or `Reflink`), `plan` SHALL identify the kept file
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

## Architecture and interfaces

Public API (`crates/rusty_fclone-core/src/action.rs`):

```rust
pub enum ActionKind { Delete, Hardlink, Reflink }
pub struct FileAction { pub path: PathBuf, pub kind: ActionKind }
pub struct ActionPlan { pub size: u64, pub kept: PathBuf,
                         pub actions: Vec<FileAction>, pub bytes_reclaimed: u64 }
pub struct ApplyReport { pub succeeded: Vec<PathBuf>, pub failed: Vec<FileError>,
                          pub bytes_reclaimed: u64 }

pub fn plan(group: &DuplicateGroup, kind: ActionKind) -> ActionPlan;
pub fn apply(plan: &ActionPlan) -> ApplyReport;
```

`Reflink` uses the `reflink-copy` crate's strict `reflink` function (not
`reflink_or_copy`) — see ADR-0014.

CLI (`rusty_fclone-cli`): `--action <report|delete|hardlink|reflink>`
(default `report`) and `--apply` (bool). The CLI's `Action` enum is a thin
wrapper adding `Report` — kept CLI-side rather than in core, since core
stays CLI-agnostic (ADR-0005).

## Data/state and invariants

- `ActionPlan::actions` never includes the kept file, and never includes a
  path that shares the kept file's platform file-id at plan time.
- `ApplyReport::bytes_reclaimed` reflects only *successful* actions —
  distinct from `ActionPlan::bytes_reclaimed`, which is what a plan would
  reclaim if every action succeeded. A caller comparing the two after
  `apply` can tell whether anything failed without inspecting `failed`.

## Errors, failure, recovery, and observability

- Every per-file failure surfaces as a `FileError` (the same type detection
  uses), keeping error handling consistent across both capabilities.
- No structured logging; CLI prints warnings to stderr, matching detection's
  existing convention.

## Security, privacy, and compatibility

- `Delete`, `Hardlink`, and `Reflink` are all irreversible or hard-to-reverse
  operations on the user's filesystem — this is the first genuinely
  destructive capability in the codebase. The dry-run-by-default,
  two-flag-to-apply design (ADR-0009) is the primary safeguard.
- Hardlinking requires the kept file and the target path to be on the same
  filesystem; a cross-device attempt fails per-file (FR-005) rather than
  aborting the whole run.
- Reflinking requires a CoW-capable filesystem (Btrfs, XFS with reflink
  enabled, APFS, some ZFS setups); anywhere else it fails per-file (FR-005,
  FR-008) rather than silently degrading to a full copy — a copy wouldn't
  free any space, defeating the point of choosing reflink over hardlink.

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

## Change history

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
