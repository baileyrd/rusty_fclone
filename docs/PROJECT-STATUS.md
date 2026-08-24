# Project Status
- Last verified main commit: `ec36eaf` (PR #2, merged) — action-layer work below is on a new branch, not yet merged
- Verified at: 2026-08-24
- Current milestone: `ACTION-LAYER` (see `docs/roadmap/ROADMAP.md`)
- Health: green — workspace builds, lints, and tests clean on the pinned toolchain

## Completed
- `DETECTION-BASELINE`, `DETECTION-BENCHMARK`, `DETECTION-BENCHMARK-VS-FCLONES`,
  `DETECTION-ADAPTIVE-SAMPLE-SIZE`, `DETECTION-IO-THREAD-SIZING` — the full
  detection engine, benchmarked and tuned to beat or match fclones on all
  four synthetic scenarios. Merged to `main` via PR #1 and #2. See
  `docs/benchmarks/FCLONES-COMPARISON.md` for the numbers.
- `ACTION-LAYER` — delete/hardlink redundant copies, dry-run by default.
  New `rusty_fclone_core::action` module (`plan`/`apply`, ADR-0009) plus
  `--action <report|delete|hardlink>` and `--apply` CLI flags. Not yet
  merged — see "In progress" below.

  Key decisions (ADR-0009, `FCLONE-ACTION-001`): keeps the alphabetically-
  first path per group (no configurable strategy in v1); skips paths
  already sharing the kept file's inode (nothing to reclaim there);
  hardlink is implemented as link-to-temp-name-then-rename so a target path
  is never momentarily missing; `--action <kind>` alone only previews,
  `--apply` is a separate required flag to actually mutate the filesystem.
  Reflink support explicitly deferred (`ACTION-REFLINK`) — platform-specific,
  needs a new dependency or unsafe FFI.

  Evidence: 6 new unit tests in `action::tests` (core crate) covering plan
  correctness, hardlink-alias skipping, delete/hardlink apply (including
  verifying hardlinked files share an inode afterward), and per-file
  failure tolerance. 5 new unit tests in `main::tests` (CLI crate — its
  first test suite, required extracting a testable `run(cli: Cli) ->
  ExitCode` from `main`) covering dry-run-never-mutates, apply-actually-
  mutates (delete and hardlink), default-report-unchanged, and
  nonexistent-root rejection. Manual CLI smoke tests confirmed real output
  and a full delete/hardlink/re-scan cycle on disk. 36/36 tests pass
  workspace-wide (up from 31).

## In progress
- `ACTION-LAYER` work is implemented, tested, and locally validated
  (fmt/clippy/test all green) but not yet through the PR → CI → merge →
  sync loop this repo now follows for every unit of work.

## Blocked
- None.

## Next
1. Open a PR for the action-layer work, get CI green, merge, sync — same
   loop as the detection-engine work.
2. After that: `ACTION-REFLINK`, `CLI-UX` (JSON output, progress reporting,
   an interactive confirmation prompt as a second safety layer beyond
   `--apply`), `DETECTION-STREAMING-OVERLAP`, or `DETECTION-LINUX-FASTPATH`
   are the open roadmap units — no strong ordering constraint between them.

## Validation
- `cargo fmt --all --check`: pass (2026-08-24)
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass (2026-08-24)
- `cargo test --workspace`: pass, 36/36 (2026-08-24)
- Manual CLI smoke tests: `--action delete` dry-run (no mutation) then
  `--apply` (redundant copies removed, kept file survives); `--action
  hardlink --apply` (all paths survive, verified same inode via `ls -li`,
  re-scan afterward correctly reports 0 duplicate groups since they're now
  hardlink aliases); default (no `--action`) output confirmed unchanged
  from pre-action-layer behavior (2026-08-24)

## Risks and decisions needed
- The action layer is the first genuinely destructive capability in this
  codebase. Its safety model (dry-run default, two-flag confirmation) is
  documented and tested, but has not been used against a real, valuable
  directory tree outside this session's smoke tests — treat it with
  appropriate caution before pointing it at anything you care about.
- The `io_threads = cores` default (ADR-0008) remains empirically tuned on
  one 4-core container; unvalidated on real spinning disks or high-latency
  network filesystems.
