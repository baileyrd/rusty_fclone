# Project Status
- Last verified main commit: none yet — `main` has no commits; this branch (`claude/custom-fclone-detection-bufv7b`) holds the baseline, gap-closure, benchmark-suite, and fclones-comparison follow-ups, not yet merged
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
- `DETECTION-BENCHMARK-VS-FCLONES` — documented head-to-head comparison
  against upstream fclones 0.35.0 on identical synthetic trees; see
  `docs/benchmarks/FCLONES-COMPARISON.md` for full methodology.
  **rusty_fclone wins ~1.9–2.0x on small-file-heavy trees (including the
  realistic mixed-tree scenario) but loses ~1.2x on a large-file
  scenario** — an honest, mixed result, not an unqualified win. The
  large-file loss is attributed (not yet root-caused by profiling) to
  ADR-0001's shared 128 KiB constant over-sampling large files during the
  partial-hash stage; tracked as the new `DETECTION-ADAPTIVE-SAMPLE-SIZE`
  roadmap unit. Setting this up also caught and fixed a real bug in the
  benchmark fixtures themselves (see spec change history 0.1.3): the
  "unique files" scenario's content generator collided every 256 files, so
  it was silently testing ~256 duplicate groups instead of zero.

  In-process Criterion numbers (this session's 4-core container;
  informational, not a portability claim), after the fixture fix:

  | Scenario | Time (mean) | Throughput |
  |---|---|---|
  | `many_small_duplicates` (2,000 files, 1 KiB, 200 dup groups of 10) | 34.5 ms | ~58 Kelem/s |
  | `many_unique_small_files` (2,000 files, 1 KiB, no duplicates) | 39.3 ms | ~51 Kelem/s |
  | `few_large_duplicates` (20 files, 8 MiB, 4 dup groups of 5) | 24.1 ms | ~6.5 GiB/s (warm page cache) |
  | `mixed_realistic_tree` (1,018 files, mostly unique, 3 small dup groups) | 20.8 ms | ~49 Kelem/s |

  CLI-to-CLI comparison against fclones (same container, same trees; full
  table in `docs/benchmarks/FCLONES-COMPARISON.md`):

  | Scenario | rusty-fclone | fclones (default) | fclones (xxhash) |
  |---|---:|---:|---:|
  | `many_small_duplicates` | **42.6 ms** | 81.6 ms | 80.2 ms |
  | `many_unique_small_files` | **39.5 ms** | 78.6 ms | 77.5 ms |
  | `few_large_duplicates` | 53.7 ms | 46.9 ms | **44.2 ms** |
  | `mixed_realistic_tree` | **27.3 ms** | 45.4 ms | 44.7 ms |

## In progress
- None. Awaiting review/merge of the baseline + gap-closure + benchmark +
  comparison work.

## Blocked
- None.

## Next
1. `DETECTION-ADAPTIVE-SAMPLE-SIZE` — the fclones comparison found a real,
   reproducible loss on large files; this is the natural next unit to close
   it. `DETECTION-STREAMING-OVERLAP` is the alternative if that's not the
   priority.

## Validation
- `cargo fmt --all --check`: pass (2026-08-24)
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass (2026-08-24)
- `cargo test --workspace`: pass, 25/25 (2026-08-24)
- `cargo bench -p rusty_fclone-core`: runs to completion, all 4 scenarios
  produce sane throughput numbers, no panics (2026-08-24; numbers above)
- `scripts/bench-vs-fclones.sh` (fclones 0.35.0 via `cargo binstall`,
  hyperfine, 3 warmup + 10+ measured runs per command): completed for all 4
  scenarios, results above and in `docs/benchmarks/FCLONES-COMPARISON.md`
  (2026-08-24)
- Manual CLI smoke tests (`rusty-fclone` against temp trees, with and
  without `--verify`): correct duplicate groups reported, hardlink aliases
  included, unique files excluded, `--verify`/`--small-file-threshold`
  flags wired through correctly (2026-08-24)

## Risks and decisions needed
- "Fastest possible" is now a measured claim, but the honest reading is
  "faster on small-file-heavy trees, slower on large-file trees" — not an
  unqualified win. `DETECTION-ADAPTIVE-SAMPLE-SIZE` is the proposed fix;
  it hasn't been root-caused by profiling yet, only motivated by the
  benchmark result.
- No CI workflow has run yet (the workflow file is new on this branch); its
  first real run should be treated as unverified until observed green.
