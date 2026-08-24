# Project Status
- Last verified main commit: `9f51074` (PR #5, merged) — Arc<Path> and
  traversal/collapse-fusion work below are on branches stacked atop each
  other, not yet merged
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
- Structured observability (`tracing`): spans on `traverse`, `run_scan`,
  `process_size_group`; leveled events at stage boundaries and every
  per-file error path; CLI wires up `tracing-subscriber` on stderr with a
  repeated `-v`/`--verbose` flag (`RUST_LOG` always takes precedence).
  ADR-0010. Merged via PR #5.

## In progress
- Path storage: `Arc<Path>` instead of `PathBuf` for every path carried
  through the detection pipeline (`Candidate.path`, `FileGroup`,
  `FileError.path`, `DuplicateGroup.paths`), so cloning a path across the
  hardlink-collapse/size/partial-hash/full-hash grouping stages is a
  refcount bump instead of a fresh allocation and copy. ADR-0011.
  Implemented, tested (fmt/clippy/test/bench all green, 42/42 tests), and
  manually smoke-tested against a real `--action delete` dry run. PR #6
  open, awaiting CI.
- `DETECTION-TRAVERSAL-COLLAPSE-FUSION` (a narrower, already-done step
  toward the roadmap's `DETECTION-STREAMING-OVERLAP`): traversal and
  hardlink-collapse now run as one streaming pass — `traversal::traverse`
  takes an `on_candidate` callback instead of returning a `Vec<Candidate>`,
  and `pipeline::run_scan` folds the collapse step directly into that
  callback, removing a full extra pass and the intermediate `Vec`. ADR-0012.
  Deliberately does *not* implement full hash-before-traversal-completes
  overlap — that needs `ScanEvent`'s finality contract (ADR-0004)
  redesigned first, kept open as `DETECTION-STREAMING-OVERLAP` proper.
  Stacked on the `Arc<Path>` branch; implemented, tested (fmt/clippy/test/
  bench all green, 42/42 tests), and manually smoke-tested. Not yet pushed
  through the PR → CI → merge → sync loop.

## Blocked
- None.

## Next
1. Get CI green and merge PR #6 (`Arc<Path>` path storage), sync main.
2. Rebase/PR the traversal-collapse-fusion branch onto the updated main,
   get CI green, merge, sync.
3. Remaining open roadmap units, no strong ordering constraint between
   them: `DETECTION-STREAMING-OVERLAP` proper (full pipeline overlap,
   needs a finality-contract decision first), `DETECTION-LINUX-FASTPATH`
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
  `RUST_LOG` override taking precedence, default output silent on success,
  `--action delete` dry run against a real duplicate pair (2026-08-24)

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
- `DETECTION-STREAMING-OVERLAP` proper (full hash-before-traversal-
  completes overlap) needs a decision on how to relax or redesign
  `ScanEvent`'s "no group revision after emission" finality contract
  (ADR-0004) before it can be implemented — not yet made.
