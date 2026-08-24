# Project Status
- Last verified main commit: `a77f1c4` (PR #10, merged)
- Verified at: 2026-08-24
- Current milestone: none active — the originally scoped "build it all and
  close all gaps" batch (roadmap's then-"Not Started" units + all known
  gaps) is fully merged to `main` and synced. See `docs/roadmap/ROADMAP.md`
  for what's newly tracked as follow-on work.
- Health: green — workspace builds, lints, and tests clean on the pinned
  toolchain, verified directly on synced `main`

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
- `DETECTION-DEVICE-AWARE-IO-SIZING` (the thread-sizing half of
  `DETECTION-LINUX-FASTPATH`): new `device::default_io_threads` picks an
  oversubscribed pool on a rotational disk (Linux-only, best-effort via
  `/proc/self/mountinfo` + `/sys/dev/block/*/queue/rotational`) or plain
  `cores` otherwise/on failure. `ScanOptions::io_threads`/CLI
  `--io-threads` change from `usize` to `Option<usize>`. ADR-0013. Merged
  via PR #8. (io_uring/`FIEMAP` extent ordering remains separately
  deferred, kept open as `DETECTION-LINUX-FASTPATH` proper.)
- `ACTION-REFLINK`: new `ActionKind::Reflink` via the `reflink-copy` crate
  (strict `reflink`, not `reflink_or_copy` — no silent copy fallback when
  a filesystem doesn't support cloning), same temp-then-rename safety
  pattern as `Hardlink`, with cleanup of the temp file on a failed clone.
  ADR-0014, `FCLONE-ACTION-001` 0.2.0. Merged via PR #9. This
  environment's filesystem isn't CoW-capable, so testing here (unit +
  manual smoke) only exercised the graceful-failure path; the success
  path is delegated to `reflink-copy`'s own platform code, not
  independently verified in this environment.
- `CLI-UX`: `--format text|json` (NDJSON: `duplicate_group` with a nested
  `action` object, `error`, `progress`, `finished`, `action_summary`); new
  `rusty_fclone_core::ScanEvent::Progress(ScanProgress)`, a cumulative
  traversal checkpoint emitted every 256 files, rendered as a live
  in-place-updating stderr line in text mode (only when stderr is a real
  terminal, via `std::io::IsTerminal` — confirmed silent when piped);
  `-y`/`--yes`-bypassable confirmation prompt before `--apply` mutates
  anything (a general warning, not exact totals — those aren't known until
  the streaming scan finishes). ADR-0015, new `CLI-UX-001` spec. Merged
  via PR #10 — the last unit in this batch.

## In progress
- None.

## Blocked
- None.

## Next
Follow-on units intentionally left open by earlier scoping decisions
(deliberately out of this batch's scope, each needs its own design work
before starting):
- `DETECTION-STREAMING-OVERLAP` proper — full hash-before-traversal-
  completes pipeline overlap. Needs a decision on how to relax or
  redesign `ScanEvent`'s "no group revision after emission" finality
  contract (ADR-0004) first.
- `DETECTION-LINUX-FASTPATH` proper — io_uring/`FIEMAP` extent-ordered
  reads. Needs an async runtime and unsafe FFI, and its own ADR; the
  thread-sizing half of the original roadmap unit is already done
  (`DETECTION-DEVICE-AWARE-IO-SIZING`, ADR-0013).

## Validation
- `cargo fmt --all --check`: pass (2026-08-24, on synced `main` @ `a77f1c4`)
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass (2026-08-24, on synced `main`)
- `cargo test --workspace`: pass, 51/51 (2026-08-24, on synced `main`)
- `cargo bench -p rusty_fclone-core --no-run`: pass (2026-08-24, on synced `main`)
- Manual CLI smoke tests across the batch: verbosity flags, `RUST_LOG`
  override, default output silent on success, `--action delete` dry run,
  auto-detected vs. explicit `--io-threads`, `--action reflink --apply`
  graceful failure, `--format json` (plain and action-annotated), progress
  checkpoints (present in JSON mode, silent in piped text mode),
  confirmation prompt decline and accept (2026-08-24)

## Risks and decisions needed
- The action layer is the first genuinely destructive capability in this
  codebase. Its safety model (dry-run default, two-flag confirmation, plus
  the interactive confirmation prompt) is documented and tested, but has
  not been used against a real, valuable directory tree outside this
  session's smoke tests — treat it with appropriate caution before
  pointing it at anything you care about.
- `DETECTION-DEVICE-AWARE-IO-SIZING`'s rotational-disk detection logic is
  unit-tested against synthetic `/proc/self/mountinfo` input but has not
  been exercised against a real spinning disk (this environment's storage
  resolves to the safe `cores` fallback) — behavior on real rotational
  media is unverified beyond the parsing logic itself.
- `ACTION-REFLINK`'s success path (an actual CoW clone happening) is
  unverified end-to-end in this environment for the same reason — no
  CoW-capable filesystem available to test against. Trusted to the
  `reflink-copy` dependency's own test coverage.
- `CLI-UX-001`'s JSON schema isn't versioned or promised stable yet, and
  no automated test asserts on its exact shape (only that `--format json`
  runs successfully) — see the spec's open questions.
- `DETECTION-STREAMING-OVERLAP` proper (full hash-before-traversal-
  completes overlap) needs a decision on how to relax or redesign
  `ScanEvent`'s "no group revision after emission" finality contract
  (ADR-0004) before it can be implemented — not yet made.
