# ADR-0005: Workspace shape — library crate + thin CLI

- Status: Accepted
- Date: 2026-08-24

## Context

The repository could ship as a single binary crate (simpler to start, split
later once there's a second real consumer) or as a workspace with a
detection-engine library crate and a separate CLI crate from day one.

## Decision

A Cargo workspace with two members:

- `crates/rusty_fclone-core` — the detection engine, no CLI concerns
  (argument parsing, process exit codes, stdout formatting). Public surface:
  `scan`, `ScanHandle`, `ScanOptions`, `ScanEvent`, `DuplicateGroup`,
  `ScanSummary`, `ScanError`, `FileError`.
- `crates/rusty_fclone-cli` — a thin binary (`rusty-fclone`) that parses
  arguments with `clap`, calls `rusty_fclone_core::scan`, and formats the
  event stream for a terminal.

Shared package metadata (`version`, `edition`, `license`, `repository`) and
dependency versions live in `[workspace.package]` /
`[workspace.dependencies]` at the workspace root; member crates reference
them via `.workspace = true` rather than repeating values.

## Consequences

- The engine is independently testable and benchmarkable without spinning up
  a process or parsing CLI output — `rusty_fclone-core`'s own test suite
  exercises it directly (see `crates/rusty_fclone-core/src/*.rs` `#[cfg(test)]`
  modules).
- A future second consumer (a GUI, a library user, an "apply the dedup"
  action crate) has a clean crate boundary to depend on instead of needing
  to be carved out of a monolithic binary later.
- Both crates are `publish = false` for now — no decision has been made yet
  about publishing to crates.io.
