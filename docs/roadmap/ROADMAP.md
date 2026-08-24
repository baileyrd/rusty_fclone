# Roadmap

| Unit | Outcome | Depends on | Specs | Exit gate | Status | Evidence |
|---|---|---|---|---|---|---|
| `DETECTION-BASELINE` | Working duplicate-detection engine + CLI, cross-platform | none | `FCLONE-DETECTION-001` | `cargo fmt`/`clippy`/`test` green; CLI finds duplicates on a real tree | Done | initial workspace commit |
| `DETECTION-BENCHMARK` | Benchmark suite proving detection speed against a synthetic large tree (and ideally against fclones) | `DETECTION-BASELINE` | `FCLONE-DETECTION-001` | `cargo bench` exists and runs in CI or documented manual step | Not Started | — |
| `DETECTION-STREAMING-OVERLAP` | Hashing begins before traversal finishes (full pipeline overlap), per ADR-0002/ADR-0004's noted scope cut | `DETECTION-BASELINE` | `FCLONE-DETECTION-001` (revision) | Benchmark shows reduced time-to-first-result on a large tree | Not Started | — |
| `DETECTION-LINUX-FASTPATH` | Optional Linux-specific I/O fast path (io_uring / `FIEMAP` extent ordering) behind a feature flag, without breaking the cross-platform default | `DETECTION-BENCHMARK` | new spec | Benchmark shows meaningful improvement on Linux; cross-platform build unaffected | Not Started | — |
| `ACTION-LAYER` | Delete/hardlink/reflink confirmed duplicates, safely | `DETECTION-BASELINE` | new spec | Dry-run mode, confirmation flow, and tests for every destructive path | Not Started | — |
| `CLI-UX` | Richer CLI output (JSON, progress reporting, `--dry-run`) | `DETECTION-BASELINE` | new spec | Documented output contract with tests | Not Started | — |

## Known gaps carried from the v1 baseline (not blocking, but tracked)

- No structured logging/progress observability yet (`tracing` or similar).
- No cycle-detection test for `--follow-symlinks`.
- Full-file hashing buffers the whole file in memory; large files aren't
  streamed through the hasher (ADR-0002 implementation note).
- Path storage is the naive `HashMap`-based model; prefix-compression
  deferred until benchmarked as necessary (ADR-0004).
