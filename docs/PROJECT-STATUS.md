# Project Status
- Last verified main commit: `487d442` (PR #1, merged)
- Verified at: 2026-08-24
- Current milestone: `DETECTION-BASELINE` (see `docs/roadmap/ROADMAP.md`) — complete
- Health: green — CI passed on PR #1's head (`c4b749a`) before merge; workspace builds, lints, and tests clean on the pinned toolchain

## Completed
- `DETECTION-BASELINE` — detection engine + CLI. Merged to `main` via
  [PR #1](https://github.com/baileyrd/rusty_fclone/pull/1); evidence:
  `cargo fmt --all --check`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, and `cargo test --workspace` all pass
  (25/25 tests), CI green on the merged head; manual CLI smoke tests
  against directory trees with exact duplicates, unique files, and
  pre-existing hardlinks confirmed correct output, including with
  `--verify`.
- Traceability gap-closure — every requirement previously flagged "needs
  dedicated unit test" now has one; see `docs/traceability/TRACEABILITY.md`.
  Closing FR-009 surfaced and fixed a real gap: read failures during
  hashing/verification were silently dropped rather than reported.
- `DETECTION-BENCHMARK` — Criterion suite added
  (`crates/rusty_fclone-core/benches/detection.rs`, `cargo bench -p
  rusty_fclone-core`), covering four synthetic scenarios. CI compiles it
  (`cargo bench --workspace --no-run`) on every push; full statistical runs
  are a documented manual step (too slow/variance-prone for per-PR CI).
- `DETECTION-BENCHMARK-VS-FCLONES` + `DETECTION-ADAPTIVE-SAMPLE-SIZE` +
  `DETECTION-IO-THREAD-SIZING` — documented head-to-head comparison against
  upstream fclones 0.35.0, then two follow-on fixes to close the gap it
  found. Full investigation and final numbers in
  `docs/benchmarks/FCLONES-COMPARISON.md`; the short version:

  **rusty_fclone now wins ~2.6–2.7x on small-file-heavy trees (including
  the realistic mixed-tree scenario) and is within measurement noise of
  fclones' best-tuned configuration on a large-file scenario, beating its
  default configuration outright.** Getting there took an honest wrong turn:
  the first hypothesis (ADR-0007, decoupling the partial-hash sample size
  from the small-file threshold) was a real improvement but didn't move the
  benchmark that motivated it, since every file in that scenario is a real
  duplicate and nothing gets pruned by partial hashing regardless of sample
  size. The actual fix (ADR-0008) was the I/O thread pool's default —
  changed from an oversubscribed `cores * 4` to plain `cores`, after
  benchmarking showed oversubscription hurting throughput on *every* tested
  scenario, not just the large-file one. Both changes are kept; both are
  documented, including the one that didn't work as expected.

  Setting up the comparison also caught a real bug in the benchmark
  fixtures themselves: the "unique files" scenario's content generator
  collided every 256 files, so it was silently testing ~256 duplicate
  groups instead of zero (fixed; benchmark-only, no production code
  affected).

  CLI-to-CLI comparison against fclones (this session's 4-core container,
  same trees; full tables in `docs/benchmarks/FCLONES-COMPARISON.md`):

  | Scenario | rusty-fclone | fclones (default) | fclones (xxhash) |
  |---|---:|---:|---:|
  | `many_small_duplicates` | **32.2 ms** | 84.3 ms | 85.4 ms |
  | `many_unique_small_files` | **31.3 ms** | 82.7 ms | 84.7 ms |
  | `few_large_duplicates` | 40.2 ms | 46.2 ms | **38.6 ms** |
  | `mixed_realistic_tree` | **17.1 ms** | 44.4 ms | 47.7 ms |

## In progress
- None.

## Blocked
- None.

## Next
1. `DETECTION-STREAMING-OVERLAP` (hashing starts before traversal finishes)
   or `DETECTION-LINUX-FASTPATH` (principled device-type-aware I/O tuning,
   superseding the single empirically-chosen `io_threads` default with
   something that detects what it's running on) are the natural next units.
   `ACTION-LAYER` (delete/hardlink/reflink) is the alternative if detection
   performance work is done for now.

## Validation
- `cargo fmt --all --check`: pass (2026-08-24)
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass (2026-08-24)
- `cargo test --workspace`: pass, 25/25 (2026-08-24)
- `cargo bench -p rusty_fclone-core`: runs to completion, all 4 scenarios
  produce sane throughput numbers, no panics (2026-08-24)
- `scripts/bench-vs-fclones.sh` (fclones 0.35.0 via `cargo binstall`,
  hyperfine, 3 warmup + 10+ measured runs per command): completed for all 4
  scenarios both before and after the ADR-0007/ADR-0008 fixes; results
  above and in `docs/benchmarks/FCLONES-COMPARISON.md` (2026-08-24)
- Manual `--io-threads` sweep (1/2/4/8/16) on all 4 scenarios: confirmed
  `io_threads = cores` (4 on this container) beats every other value
  tested, on every scenario (2026-08-24)
- Manual CLI smoke tests (`rusty-fclone` against temp trees, with and
  without `--verify`): correct duplicate groups reported, hardlink aliases
  included, unique files excluded, all six `ScanOptions` flags wired
  through correctly (2026-08-24)

## Risks and decisions needed
- The `io_threads = cores` default (ADR-0008) is empirically tuned on one
  4-core container with presumably low-latency backing storage. It hasn't
  been validated on real spinning disks or high-latency network
  filesystems, where oversubscription's original rationale (ADR-0002) may
  still hold — `--io-threads` exists as an override for that case, but no
  such environment has actually been tested.
- None currently. CI's first real run (PR #1) was observed green before merge.
