# ADR-0010: Structured observability via `tracing`

- Status: Accepted
- Date: 2026-08-24
- Related: ADR-0002 (I/O and concurrency), ADR-0004 (engine API), ADR-0005
  (workspace structure)

## Context

The engine had no logging or progress observability: a slow or stuck scan
was a black box, and diagnosing where time went (traversal vs. partial-hash
vs. full-hash vs. verify) meant reaching for a profiler or adding temporary
`eprintln!`s. This was tracked as a known gap on the roadmap. A duplicate
finder that streams events (ADR-0004) is naturally well-suited to
structured, leveled tracing rather than ad hoc prints — the same staged
pipeline that emits `ScanEvent`s has clear stage boundaries worth
instrumenting.

## Decision

- **Library**: `tracing` (spans + events) in `rusty_fclone-core`, with
  `tracing-subscriber` (`env-filter` feature) wired up only in
  `rusty_fclone-cli`. The core crate stays CLI-agnostic (ADR-0005): it emits
  `tracing` spans/events but never initializes a subscriber itself, so a
  library consumer chooses (or ignores) how they're rendered. `tracing` was
  chosen over the plain `log` facade because the engine's structure —
  nested stages (`run_scan` → `process_size_group` → hashing) running
  across rayon's parallel iterators — maps directly onto `tracing`'s spans,
  which `log` has no equivalent for.
- **What's instrumented**: `#[tracing::instrument]` spans on `traverse`,
  `run_scan`, and `process_size_group` (the natural pipeline stage
  boundaries), plus `tracing::info!`/`debug!`/`trace!`/`warn!` events at
  each stage transition (traversal finished, size-grouping complete, scan
  finished) and every per-file error path (already reported via
  `ScanEvent::Error`/`FileError` for API consumers — tracing gives the same
  information a human can `grep`/filter live without consuming the event
  stream). Deliberately not instrumented at per-file granularity inside the
  hot `into_par_iter()` hashing loops: a span or event per file would
  dominate the actual work on a large tree for no diagnostic benefit over
  the per-group summary events.
- **CLI wiring**: `rusty_fclone-cli` initializes `tracing_subscriber::fmt`
  writing to stderr (stdout stays reserved for duplicate-group output, so
  piping/redirecting either stream works independently). Verbosity is a
  repeated `-v`/`--verbose` flag (`0` = warn, `1` = info, `2` = debug, `3+`
  = trace), but `RUST_LOG` always wins when set — matching the ecosystem
  convention so users who already know `tracing`/`env_logger` conventions
  get the filtering syntax they expect (per-module/per-target overrides,
  etc.) without the CLI needing to expose that as bespoke flags.

## Consequences

- New dependencies: `tracing`, `tracing-subscriber` (workspace-level,
  `tracing` also in `rusty_fclone-core`'s own `Cargo.toml`). Both are
  widely-used, actively maintained crates with no notable downsides for a
  CLI tool.
- Default (no `-v`, no `RUST_LOG`) output is unchanged from before this
  ADR: only warnings print, and only to stderr — existing scripts piping
  stdout are unaffected.
- Spans nest through rayon's parallel iterators (`tracing`'s span context
  is thread-local and propagates correctly across the `into_par_iter()`
  closures already used for hashing), so `-vv` output groups
  per-size-group events under their `process_size_group` span rather than
  interleaving indistinguishably.
- This closes the roadmap's "no structured logging/progress observability
  yet" known gap. A live progress bar/percentage display (as opposed to
  leveled log output) is a separate concern, tracked under the `CLI-UX`
  roadmap unit's planned `ScanEvent::Progress` variant — this ADR is about
  diagnosability, not end-user progress UX.
