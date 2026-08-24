# ADR-0017: Scan-history persistence via SQLite

- Status: Accepted
- Date: 2026-08-24
- Related: ADR-0005 (core stays CLI-agnostic), ADR-0016 (the companion
  `redb` cache from the same "what database fits here" discussion)

## Context

ADR-0016 added a cache for speeding up repeated scans of the same tree.
The complementary, longer-term need is answering questions *across* many
scans over time — "how much space have I reclaimed this month," "is this
tree's duplicate count growing" — which a KV cache keyed by path can't
answer at all (it's not designed to be queried by anything other than
exact path). That's a genuine relational/analytical access pattern:
aggregating and filtering across many rows, not a single-key lookup.

## Decision

- **SQLite via `rusqlite` (`bundled` feature), not `redb`, and not a new
  service**: real `GROUP BY`/aggregate queries over accumulated history
  are exactly what a KV store makes you reimplement badly, and a
  single-user CLI tool has no business standing up a database server. The
  `bundled` feature statically links SQLite (no system dependency),
  matching this project's "self-contained binary" pattern already
  established by `reflink-copy` and `redb` (no runtime dependency beyond
  the compiled binary itself).
- **CLI-crate-only, no core-crate change**: unlike ADR-0016's cache (which
  changes what the detection *engine* does, so it had to live in
  `rusty_fclone-core`), history is purely "record what already happened,"
  computable entirely from data the CLI already has once a scan
  completes (`ScanSummary` plus the action totals it already tracks for
  the text/JSON trailer). Keeping it CLI-only respects ADR-0005 — the
  core crate stays unaware output formats or history even exist.
- **Opt-in via `--history <path>`**, off by default — same reasoning as
  every other flag in this CLI, including ADR-0016's `--cache`.
- **Scope: per-scan summaries only, not per-file or per-group detail**.
  One row per completed scan (`root`, `started_at`, `files_scanned`,
  `bytes_scanned`, `duplicate_groups`, `duplicate_files`, and the action's
  `kind`/`applied`/`bytes_reclaimed`/`files_acted_on` if one ran). This is
  a deliberate scope boundary, not an oversight: per-file/per-group
  history would mean a table growing unbounded with tree size and scan
  frequency, for detail nobody asked for; the summary table is exactly
  enough to answer the "longer-term analytics" questions above (trends
  over time) without that cost.
- **No query/report subcommand in this unit**: this ADR only makes the
  data exist (via `INSERT`); reading it back is left to a future unit, or
  to a user pointing `sqlite3`/DuckDB/anything else at the file directly
  in the meantime. Matches this project's established scoping pattern of
  landing one well-tested capability at a time rather than gold-plating a
  full feature in one pass.
- **Write failures degrade gracefully**: a failed `--history` write prints
  a warning and does not change the scan's own exit code — recording
  history is adjunct to the scan, not a reason to report the scan itself
  as failed.

## Consequences

- New dependency: `rusqlite` (`bundled` feature, pulling in a vendored
  SQLite C build via `libsqlite3-sys`), `rusty_fclone-cli` only.
- Schema (`scans` table, created via `CREATE TABLE IF NOT EXISTS` on first
  use): `id` (autoincrement), `root`, `started_at` (Unix seconds),
  `files_scanned`, `bytes_scanned`, `duplicate_groups`, `duplicate_files`,
  `action_kind` (nullable text), `action_applied` (nullable bool),
  `bytes_reclaimed` (nullable), `files_acted_on` (nullable) — null action
  fields mean no `--action` was requested that run.
- `u64` scan counters are stored as SQLite's native `i64` (`as i64` casts
  at the write boundary) — rusqlite has no `ToSql` impl for `u64` since it
  can't losslessly represent every value; safe in practice since file/byte
  counts never approach `i64::MAX`.
- Manually smoke-tested end-to-end: two scans against the same tree (a
  plain scan, then `--action delete --apply -y`) both recorded correctly,
  confirmed via a direct SQL query against the resulting database showing
  both rows with the expected `action_kind`/`action_applied`/
  `bytes_reclaimed` values.
- No schema versioning/migration story yet — a future incompatible schema
  change would need one; not needed for a single `CREATE TABLE IF NOT
  EXISTS` in v1.
