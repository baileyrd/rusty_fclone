# Project Status
- Last verified main commit: none yet — `main` has no commits; this branch (`claude/custom-fclone-detection-bufv7b`) holds the baseline, gap-closure, and benchmark-suite follow-ups, not yet merged
- Verified at: 2026-08-24
- Current milestone: `DETECTION-BASELINE` (see `docs/roadmap/ROADMAP.md`)
- Health: green — workspace builds, lints, and tests clean on the pinned toolchain

## Completed
- `DETECTION-BASELINE` — detection engine + CLI implemented on this branch,
  not yet merged; evidence: `cargo fmt --all --check`, `cargo clippy
  --workspace --all-targets --all-features -- -D warnings`, and `cargo test
  --workspace` all pass locally (25/25 tests); manual CLI smoke tests
  against directory trees with exact duplicates, unique files, and
  pre-existing hardlinks confirmed correct output, including with
  `--verify`.
- Traceability gap-closure — every requirement previously flagged "needs
  dedicated unit test" (`FCLONE-DETECTION-001-FR-005`, `FR-006`, `FR-008`,
  `FR-009`, `NFR-001`) now has one; see
  `docs/traceability/TRACEABILITY.md`. Closing FR-009 surfaced and fixed a
  real gap: read failures during hashing/verification were silently
  dropped rather than reported — see
  `docs/specifications/detection/FCLONE-DETECTION-001.md` change history
  (0.1.1).
- `DETECTION-BENCHMARK` — Criterion suite added
  (`crates/rusty_fclone-core/benches/detection.rs`, `cargo bench -p
  rusty_fclone-core`), covering four synthetic scenarios. CI compiles it
  (`cargo bench --workspace --no-run`) on every push; full statistical runs
  are a documented manual step (too slow/variance-prone for per-PR CI).
  Sample run on this session's 4-core container (informational only — not a
  portability claim, and not yet compared against fclones; see
  `DETECTION-BENCHMARK-VS-FCLONES` on the roadmap):

  | Scenario | Time (mean) | Throughput |
  |---|---|---|
  | `many_small_duplicates` (2,000 files, 1 KiB, 200 dup groups of 10) | 35.7 ms | ~56 Kelem/s |
  | `many_unique_small_files` (2,000 files, 1 KiB, no duplicates) | 37.1 ms | ~54 Kelem/s |
  | `few_large_duplicates` (20 files, 8 MiB, 4 dup groups of 5) | 23.4 ms | ~6.7 GiB/s (warm page cache — see spec's open questions) |
  | `mixed_realistic_tree` (1,018 files, mostly unique, 3 small dup groups) | 20.3 ms | ~50 Kelem/s |

## In progress
- None. Awaiting review/merge of the baseline + gap-closure + benchmark work.

## Blocked
- None.

## Next
1. `DETECTION-BENCHMARK-VS-FCLONES` — install/build upstream fclones and run
   the same synthetic trees through it for a real comparative number, or
   `DETECTION-STREAMING-OVERLAP` if a comparison isn't wanted next.

## Validation
- `cargo fmt --all --check`: pass (2026-08-24)
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass (2026-08-24)
- `cargo test --workspace`: pass, 25/25 (2026-08-24)
- `cargo bench -p rusty_fclone-core`: runs to completion, all 4 scenarios
  produce sane throughput numbers, no panics (2026-08-24; numbers above)
- Manual CLI smoke tests (`rusty-fclone` against temp trees, with and
  without `--verify`): correct duplicate groups reported, hardlink aliases
  included, unique files excluded, `--verify`/`--small-file-threshold`
  flags wired through correctly (2026-08-24)

## Risks and decisions needed
- The benchmark suite is relative/regression-only so far — no comparison
  against fclones exists yet, so "fastest possible" is a measured number for
  this crate's own throughput but still an architectural intent for the
  comparative claim. See `DETECTION-BENCHMARK-VS-FCLONES` on the roadmap.
- No CI workflow has run yet (the workflow file is new on this branch); its
  first real run should be treated as unverified until observed green.
