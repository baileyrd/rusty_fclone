# CLI-UX-001 — CLI Output, Progress, and Confirmation
- Version: 0.3.5
- Status: Implemented (v1)
- Owners: baileyrd
- Depends on: `FCLONE-DETECTION-001`, `FCLONE-ACTION-001`
- Supersedes: none

## Purpose and scope

Everything about how `rusty_fclone-cli` presents scan results and gates
destructive actions, beyond what `FCLONE-DETECTION-001`/`FCLONE-ACTION-001`
already specify at the engine level: output format (text vs. machine-
readable JSON), progress visibility on long scans, and an interactive
confirmation prompt as a second safety layer on top of `--apply`.

## Non-goals

- Any change to detection or action semantics — this is presentation and
  a CLI-level safety gate only (ADR-0005: core stays CLI-agnostic).
- A live progress *percentage* or ETA — the engine doesn't know the total
  file count until traversal finishes, so progress is a running counter,
  not a fraction.
- A GUI or TUI — out of *this spec's* scope, which is the plain
  terminal/pipe-friendly CLI only. A GUI now exists as a separate
  consumer of `rusty_fclone-core` with its own spec (`GUI-UX-001`,
  ADR-0020); this document's requirements are unaffected by it.
- Configurable JSON schema versioning/stability guarantees — the schema
  below is what v1 emits; not yet promised stable across releases.
- Querying or reporting against `--history`'s recorded data — this unit
  only makes the data exist (FR-010); reading it back is a future unit,
  or a user pointing `sqlite3`/anything else at the file directly.
- Per-file or per-duplicate-group history detail — `--history` records
  one row per *scan* (a summary), deliberately not one row per file or
  group (ADR-0017).

## Context and terminology

- **NDJSON**: newline-delimited JSON — one JSON object per line, no
  wrapping array. Chosen so `--format json` output can be consumed
  incrementally (matching the engine's streaming design) rather than
  requiring the whole scan to finish before any of it parses.
- **Progress checkpoint**: a `ScanEvent::Progress` event, emitted every
  256 files scanned during traversal.
- **Scan-history record**: one row in `--history`'s `scans` table,
  summarizing one completed scan (ADR-0017).

## Requirements

- `CLI-UX-001-FR-001`: The CLI SHALL support `--format text` (default,
  today's human-readable output, unchanged) and `--format json` (one
  NDJSON object per line on stdout).
- `CLI-UX-001-FR-002`: In `--format json`, a duplicate group SHALL be
  emitted as `{"type":"duplicate_group","size":<u64>,"paths":[<string>,...],"action":<action-or-null>}`.
  When an action was requested (`--action` other than `report`) and the
  group has at least one non-kept path, `action` SHALL be
  `{"kind":<string>,"kept":<string>,"keep_reason":<string>,"applied":<bool>,"planned":[<string>,...],"succeeded":[<string>,...],"failed":[<string>,...],"bytes_reclaimed":<u64>}`;
  otherwise `null`. `keep_reason` is a one-line, human-readable explanation
  of why `kept` was chosen (`SELECTION-RULES`, `FCLONE-ACTION-001` FR-014).
- `CLI-UX-001-FR-003`: In `--format json`, a per-file error SHALL be
  emitted as `{"type":"error","path":<string>,"message":<string>}`.
- `CLI-UX-001-FR-004`: In `--format json`, a progress checkpoint SHALL be
  emitted as `{"type":"progress","files_scanned":<u64>,"bytes_scanned":<u64>}`.
- `CLI-UX-001-FR-005`: In `--format json`, the scan's completion SHALL be
  emitted as `{"type":"finished","files_scanned":<u64>,"bytes_scanned":<u64>,"duplicate_groups":<u64>,"duplicate_files":<u64>}`,
  followed, if an action was requested, by
  `{"type":"action_summary","kind":<string>,"applied":<bool>,"bytes_reclaimed":<u64>,"files":<u64>}`.
- `CLI-UX-001-FR-006`: `rusty_fclone_core::ScanEvent` SHALL gain a
  `Progress(ScanProgress)` variant, emitted during traversal at a fixed
  file-count interval, always before `Finished`.
- `CLI-UX-001-FR-007`: In `--format text`, when stderr is a real terminal
  (`std::io::IsTerminal`), progress checkpoints SHALL render as a single,
  in-place-updating line, cleared before any other output is printed.
  When stderr is not a terminal (piped, redirected), no progress output
  SHALL be printed.
- `CLI-UX-001-FR-008`: When `--action` is not `report` and `--apply` is
  passed without `--yes`, the CLI SHALL prompt on stderr and read a
  yes/no answer from stdin before scanning begins; only "y"/"yes"
  (case-insensitive) SHALL proceed. Any other answer, or a read failure
  (e.g. EOF), SHALL abort without scanning or mutating anything, exiting
  `ExitCode::SUCCESS` (declining is not a failure).
- `CLI-UX-001-FR-009`: `--yes`/`-y` SHALL bypass FR-008's prompt entirely.
- `CLI-UX-001-FR-010`: When `--history <path>` is set, the CLI SHALL
  append exactly one row to a `scans` table (creating the SQLite database
  and table if they don't exist) after each completed scan, recording the
  scan root, start time, `ScanSummary`'s counters, and — if `--action` was
  something other than `report` — that action's kind, whether `--apply`
  was passed, bytes reclaimed, and files acted on (ADR-0017).
- `CLI-UX-001-FR-011`: A failure to open or write the `--history` database
  SHALL be reported as a warning and SHALL NOT change the scan's own exit
  code.
- `CLI-UX-001-FR-012`: When `--find-duplicate-folders` is set, the CLI
  SHALL run `rusty_fclone_core::find_folder_duplicates` after the scan
  completes, using every `DuplicateGroup` the scan produced, and report
  each resulting `FolderMatch`. In `--format text`, an `Exact` match SHALL
  print as `--- duplicate folders: <bytes> bytes, <file_count> files ---`
  followed by one line per folder path; a `Contained` match SHALL print as
  `--- folder contained in another: <bytes> bytes, <file_count> files ---`
  followed by `subset:`/`superset:` lines. In `--format json`, they SHALL
  be emitted as
  `{"type":"folder_exact","folders":[<string>,...],"file_count":<u64>,"bytes":<u64>}`
  and
  `{"type":"folder_contained","subset":<string>,"superset":<string>,"file_count":<u64>,"bytes":<u64>}`
  respectively. Off by default (ADR-0021) — collecting every group in
  memory and re-traversing the tree is extra work most scans don't need.
- `CLI-UX-001-FR-013`: When `--find-duplicate-folders` is combined with
  `--action <kind>`, the CLI SHALL plan (and, with `--apply`, apply)
  `kind` for every `FolderMatch` via `rusty_fclone_core::folder_action`
  (ADR-0023) — a `Contained` match's `subset` against its `superset`; an
  `Exact` cluster keeping its alphabetically-first folder and acting on
  every other one against it. To avoid a folder match's defining
  evidence being consumed by a live per-group action before folder-dedup
  ever sees it, the CLI SHALL defer both reporting and any `--action` for
  every `DuplicateGroup` until after the folder-dedup pass completes
  whenever `--find-duplicate-folders` is set (unchanged, immediate
  per-group reporting/action when it is not). A `DuplicateGroup` every
  one of whose paths lies under a reported folder match's folders SHALL
  NOT be reported or acted on again individually — its folder-level
  report already covers it; every other group SHALL still be reported
  and, if requested, acted on exactly as when `--find-duplicate-folders`
  is unset. In `--format text`, each acted-on folder pair SHALL print a
  `keep folder: <path>` line and a
  `<verb> folder: <path> (<N> files, <B> bytes)` line, annotated with
  `-- folder removed` when `directory_removed` was true and/or
  `-- <n> file(s) failed` when any per-file action failed, or
  `(preview -- pass --apply to actually do this)` without `--apply`. In
  `--format json`, the enclosing `folder_exact`/`folder_contained` event
  SHALL gain an `action` array (one entry per acted-on pair, empty when
  no `--action` was requested) of
  `{"kind":<string>,"kept":<string>,"removed":<string>,"applied":<bool>,"file_count":<u64>,"bytes":<u64>,"directory_removed":<bool>,"failed":<u64>}`.
- `CLI-UX-001-FR-014`: The CLI SHALL expose `FCLONE-DETECTION-001`'s
  scan-filter fields as flags: `--min-size <BYTES>`, `--max-size <BYTES>`,
  `--include-ext <EXT>` (repeatable), `--exclude-ext <EXT>` (repeatable),
  and `--exclude-path <PATH>` (repeatable), mapped directly onto
  `ScanOptions::min_size`/`max_size`/`include_extensions`/
  `exclude_extensions`/`exclude_paths`. An empty `--include-ext`/
  `--exclude-ext` list (the default, flag never passed) SHALL map to
  `None`, matching `ScanOptions`'s own "no filtering" default
  (`DETECTION-SCAN-FILTERS`).
- `CLI-UX-001-FR-015`: The CLI SHALL expose `FCLONE-ACTION-001`'s
  `select::Rule` as `--keep-rule <alphabetical|newest|oldest|
  shortest-path|longest-path>`, default `alphabetical`, applied via
  `action::plan_with_keep`/`select::choose_keep` in place of
  `action::plan` for every group when `--action` is set. `--keep-rule`
  SHALL have no effect in the default `report` mode, which does not
  designate a kept file at all (`SELECTION-RULES`).
- `CLI-UX-001-FR-016`: The CLI SHALL expose `FCLONE-ACTION-001`'s
  reference-folder guardrail as a repeatable `--reference <PATH>` flag,
  default none, passed to `select::choose_keep`, `action::plan_with_keep`,
  and (when `--find-duplicate-folders` is also set) `folder_action::
  plan_folder` for every group/folder pair acted on. `--reference` SHALL
  have no effect in the default `report` mode, and an empty
  `--reference` list SHALL be identical to no guardrail
  (`ACTION-REFERENCE-FOLDERS`).
- `CLI-UX-001-FR-017`: The CLI SHALL expose `FCLONE-ACTION-001`'s
  `ActionKind::Move`/`Copy` as `--action move`/`--action copy`, and
  SHALL expose their archive destination as `--archive-dir <PATH>`.
  `--action move`/`copy` without `--archive-dir` SHALL be rejected with
  an error naming the specific action that needs it, before any scan
  runs. `--archive-dir` SHALL have no effect with any other `--action`
  value (`ACTION-MOVE-COPY`).
- `CLI-UX-001-FR-018`: When `--history <path>` is set and `--apply` runs
  a real action, the CLI SHALL record one row per individual file/pair
  actually acted on (path, action kind, bytes, success/failure, and the
  error text on failure), correlated with its real per-file outcome —
  never for a preview (`--action` without `--apply`), which plans but
  runs nothing. The CLI SHALL additionally expose a separate `rusty-
  fclone history <list|stats>` command reading an existing `--history`
  database: `list [--limit N]` (default 20) prints the most recent scans
  newest first; `stats [--since TS] [--until TS]` (Unix timestamps,
  either bound optional) prints aggregate totals (scan count, files/bytes
  scanned, duplicate groups/files, bytes reclaimed, files acted on) across
  scans in that range. Both SHALL support `--format text|json`, matching
  the main command's own convention. `history` SHALL be treated as a
  reserved top-level keyword: `rusty-fclone history ...` never triggers a
  scan, and `rusty-fclone <ROOT> ...` for any other first argument SHALL
  be completely unaffected (`CLI-HISTORY-AUDIT`).

## Architecture and interfaces

`rusty_fclone_core` (extends `FCLONE-DETECTION-001`'s public API):

```rust
pub enum ScanEvent { DuplicateGroup(DuplicateGroup), Error(FileError),
                      Progress(ScanProgress), Finished(ScanSummary) }
pub struct ScanProgress { pub files_scanned: u64, pub bytes_scanned: u64 }
```

`rusty_fclone-cli` (`src/main.rs`): `--format <text|json>` (default
`text`), `-y`/`--yes` (bool), `--find-duplicate-folders` (bool). JSON
serialization types (`JsonEvent`, `JsonAction`, `FolderPairActionJson`)
and progress-line rendering (`ProgressLine`) are CLI-only, not part of
the core crate's public surface. `--find-duplicate-folders` calls
`rusty_fclone_core::find_folder_duplicates` (`FCLONE-DETECTION-001`
FR-010) after the scan's event stream is fully drained, then (FR-013)
`folder_action::plan_folder`/`apply_folder` (`FCLONE-ACTION-001`
FR-009 through FR-011) per matched folder pair when `--action` was also
set. `folder_match_roots`/`group_fully_covered_by` decide, per
`DuplicateGroup`, whether it's already represented by a folder match's
own output — CLI-only classification logic, no core-crate involvement.

`rusty_fclone-cli` `history` module (ADR-0017, extended by ADR-0027):
`--history <path>` (`Option<PathBuf>`). `history::ScanRecord` (one
completed scan's summary, now including `actions: Vec<ActionRecord>`)
and `history::record_scan(path, &ScanRecord) -> rusqlite::Result<()>`
(creates the `scans`/`actions` schema if needed, appends one `scans` row
plus one `actions` row per entry, in one transaction). `history::
list_scans(path, limit) -> rusqlite::Result<Vec<ScanRow>>` and `history::
stats(path, since, until) -> rusqlite::Result<HistoryStats>` are the
read-only counterparts, backing the `history` subcommand. CLI-only, no
core-crate involvement — computed entirely from `ScanSummary`, the action
totals `run()` already tracks, and each group/pair's real `ApplyReport`/
`FolderApplyReport` outcome.

A separate `HistoryCli` (`#[derive(Parser)]`, its own `list`/`stats`
subcommands via `HistoryCommand`) is parsed only when `main` detects
`args[1] == "history"`, before `Cli::parse` ever runs — a manual
pre-dispatch rather than folding `history` into `Cli` as a
`#[command(subcommand)]`, since `Cli::root` is a required positional
argument clap can't cleanly disambiguate from a subcommand name at the
same position without restructuring every existing scan invocation
(ADR-0027).

## Data/state and invariants

- `ScanProgress`'s counters are cumulative from the start of the scan, not
  deltas since the last checkpoint.
- `ScanEvent::Progress` only ever appears before `ScanEvent::Finished`,
  consistent with `FCLONE-DETECTION-001`'s existing `Finished`-is-always-
  last invariant (unchanged by this addition).
- The confirmation prompt (FR-008) runs before `scan()` is even called —
  a decline touches nothing, including read-only traversal.
- When `--find-duplicate-folders` is set, `DuplicateGroup` reporting and
  action are deferred as a whole (FR-013) — no group is printed or acted
  on until the scan's `Finished` event and the folder-dedup pass have
  both completed, unlike the immediate per-event handling used when the
  flag is unset. The action-summary total (`FR-005`'s `action_summary`
  event / the text mode's final `reclaimed ... bytes` line) is printed
  after this deferred pass too, so it reflects both file-level and
  folder-level bytes reclaimed — never an undercount from being emitted
  before folder-level actions ran.

## Errors, failure, recovery, and observability

- A stdin read failure during the confirmation prompt is treated the same
  as an explicit decline (fail closed, not open).
- `--format json` errors are emitted as `error` events on stdout rather
  than the text mode's `warning: ...` line on stderr, so a JSON consumer
  doesn't need to parse two streams to see every failure.

## Security, privacy, and compatibility

- The confirmation prompt is an additional, opt-out (`--yes`) safety
  layer on top of `FCLONE-ACTION-001`'s existing dry-run-by-default,
  two-flag (`--action`+`--apply`) model — it doesn't replace or weaken
  that model, and `--yes` existing at all means automation/scripting isn't
  blocked by the new prompt.
- JSON path fields are rendered via `.display().to_string()`, which is
  lossy for non-UTF-8 paths (platform-dependent, generally a Unix-only
  concern) — the same tradeoff any string-based JSON path representation
  makes; not a regression from text mode, which already used `.display()`.

## Acceptance criteria

- FR-001 through FR-005 (JSON event shapes) are exercised by a manual CLI
  smoke test (`--format json` against a real duplicate pair, with and
  without `--action delete`) confirming the exact NDJSON shape; no
  automated test asserts on exact JSON string output (see open questions).
- FR-006 (new `ScanEvent` variant) is exercised indirectly: the CLI
  compiles against an exhaustive `match` on `ScanEvent`, so a missing
  handler is a compile error, not a runtime gap.
- FR-007 (terminal-gated progress rendering) is exercised by a manual
  smoke test confirming zero `\r`/progress output when stderr is piped
  (non-terminal), and JSON-mode progress events appearing at the expected
  256-file interval on a 600-file synthetic tree.
- FR-008/FR-009 (confirmation prompt) are exercised by
  `main::tests::apply_without_yes_is_blocked_by_the_unanswered_confirmation_prompt`
  (decline path, via unanswered/EOF stdin) and
  `main::tests::confirm_accepts_y_and_yes_case_insensitively`/
  `confirm_rejects_anything_else` (the decision logic in isolation). The
  accept path (real "y" input actually proceeding) is manually
  smoke-tested by piping `echo y |` into the CLI.
- FR-010/FR-011 (scan-history persistence) are exercised by
  `main::tests::history_flag_records_one_row_per_scan` and
  `main::tests::history_flag_records_the_action_result_when_an_action_runs`,
  plus `history::tests::*` (4 tests: schema creation, every field recorded
  correctly, multiple scans append rather than overwrite, reopening an
  existing database preserves prior rows). Manual smoke test additionally
  confirmed two real scans (a plain scan, then `--action delete --apply`)
  produced the expected two rows via a direct SQL query.
- FR-012 (`--find-duplicate-folders`) is exercised by
  `main::tests::find_duplicate_folders_flag_succeeds_on_an_exact_folder_match`
  and `find_duplicate_folders_flag_succeeds_with_json_format`, plus the
  underlying `rusty_fclone_core::folder_dedup` unit tests (`FCLONE-DETECTION-001`
  FR-010 through FR-013). Manual smoke test against a real three-directory
  tree (`photos/vacation`, `backup/vacation` with an extra file) confirmed
  the exact text and NDJSON output shapes, including the `Contained`
  subset/superset direction.
- FR-013 (`--find-duplicate-folders` + `--action`) is exercised by
  `main::tests::find_duplicate_folders_with_action_delete_apply_removes_the_subset_folder`
  (a real `Contained` match, `--apply`'d, confirmed against the
  filesystem: the subset folder is gone, the superset untouched),
  `find_duplicate_folders_with_action_delete_without_apply_is_a_dry_run`,
  and `find_duplicate_folders_with_action_still_acts_on_an_unrelated_duplicate_pair`
  — a regression test locking in that a `DuplicateGroup` outside any
  folder match still gets the normal per-file action once the deferred
  pass runs, not silently skipped. This last test was added after an
  earlier version of this feature was found, by its own first test run,
  to apply `--action` to individual groups live during the scan —
  consuming a folder match's defining file-level evidence before
  `find_folder_duplicates` ever ran, so the match (and the CLI's
  "prune the now-empty folder" behavior) silently disappeared. Deferring
  reporting/action until after the folder-dedup pass (this version's
  actual behavior) fixed it; the regression test exists so this doesn't
  reappear silently. Manual smoke test additionally confirmed real
  filesystem state (`find`, before/after) and the exact text/NDJSON
  output shapes for both a preview and a real `--apply`'d run.
- FR-016 (`--reference`) is exercised by
  `main::tests::reference_path_overrides_keep_rule_and_is_never_acted_on`
  (a protected file that would lose to an unprotected one under the
  default alphabetical `--keep-rule` is kept instead) and
  `find_duplicate_folders_with_reference_protects_a_file_and_blocks_the_prune`
  (combined with `--find-duplicate-folders`: the protected file survives
  and the subset directory is not pruned). Manual smoke test against a
  real filesystem confirmed both: a file-level `--action trash --reference
  <dir> --apply` run kept the protected copy and trashed the other
  despite alphabetical ordering favoring the unprotected one; a
  folder-level `--find-duplicate-folders --action delete --reference
  <dir> --apply` run against a subset folder containing a protected file
  left the file and the folder itself untouched.
- FR-017 (`--action move`/`copy`, `--archive-dir`) is exercised by
  `main::tests::action_with_apply_actually_moves_into_the_archive_directory`,
  `action_with_apply_actually_copies_into_the_archive_directory_and_keeps_the_original`,
  and `action_move_without_archive_dir_fails_before_touching_anything`.
  Manual smoke test against a real filesystem confirmed all three: `move`
  relocated the redundant copy to its mirrored archive path; `copy`
  archived a copy while leaving both originals untouched and reported `0`
  bytes reclaimed, with a repeated run against the same tree failing
  cleanly on the already-archived destination rather than clobbering it;
  `move` without `--archive-dir` was rejected before any scan ran.
- FR-018 (per-action history rows, `history` subcommand) is exercised by
  `main::tests::history_flag_records_one_action_row_per_file_acted_on`,
  `history_flag_records_no_action_rows_for_a_dry_run`,
  `history_flag_records_action_rows_for_folder_level_actions`,
  `history_list_reports_recent_scans_newest_first`,
  `history_stats_aggregates_across_the_database`, and
  `history_subcommand_reports_failure_for_an_unreadable_database`, plus
  `history::tests::*` (11 tests: action-row recording, correct scan
  correlation across multiple scans, `list_scans`'s newest-first/limit
  behavior and empty-database case, `stats`'s aggregation with and
  without a date range and its empty-database case). Manual smoke test
  against a real filesystem: two real scans (one `report`-only, one
  `--action trash --apply`) against a `--history` database, followed by
  `history list` (both `--format text` and `--format json`) and `history
  stats`, confirmed the exact row counts, per-action detail (right path/
  kind/bytes/succeeded), and aggregate totals matched what actually
  happened on disk; a plain `rusty-fclone <ROOT>` invocation confirmed
  unaffected by the new `history` keyword dispatch.

## Verification plan

Unit tests in `main::tests` (CLI crate): `confirm_accepts_y_and_yes_case_insensitively`,
`confirm_rejects_anything_else` (prompt decision logic, isolated from I/O),
`apply_without_yes_is_blocked_by_the_unanswered_confirmation_prompt`
(end-to-end: declining leaves the filesystem untouched, exits success),
`json_format_reports_duplicates_as_ndjson` (smoke-level: JSON format
doesn't crash and exits success on a real duplicate pair). The three
existing `action_with_apply_actually_*`/dry-run tests were updated to set
`yes: true` so they exercise the actual apply path rather than the new
confirmation gate.

Manual smoke tests: `--format json` (plain and with `--action delete`)
inspected against the schema above; confirmation prompt decline (empty
stdin) and accept (`echo y |`) both confirmed against real filesystem
state; progress checkpoints confirmed present in JSON mode and absent
(no `\r` spam) in text mode against a piped, non-terminal stderr;
`--history` across two real scans confirmed correct via a direct SQL
query against the resulting database.

## Traceability

See `docs/traceability/TRACEABILITY.md`.

## Open questions

- No automated test parses and asserts on the exact JSON emitted (field
  names, nesting) — only that `--format json` runs successfully. A golden-
  file or `serde_json::Value`-based assertion test would close this if the
  schema needs to be guaranteed stable for downstream consumers later.
- The JSON schema isn't versioned or promised stable yet; a consumer
  depending on it today should expect it may change without a major
  version bump until that's explicitly decided.
- Whether `--format json` should also suppress the human-readable
  confirmation prompt's wording (currently plain text regardless of
  `--format`) wasn't raised as a problem — the prompt happens before any
  output format's event stream starts, so there's no literal mixing, but
  a strictly-machine-consumed pipeline would still see that one non-JSON
  line on stderr if `--yes` isn't passed.
- `--history`'s schema isn't versioned; a future incompatible change would
  need a migration story that doesn't exist yet (not needed for two
  `CREATE TABLE IF NOT EXISTS` statements in v1).
- `history stats --since`/`--until` take raw Unix timestamps, not
  human-readable date strings — deliberate, to avoid adding a
  date-parsing dependency for a "at minimum" exit-gate ask; a future unit
  could add friendlier parsing on top without changing the stored schema
  or query semantics.
- A real directory literally named `history` at a scan root needs
  `rusty-fclone ./history` to disambiguate from the reserved subcommand
  keyword — narrow, and not expected to matter in practice (ADR-0027).

## Change history

- 0.3.5 (2026-08-27): Added per-action history detail rows and a
  `rusty-fclone history <list|stats>` query subcommand (FR-018,
  `CLI-HISTORY-AUDIT`, third and final unit of `docs/roadmap/
  DEDUP-GAP-IMPLEMENTATION-PLAN.md`'s Phase 2, closing Phase 2 in full).
  Closes the two gaps `CLI-SCAN-HISTORY` (ADR-0017) deliberately deferred:
  per-file/pair audit detail, and reading history back. `history` is a
  reserved top-level keyword, dispatched manually in `main` before
  `Cli::parse` runs, rather than folded into `Cli` as a
  `#[command(subcommand)]` — `Cli::root`'s required positional argument
  makes that ambiguous without a breaking restructure this unit's scope
  doesn't call for. GUI's "Export (JSON)" button (Dashboard) is wired for
  real via a plain `<a download>`/object-URL, no new Tauri plugin needed;
  "Import history" stays an explicit disabled placeholder, blocked on the
  same Tauri `dialog`/`fs` plugin prerequisite already tracked for the
  GUI's root-path field. ADR-0027.
- 0.3.4 (2026-08-27): Added `--action move`/`copy` and `--archive-dir`
  (FR-017), the CLI surface for `FCLONE-ACTION-001`'s archive-folder
  actions (`ACTION-MOVE-COPY`, second unit of `docs/roadmap/
  DEDUP-GAP-IMPLEMENTATION-PLAN.md`'s Phase 2). `--archive-dir` is
  required by, and only meaningful with, `--action move`/`copy` —
  validated explicitly (not via clap's generic required-if machinery) so
  the error names the specific action needing it. ADR-0026.
- 0.3.3 (2026-08-27): Added `--reference` (FR-016), the CLI surface for
  `FCLONE-ACTION-001`'s reference-folder guardrail
  (`ACTION-REFERENCE-FOLDERS`, first unit of `docs/roadmap/
  DEDUP-GAP-IMPLEMENTATION-PLAN.md`'s Phase 2). Repeatable, threaded
  through both the per-file and (combined with
  `--find-duplicate-folders`) folder-level action paths. ADR-0025.
- 0.3.2 (2026-08-26): Added `--keep-rule` (FR-015), the CLI surface for
  `FCLONE-ACTION-001` 0.5.0's new `select::Rule`/`choose_keep`
  (`SELECTION-RULES`, third and final Phase 1 unit of
  `docs/roadmap/DEDUP-GAP-IMPLEMENTATION-PLAN.md`). The `--format json`
  action shape (FR-002) gained a `keep_reason` field; text output's
  `keep:` line now shows the reason in parentheses. No existing flag or
  default behavior changed — `--keep-rule`'s default (`alphabetical`)
  reproduces the exact prior behavior.
- 0.3.1 (2026-08-26): Added `--min-size`/`--max-size`/`--include-ext`/
  `--exclude-ext`/`--exclude-path` (FR-014), the CLI surface for
  `FCLONE-DETECTION-001` 0.2.1's new scan-filter fields
  (`DETECTION-SCAN-FILTERS`, first unit of the phased plan in
  `docs/roadmap/DEDUP-GAP-IMPLEMENTATION-PLAN.md`). No existing flag or
  output shape changed.
- 0.3.0 (2026-08-25): Added folder-level action support (FR-013) —
  `--find-duplicate-folders` combined with `--action <kind>` now plans
  (and, with `--apply`, applies) `kind` for every folder match via the
  new `rusty_fclone_core::folder_action` (ADR-0023), in text and
  `--format json` (a new `action` array on `folder_exact`/
  `folder_contained` events). Fixed a real ordering bug found while
  building this: applying `--action` to individual duplicate groups live
  during the scan could delete the very files a folder match's
  detection depends on before `find_folder_duplicates` ever ran, making
  the match silently vanish. Fixed by deferring all `DuplicateGroup`
  reporting and action until after the folder-dedup pass completes,
  whenever `--find-duplicate-folders` is set — unchanged, immediate
  behavior when it isn't. `FCLONE-ACTION-001` 0.3.0.
- 0.2.2 (2026-08-25): Added `--find-duplicate-folders` (FR-012), which
  runs the new `rusty_fclone_core::find_folder_duplicates` after the scan
  completes and reports `FolderMatch::Exact`/`Contained` results in both
  text and `--format json` (new `folder_exact`/`folder_contained` NDJSON
  event types). Off by default. `FCLONE-DETECTION-001` 0.2.0, ADR-0021.
- 0.2.1 (2026-08-25): Reworded the GUI/TUI Non-goal — a GUI now exists as
  a separate spec/crate (`GUI-UX-001`, ADR-0020), so the prior wording
  ("anything beyond a plain terminal/pipe-friendly CLI") read as a
  whole-project claim rather than this document's own scope boundary. No
  functional requirement changed.
- 0.2.0 (2026-08-24): Added `--history <path>` (FR-010/FR-011,
  `DETECTION-INCREMENTAL-CACHE`'s companion unit) — a SQLite-backed
  per-scan summary record for longer-term analytics, off by default. New
  `history` module (`rusty_fclone-cli` only, no core-crate change). ADR-0017.
- 0.1.0 (2026-08-24): Initial implementation and specification. Closes
  the `CLI-UX` roadmap unit. ADR-0015.
