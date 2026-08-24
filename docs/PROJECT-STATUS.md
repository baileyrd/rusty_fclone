# Project Status
- Last verified main commit: `8d36c62` (PR #4, merged) — tracing/observability
  work below is on a new branch, not yet merged
- Verified at: 2026-08-24
- Current milestone: closing out the roadmap's "Not Started" units and known
  gaps (see `docs/roadmap/ROADMAP.md`)
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

## In progress
- Structured observability (`tracing`): spans on `traverse`, `run_scan`,
  `process_size_group`; leveled events at stage boundaries and every
  per-file error path; CLI wires up `tracing-subscriber` on stderr with a
  repeated `-v`/`--verbose` flag (`RUST_LOG` always takes precedence).
  ADR-0010. Implemented, tested (fmt/clippy/test/bench all green, 42/42
  tests), and manually smoke-tested (`-v`, `-vv`, default-silent, and
  `RUST_LOG` override all confirmed). Not yet through the PR → CI → merge →
  sync loop.

## Blocked
- None.

## Next
1. Open a PR for the tracing/observability work, get CI green, merge, sync.
2. Remaining open roadmap units, no strong ordering constraint between
   them: path storage (`Arc<Path>`), `DETECTION-STREAMING-OVERLAP` (scoped
   to merging traversal/collapse/size-grouping into one streaming pass, not
   full hash-before-traversal-completes overlap), `DETECTION-LINUX-FASTPATH`
   (scoped to rotational-vs-SSD-aware `io_threads` sizing, not
   io_uring/FIEMAP), `ACTION-REFLINK`, `CLI-UX` (JSON output, progress
   reporting, an interactive confirmation prompt as a second safety layer
   beyond `--apply`).

## Validation
- `cargo fmt --all --check`: pass (2026-08-24)
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass (2026-08-24)
- `cargo test --workspace`: pass, 42/42 (2026-08-24)
- `cargo bench -p rusty_fclone-core --no-run`: pass (2026-08-24)
- Manual CLI smoke tests: verbosity flags (`-v` info, `-vv` debug),
  `RUST_LOG` override taking precedence, default output silent on success
  (2026-08-24)

## Risks and decisions needed
- The action layer is the first genuinely destructive capability in this
  codebase. Its safety model (dry-run default, two-flag confirmation) is
  documented and tested, but has not been used against a real, valuable
  directory tree outside this session's smoke tests — treat it with
  appropriate caution before pointing it at anything you care about.
- The `io_threads = cores` default (ADR-0008) remains empirically tuned on
  one 4-core container; unvalidated on real spinning disks or high-latency
  network filesystems. `DETECTION-LINUX-FASTPATH` will make this
  device-aware, but hasn't started yet.
