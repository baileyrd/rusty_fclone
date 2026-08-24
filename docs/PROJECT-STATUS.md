# Project Status
- Last verified main commit: `b616294` (PR #14, merged) — the redb
  incremental hash cache (PR #12) and SQLite scan-history store (PR #13)
  are fully merged; this branch adds `RELEASE-BINARIES` (PR TBD) on top
- Tagged: `v0.1.0` at commit `b616294`, GitHub Release published but
  (until `RELEASE-BINARIES` lands and is manually dispatched against it)
  with no attached binaries
- Verified at: 2026-08-24
- Current milestone: `RELEASE-BINARIES` — tag-triggered release workflow
  so `v0.1.0` (and every future `vX.Y.Z` tag) gets real downloadable
  binaries. See `docs/roadmap/ROADMAP.md`.
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
- Two known gaps closed via PR #4: symlink-cycle safety net; streaming
  full-file hashing (ADR-0002 addendum).
- Structured observability (`tracing`) — ADR-0010. Merged via PR #5.
- Path storage: `Arc<Path>` instead of `PathBuf`. ADR-0011. Merged via PR #6.
- `DETECTION-TRAVERSAL-COLLAPSE-FUSION` — ADR-0012. Merged via PR #7.
- `DETECTION-DEVICE-AWARE-IO-SIZING` — ADR-0013. Merged via PR #8.
- `ACTION-REFLINK` — ADR-0014, `FCLONE-ACTION-001` 0.2.0. Merged via PR #9.
- `CLI-UX`: `--format text|json`, `ScanEvent::Progress`, confirmation
  prompt. ADR-0015, `CLI-UX-001` 0.1.0. Merged via PR #10 — closed out the
  original "build it all and close all gaps" batch (PRs #4–#10, plus a
  docs-only #11).
- `DETECTION-INCREMENTAL-CACHE`: new opt-in `cache` module backed by
  `redb` — a file whose `(size, mtime)` match a cached entry reuses its
  full hash, skipping both the partial-hash and full-hash stages for that
  file. `ScanOptions::cache_path`/CLI `--cache <path>`, off by default.
  ADR-0016, `FCLONE-DETECTION-001` 0.1.8 (NFR-004). Implemented, tested
  (60/60 tests: 7 new `cache` unit tests + 2 `pipeline` integration
  tests), manually smoke-tested via `-vvv` trace output (zero hits cold,
  exactly one hit per file warm, correct results throughout). Merged via
  PR #12. Benchmark verification of the cache-off path was inconclusive
  in this environment (noisy shared-container load swung the criterion
  comparison between "+144% regressed" and "-6.8% improved" across
  consecutive runs of identical code) — the code path is structurally
  unaffected (a `None` short-circuit), so not treated as a real
  regression signal; see ADR-0016's consequences.
- `CLI-SCAN-HISTORY`: new `history` module (`rusty_fclone-cli` only, no
  core-crate change) backed by SQLite (`rusqlite`, `bundled` feature) — a
  summary of each completed scan (files/bytes scanned, duplicate
  groups/files, and any action's kind/applied/bytes-reclaimed/files-
  acted-on) is appended as one row when `--history <path>` is set, off by
  default. Deliberately scoped to per-scan summaries only, not per-file/
  per-group detail, and no query/report subcommand yet (both explicitly
  deferred, matching this project's established scoping pattern).
  ADR-0017, `CLI-UX-001` 0.2.0. Implemented, tested (fmt/clippy/test/bench
  all green, 66/66 tests — 4 new `history` unit tests + 2 new CLI-level
  tests), and manually smoke-tested (two real scans — plain, then
  `--action delete --apply` — produced two correctly-populated rows,
  confirmed via a direct SQL query). Merged via PR #13.

## In progress
- `RELEASE-BINARIES`: new `.github/workflows/release.yml`, triggered on
  `v*` tag pushes and manual `workflow_dispatch`, builds `rusty-fclone`
  for `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`,
  `x86_64-apple-darwin`, and `x86_64-pc-windows-msvc`, then uses
  `softprops/action-gh-release` to attach each platform's archive
  (binary + README + both license files) to the tag's GitHub Release
  without touching its title/body. ADR-0018. `cargo fmt`/`clippy`/`test`
  unaffected (workflow-only change, no Rust code touched). Not yet
  exercised by a real GitHub Actions run — the plan is to merge this,
  then manually dispatch the workflow against the existing `v0.1.0` tag
  to retroactively give that release its binaries, and verify the
  uploaded assets via the GitHub API afterward.

## Blocked
- None.

## Next
- Follow-on units intentionally left open by earlier scoping decisions
  (each needs its own design work before starting): `DETECTION-STREAMING-OVERLAP`
  proper (full pipeline overlap, needs a `ScanEvent` finality-contract
  decision first), `DETECTION-LINUX-FASTPATH` proper (io_uring/FIEMAP,
  needs an async runtime and unsafe FFI, its own ADR), and — if wanted —
  a query/report surface over `--history`'s accumulated data (explicitly
  out of scope for `CLI-SCAN-HISTORY` itself).

## Validation
- `cargo fmt --all --check`: pass (2026-08-24)
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass (2026-08-24)
- `cargo test --workspace`: pass, 66/66 (2026-08-24)
- `cargo bench -p rusty_fclone-core --no-run`: pass (2026-08-24)
- Manual CLI smoke tests across the project: verbosity flags, `RUST_LOG`
  override, default output silent on success, action dry runs, JSON
  format, progress checkpoints, confirmation prompt decline/accept,
  cold/warm `--cache` behavior, and now `--history` across two real scans
  (2026-08-24)

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
- `DETECTION-INCREMENTAL-CACHE`'s benchmark verification was inconclusive
  in this noisy environment (see above); its only invalidation signal is
  `(size, mtime)` — a file whose content changes without its mtime
  updating (contrived, or an unreliable filesystem) would be served a
  stale hash, the same trust model `make` and most incremental build
  tools accept.
- `CLI-SCAN-HISTORY`'s schema isn't versioned; a future incompatible
  change would need a migration story that doesn't exist yet. No query/
  report tooling exists yet either — reading history back is manual SQL
  or a future unit.
- `DETECTION-STREAMING-OVERLAP` proper needs a decision on how to relax or
  redesign `ScanEvent`'s "no group revision after emission" finality
  contract (ADR-0004) before it can be implemented — not yet made.
