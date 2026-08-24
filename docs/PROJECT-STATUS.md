# Project Status
- Last verified main commit: `4d320a6` (PR #7, merged) — device-aware
  io_threads sizing and reflink action work below are on branches stacked
  atop each other, not yet merged
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
- Path storage: `Arc<Path>` instead of `PathBuf` for every path carried
  through the detection pipeline (`Candidate.path`, `FileGroup`,
  `FileError.path`, `DuplicateGroup.paths`), so cloning a path across the
  hardlink-collapse/size/partial-hash/full-hash grouping stages is a
  refcount bump instead of a fresh allocation and copy. ADR-0011. Merged
  via PR #6.
- `DETECTION-TRAVERSAL-COLLAPSE-FUSION`: traversal and hardlink-collapse
  now run as one streaming pass — `traversal::traverse` takes an
  `on_candidate` callback instead of returning a `Vec<Candidate>`, and
  `pipeline::run_scan` folds the collapse step directly into that
  callback. ADR-0012. Merged via PR #7. (Deliberately not full
  hash-before-traversal-completes overlap — that's kept open separately
  as `DETECTION-STREAMING-OVERLAP` proper, needing `ScanEvent`'s finality
  contract redesigned first.)

## In progress
- `DETECTION-DEVICE-AWARE-IO-SIZING` (the thread-sizing half of
  `DETECTION-LINUX-FASTPATH`): new `device::default_io_threads` picks an
  oversubscribed pool on a rotational disk (Linux-only, best-effort via
  `/proc/self/mountinfo` + `/sys/dev/block/*/queue/rotational`) or plain
  `cores` otherwise/on failure. `ScanOptions::io_threads` and the CLI's
  `--io-threads` flag change from `usize` to `Option<usize>` (`None` =
  auto-detect at scan time, `Some(n)` pins it explicitly). ADR-0013.
  io_uring/`FIEMAP` extent ordering remains separately deferred, kept open
  as `DETECTION-LINUX-FASTPATH` proper. PR #8 open, awaiting CI.
- `ACTION-REFLINK`: new `ActionKind::Reflink` via the `reflink-copy` crate
  (strict `reflink`, not `reflink_or_copy` — no silent copy fallback when
  a filesystem doesn't support cloning), same temp-then-rename safety
  pattern as `Hardlink`, with cleanup of the temp file on a failed clone.
  ADR-0014, `FCLONE-ACTION-001` 0.2.0. Stacked on the device-aware-io-
  sizing branch; implemented, tested (fmt/clippy/test/bench all green,
  47/47 tests), and manually smoke-tested — this environment's filesystem
  isn't CoW-capable, so the smoke test (and the tolerant unit test)
  exercised the graceful-failure path: reported per-file error, zero
  bytes reclaimed, both files left with correct unmodified content, no
  stray temp file. The success path is implemented and delegated to
  `reflink-copy`'s own platform code, not independently verified on real
  CoW storage here. Not yet pushed through the PR → CI → merge → sync loop.

## Blocked
- None.

## Next
1. Get CI green and merge PR #8 (device-aware io_threads sizing), sync
   main.
2. Rebase/PR the reflink-action branch onto the updated main, get CI
   green, merge, sync.
3. Remaining open roadmap units, no strong ordering constraint between
   them: `DETECTION-STREAMING-OVERLAP` proper (full pipeline overlap,
   needs a finality-contract decision first), `DETECTION-LINUX-FASTPATH`
   proper (io_uring/FIEMAP, needs an async runtime and unsafe FFI, its own
   ADR), `CLI-UX` (JSON output, progress reporting, an interactive
   confirmation prompt as a second safety layer beyond `--apply`).

## Validation
- `cargo fmt --all --check`: pass (2026-08-24)
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass (2026-08-24)
- `cargo test --workspace`: pass, 47/47 (2026-08-24)
- `cargo bench -p rusty_fclone-core --no-run`: pass (2026-08-24)
- Manual CLI smoke tests: verbosity flags (`-v` info, `-vv` debug),
  `RUST_LOG` override taking precedence, default output silent on success,
  `--action delete` dry run against a real duplicate pair, auto-detected
  vs. explicit `--io-threads`, `--action reflink --apply` (graceful
  per-file failure on this non-CoW filesystem, confirmed clean) (2026-08-24)

## Risks and decisions needed
- The action layer is the first genuinely destructive capability in this
  codebase. Its safety model (dry-run default, two-flag confirmation) is
  documented and tested, but has not been used against a real, valuable
  directory tree outside this session's smoke tests — treat it with
  appropriate caution before pointing it at anything you care about.
- `DETECTION-DEVICE-AWARE-IO-SIZING`'s rotational-disk detection logic is
  unit-tested against synthetic `/proc/self/mountinfo` input but has not
  been exercised against a real spinning disk (this environment's storage
  resolves to the safe `cores` fallback) — behavior on real rotational
  media is unverified beyond the parsing logic itself.
- `ACTION-REFLINK`'s success path (an actual CoW clone happening) is
  unverified end-to-end in this environment for the same reason — no
  CoW-capable filesystem available to test against. Trusted to the
  `reflink-copy` dependency's own test coverage.
- `DETECTION-STREAMING-OVERLAP` proper (full hash-before-traversal-
  completes overlap) needs a decision on how to relax or redesign
  `ScanEvent`'s "no group revision after emission" finality contract
  (ADR-0004) before it can be implemented — not yet made.
