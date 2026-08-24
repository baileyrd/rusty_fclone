# Project Status
- Last verified main commit: `a77f1c4` (PR #10, merged) — the incremental
  hash-cache work below is on its own branch, not yet merged
- Verified at: 2026-08-24
- Current milestone: `DETECTION-INCREMENTAL-CACHE` (first of two new,
  user-requested units — a `redb` incremental hash cache and, next, a
  SQLite scan-history store for longer-term analytics; both stemmed from a
  "what database would fit here" design discussion, not the earlier
  "build it all" batch). See `docs/roadmap/ROADMAP.md`.
- Health: green — workspace builds, lints, and tests clean on the pinned
  toolchain

## Completed
- `DETECTION-BASELINE`, `DETECTION-BENCHMARK`, `DETECTION-BENCHMARK-VS-FCLONES`,
  `DETECTION-ADAPTIVE-SAMPLE-SIZE`, `DETECTION-IO-THREAD-SIZING` — the full
  detection engine, benchmarked and tuned to beat or match fclones on all
  four synthetic scenarios. Merged to `main` via PR #1 and #2. See
  `docs/benchmarks/FCLONES-COMPARISON.md` for the numbers.
- `ACTION-LAYER` — delete/hardlink redundant copies, dry-run by default.
  `rusty_fclone_core::action` module (`plan`/`apply`, ADR-0009) plus
  `--action <report|delete|hardlink>` and `--apply` CLI flags. Merged via
  PR #3.
- Two known gaps closed via PR #4:
  - Symlink-cycle safety net: `traversal::tests::follow_symlinks_terminates_on_a_cycle`
    proves jwalk's loop detection under `--follow-symlinks` actually works,
    bounded by a 10s timeout so a regression fails the test instead of
    hanging CI.
  - Streaming full-file hashing: `IoPool::hash_full_file`/`files_equal`
    stream through fixed 1 MiB chunks instead of buffering whole files;
    `--verify`'s peak memory no longer scales with duplicate-group size
    (ADR-0002 addendum).
- Structured observability (`tracing`): spans on `traverse`, `run_scan`,
  `process_size_group`; leveled events at stage boundaries and every
  per-file error path; CLI wires up `tracing-subscriber` on stderr with a
  repeated `-v`/`--verbose` flag (`RUST_LOG` always takes precedence).
  ADR-0010. Merged via PR #5.
- Path storage: `Arc<Path>` instead of `PathBuf` for every path carried
  through the detection pipeline. ADR-0011. Merged via PR #6.
- `DETECTION-TRAVERSAL-COLLAPSE-FUSION`: traversal and hardlink-collapse
  run as one streaming pass. ADR-0012. Merged via PR #7.
- `DETECTION-DEVICE-AWARE-IO-SIZING`: `io_threads` auto-detects an
  oversubscribed pool on a rotational disk (Linux, best-effort) or plain
  `cores` otherwise. ADR-0013. Merged via PR #8.
- `ACTION-REFLINK`: new `ActionKind::Reflink` via `reflink-copy`'s strict
  `reflink` (no silent copy fallback). ADR-0014, `FCLONE-ACTION-001` 0.2.0.
  Merged via PR #9.
- `CLI-UX`: `--format text|json` (NDJSON), new `ScanEvent::Progress`
  (terminal-gated live progress line), `-y`/`--yes`-bypassable
  confirmation prompt before `--apply` mutates anything. ADR-0015, new
  `CLI-UX-001` spec. Merged via PR #10 — closed out the original "build it
  all and close all gaps" batch (PRs #4–#10, plus a docs-only #11).

## In progress
- `DETECTION-INCREMENTAL-CACHE`: new opt-in `cache` module backed by
  `redb` — a file whose `(size, mtime)` match a cached entry reuses its
  full hash instead of being re-read and re-hashed, skipping both the
  partial-hash and full-hash stages for that file. New
  `ScanOptions::cache_path: Option<PathBuf>` / CLI `--cache <path>`, off
  by default. ADR-0016, `FCLONE-DETECTION-001` 0.1.8 (NFR-004).
  Implemented, tested (fmt/clippy/test/bench all green, 60/60 tests — 7
  new `cache` unit tests plus 2 new `pipeline` integration tests: cached
  vs. uncached scans produce identical results, and a changed file is
  never served a stale cached hash), and manually smoke-tested via
  `-vvv` trace output (zero cache hits on a cold run against two 5 MB
  duplicate files, exactly two hits — one per file — on an immediately
  following warm run, correct duplicate-group results throughout). Not
  yet pushed through the PR → CI → merge → sync loop.
- Next up after this merges: a SQLite-backed scan-history store
  (`CLI-SCAN-HISTORY`, working title) for longer-term analytics —
  per-scan summaries only (files/bytes scanned, duplicate groups/files,
  action results), not per-file/per-group detail, and no query/report
  subcommand yet (explicitly deferred, same scoping pattern as everything
  else in this project). CLI-only (`rusqlite`), no core-crate change,
  since it's just recording a completed scan's summary.

## Blocked
- None.

## Next
1. Get CI green and merge the `DETECTION-INCREMENTAL-CACHE` PR, sync main.
2. Implement `CLI-SCAN-HISTORY` (SQLite scan-summary persistence via
   `--history <path>`), same validation/PR/merge/sync loop.
3. Follow-on units intentionally left open by earlier scoping decisions
   (each needs its own design work before starting):
   `DETECTION-STREAMING-OVERLAP` proper (full pipeline overlap, needs a
   `ScanEvent` finality-contract decision first), `DETECTION-LINUX-FASTPATH`
   proper (io_uring/FIEMAP, needs an async runtime and unsafe FFI, its own
   ADR).

## Validation
- `cargo fmt --all --check`: pass (2026-08-24)
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass (2026-08-24)
- `cargo test --workspace`: pass, 60/60 (2026-08-24)
- `cargo bench -p rusty_fclone-core --no-run`: pass (2026-08-24)
- Manual CLI smoke tests across the project: verbosity flags, `RUST_LOG`
  override, default output silent on success, action dry runs, JSON
  format, progress checkpoints, confirmation prompt decline/accept, and
  now cold/warm `--cache` behavior (2026-08-24)

## Risks and decisions needed
- The action layer is the first genuinely destructive capability in this
  codebase. Its safety model (dry-run default, two-flag confirmation, plus
  the interactive confirmation prompt) is documented and tested, but has
  not been used against a real, valuable directory tree outside this
  session's smoke tests — treat it with appropriate caution before
  pointing it at anything you care about.
- `DETECTION-DEVICE-AWARE-IO-SIZING`'s rotational-disk detection logic is
  unverified against a real spinning disk (this environment's storage
  resolves to the safe `cores` fallback).
- `ACTION-REFLINK`'s success path is unverified end-to-end in this
  environment (no CoW-capable filesystem available to test against).
- `CLI-UX-001`'s JSON schema isn't versioned or promised stable yet.
- `DETECTION-INCREMENTAL-CACHE`'s benchmark verification was inconclusive:
  `cargo bench`'s comparison against its saved baseline swung between
  "+144% regressed" and "-6.8% improved" across consecutive runs of
  *identical* code (no cache flag passed either time), which reflects this
  sandboxed container's variable background load rather than a real
  effect — the cache-off code path is structurally a no-op (a `None`
  short-circuit), but a clean before/after number was not obtained here.
  If real performance validation matters, re-run the benchmark suite on a
  quieter, dedicated machine.
- `DETECTION-INCREMENTAL-CACHE`'s only invalidation signal is `(size,
  mtime)` — a file whose content changes without its mtime updating
  (contrived, or an unreliable filesystem) would be served a stale hash.
  Same trust model as `make` and most incremental build tools; not
  treated as a gap warranting content-based invalidation for v1.
- `DETECTION-STREAMING-OVERLAP` proper needs a decision on how to relax or
  redesign `ScanEvent`'s "no group revision after emission" finality
  contract (ADR-0004) before it can be implemented — not yet made.
