# ADR-0027: Per-action history rows and a `history` query subcommand

- Status: Accepted
- Date: 2026-08-27
- Related: ADR-0017 (scan-history persistence, extended here), `docs/roadmap/DEDUP-GAP-IMPLEMENTATION-PLAN.md` (`CLI-HISTORY-AUDIT`, Phase 2)

## Context

`CLI-SCAN-HISTORY` (ADR-0017) deliberately scoped `--history` to one row
per *scan*, not one row per file/action, and shipped with no way to read
the data back — both explicitly named as deferred gaps in
`docs/PROJECT-STATUS.md`. This unit closes both: an audit trail detailed
enough to answer "what actually happened to file X," and a way to query
accumulated history without hand-written SQL.

Two design questions came up:

1. Where does per-action detail live, and what triggers writing it?
2. How does a *query* subcommand fit into a CLI whose top-level shape is
   `rusty-fclone <ROOT> [FLAGS]` — a single required positional argument,
   not a `verb <args>` structure with room for a second, unrelated verb?

## Decision

- **A new `actions` table, one row per file/pair a scan actually acted
  on** — FK'd to its `scans` row, written in the same transaction. Scoped
  to *applied* actions only: a preview (`--action <kind>` without
  `--apply`) plans but never runs anything, so there's nothing real yet
  to audit; the existing `scans.action_applied` column already carries
  "this scan chose not to apply" for the dry-run case. Collected in an
  in-memory `Vec` only when `--history` is actually set (`cli.history.
  is_some().then(Vec::new)`), so a scan without `--history` pays nothing
  for this — the same "opt-in, no cost if unused" posture `ScanOptions`'
  every other optional field already has.
- **One correlation helper, not two.** `ApplyReport` (per-file) and
  `FolderApplyReport` (per-folder-pair) both carry `succeeded: Vec<PathBuf>`/
  `failed: Vec<FileError>`, and `FileAction`/`FolderFilePair` both reduce
  to a `(path, bytes)` pair for this purpose — so `record_action_outcomes`
  takes those two report fields plus a `(path, bytes)` iterator and a
  `kind` word, letting `handle_group` and `report_folder_matches` both
  record through the same logic instead of duplicating the
  succeeded/failed lookup twice.
- **`history` is a reserved top-level keyword, dispatched manually in
  `main` before `Cli::parse` runs** — not folded into `Cli` as a
  `#[command(subcommand)]`. `Cli::root` is a required positional
  argument; clap's derive API can't cleanly express "a subcommand name OR
  a positional path" at the same argument position without restructuring
  every existing scan invocation into `rusty-fclone scan <ROOT> ...`, a
  breaking change to this project's primary CLI shape that this unit's
  scope doesn't call for. Instead, `main` peeks at `args[1]`: exactly
  `"history"` routes to a separate `HistoryCli` (its own `#[derive(Parser)]`
  with `list`/`stats` subcommands); anything else parses via the existing
  `Cli` exactly as before. Every existing invocation is unaffected byte
  for byte. The cost: a real directory literally named `history` at the
  scan root needs `rusty-fclone ./history` to disambiguate — a narrow,
  well-precedented tradeoff (`git`, among others, reserves subcommand
  names the same way) not expected to matter in practice.
- **`--since`/`--until` take raw Unix timestamps**, matching how
  `scans.started_at` is already stored, rather than adding a date-parsing
  dependency for human-readable date strings. A future unit can add
  friendlier parsing without changing the stored format or this one's
  query semantics.
- **GUI: "Export (JSON)" is wired for real, via a plain `<a download>` +
  object-URL** — a standard webview download, not a native save dialog,
  so it needed no new Tauri plugin or permission grant. It exports
  `state.scanHistory`, the session-scoped array the Dashboard's Recent
  Scans table already tracks in memory — not the persisted SQLite
  database, which the GUI has no reader for. **"Import history" stays an
  explicit, disabled placeholder** — reading an arbitrary `--history`
  database needs a real filesystem path from the user, and getting one
  from a webview needs Tauri's `dialog`/`fs` plugin, already tracked as a
  deferred prerequisite elsewhere in this project's docs (the GUI
  root-path field's own native picker). Wiring that prerequisite is a
  larger, separately-scoped piece of work than "wire two buttons," so
  this unit ships the half that's genuinely deliverable today and leaves
  the other honestly blocked rather than half-implemented.

## Consequences

- `rusty_fclone-cli`'s `history` module gains `ActionRecord`, `ScanRow`,
  `HistoryStats`, `list_scans`, and `stats` — all read/write against the
  same SQLite database `--history` already used, no new dependency.
- The CLI gains a second top-level invocation shape
  (`rusty-fclone history <list|stats>`) alongside the existing
  `rusty-fclone <ROOT> [FLAGS]` — the first departure from "one flat
  argument set" this project's CLI has had, deliberately implemented as
  additive pre-dispatch rather than a restructure, so it costs nothing to
  every existing script or muscle-memory invocation.
- `handle_group`/`report_folder_matches` each gained one new parameter
  (`Option<&mut Vec<ActionRecord>>`) — both already had
  `#[allow(clippy::too_many_arguments)]` from earlier units, so this
  didn't introduce a new lint suppression.
- GUI's "Export (JSON)" now does something real but is honestly scoped to
  the current session — closing a launcher and reopening the GUI still
  loses that history, same as the Recent Scans table it's drawn from
  already did before this change. Not manually verified through a real
  rendered window in this environment (no display/`xdotool`), the same
  standing gap every other GUI-facing unit this session has left open.
