# Roadmap

| Unit | Outcome | Depends on | Specs | Exit gate | Status | Evidence |
|---|---|---|---|---|---|---|
| `DETECTION-BASELINE` | Working duplicate-detection engine + CLI, cross-platform | none | `FCLONE-DETECTION-001` | `cargo fmt`/`clippy`/`test` green; CLI finds duplicates on a real tree | Done | initial workspace commit |
| `DETECTION-BENCHMARK` | Criterion benchmark suite over synthetic trees (many small duplicates, many unique files, few large duplicates, a mixed realistic tree), reporting files/sec and bytes/sec | `DETECTION-BASELINE` | `FCLONE-DETECTION-001` | `cargo bench` exists, compiles in CI (`--no-run`), and runs manually with real numbers | Done | `crates/rusty_fclone-core/benches/detection.rs`; sample run recorded in `docs/PROJECT-STATUS.md` |
| `DETECTION-BENCHMARK-VS-FCLONES` | Head-to-head comparison against upstream fclones on the same synthetic trees, so "fastest possible" becomes a measured claim rather than an architectural intent | `DETECTION-BENCHMARK` | `FCLONE-DETECTION-001` | Documented comparison (methodology + numbers) against an installed fclones build | Done | `docs/benchmarks/FCLONES-COMPARISON.md`; `scripts/gen_bench_trees.py`, `scripts/bench-vs-fclones.sh` |
| `DETECTION-ADAPTIVE-SAMPLE-SIZE` | Decouple the partial-hash sample size from the small-file threshold so large files aren't over-sampled during pruning | `DETECTION-BENCHMARK-VS-FCLONES` | `FCLONE-DETECTION-001` (revision) | `few_large_duplicates` benchmark scenario matches or beats fclones | Done, but see evidence — this alone didn't move the target scenario | ADR-0007; new `partial_hash_sample_size` field + `--partial-hash-sample-size` flag. Re-measured after: no meaningful change on `few_large_duplicates` (every file there is a real duplicate, so nothing gets pruned regardless of sample size) — see `docs/benchmarks/FCLONES-COMPARISON.md` |
| `DETECTION-IO-THREAD-SIZING` | Size the I/O pool to core count instead of an oversubscribed multiple, after benchmarking showed oversubscription hurting throughput on every tested scenario | `DETECTION-ADAPTIVE-SAMPLE-SIZE` | `FCLONE-DETECTION-001` (revision) | `few_large_duplicates` benchmark scenario matches or beats fclones | Done | ADR-0008; `few_large_duplicates` moved from 1.21x slower to within noise of fclones' best config and beats its default; small-file scenarios improved from ~1.9-2.0x to ~2.6-2.7x faster. Exit gate met by this unit, not `DETECTION-ADAPTIVE-SAMPLE-SIZE` — see `docs/benchmarks/FCLONES-COMPARISON.md` |
| `DETECTION-STREAMING-OVERLAP` | Hashing begins before traversal finishes (full pipeline overlap), per ADR-0002/ADR-0004's noted scope cut | `DETECTION-BASELINE` | `FCLONE-DETECTION-001` (revision) | Benchmark shows reduced time-to-first-result on a large tree | Not Started | — |
| `DETECTION-LINUX-FASTPATH` | Optional Linux-specific I/O fast path (io_uring / `FIEMAP` extent ordering, and principled device-type-aware thread sizing rather than one guessed constant) behind a feature flag, without breaking the cross-platform default | `DETECTION-BENCHMARK` | new spec | Benchmark shows meaningful improvement on Linux; cross-platform build unaffected | Not Started | — |
| `ACTION-LAYER` | Delete/hardlink confirmed duplicates, safely | `DETECTION-BASELINE` | `FCLONE-ACTION-001` | Dry-run-by-default, two-flag confirmation, and tests for every destructive path | Done | ADR-0009; `crates/rusty_fclone-core/src/action.rs` (6 tests); `--action`/`--apply` CLI flags; manual smoke tests for delete, hardlink, and unchanged default (report) behavior |
| `ACTION-REFLINK` | Copy-on-write clone as a third `ActionKind`, alongside delete/hardlink | `ACTION-LAYER` | `FCLONE-ACTION-001` (revision) | Reflink works on at least one CFS filesystem (Btrfs/XFS/APFS) without breaking the cross-platform default elsewhere | Not Started | deferred by ADR-0009 — platform-specific, needs a new dependency or unsafe FFI |
| `CLI-UX` | Richer CLI output (JSON, progress reporting, interactive confirmation prompt for actions) | `DETECTION-BASELINE`, `ACTION-LAYER` | new spec | Documented output contract with tests | Not Started | — |

## Known gaps carried from the v1 baseline (not blocking, but tracked)

- No structured logging/progress observability yet (`tracing` or similar).
- No cycle-detection test for `--follow-symlinks`.
- Full-file hashing buffers the whole file in memory; large files aren't
  streamed through the hasher (ADR-0002 implementation note).
- Path storage is the naive `HashMap`-based model; prefix-compression
  deferred until benchmarked as necessary (ADR-0004).
