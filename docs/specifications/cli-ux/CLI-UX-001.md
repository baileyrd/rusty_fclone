# CLI-UX-001 — CLI Output, Progress, and Confirmation
- Version: 0.2.1
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
  `{"kind":<string>,"kept":<string>,"applied":<bool>,"planned":[<string>,...],"succeeded":[<string>,...],"failed":[<string>,...],"bytes_reclaimed":<u64>}`;
  otherwise `null`.
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

## Architecture and interfaces

`rusty_fclone_core` (extends `FCLONE-DETECTION-001`'s public API):

```rust
pub enum ScanEvent { DuplicateGroup(DuplicateGroup), Error(FileError),
                      Progress(ScanProgress), Finished(ScanSummary) }
pub struct ScanProgress { pub files_scanned: u64, pub bytes_scanned: u64 }
```

`rusty_fclone-cli` (`src/main.rs`): `--format <text|json>` (default
`text`), `-y`/`--yes` (bool). JSON serialization types (`JsonEvent`,
`JsonAction`) and progress-line rendering (`ProgressLine`) are CLI-only,
not part of the core crate's public surface.

`rusty_fclone-cli` `history` module (ADR-0017): `--history <path>`
(`Option<PathBuf>`). `history::ScanRecord` (one completed scan's summary)
and `history::record_scan(path, &ScanRecord) -> rusqlite::Result<()>`
(creates the database/table if needed, appends one row). CLI-only, no
core-crate involvement — computed entirely from `ScanSummary` and the
action totals `run()` already tracks.

## Data/state and invariants

- `ScanProgress`'s counters are cumulative from the start of the scan, not
  deltas since the last checkpoint.
- `ScanEvent::Progress` only ever appears before `ScanEvent::Finished`,
  consistent with `FCLONE-DETECTION-001`'s existing `Finished`-is-always-
  last invariant (unchanged by this addition).
- The confirmation prompt (FR-008) runs before `scan()` is even called —
  a decline touches nothing, including read-only traversal.

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
  need a migration story that doesn't exist yet (not needed for a single
  `CREATE TABLE IF NOT EXISTS` in v1). No query/report subcommand exists
  yet either — reading recorded history back is a future unit.

## Change history

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
