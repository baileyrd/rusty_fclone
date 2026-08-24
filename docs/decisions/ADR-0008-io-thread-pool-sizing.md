# ADR-0008: Size the I/O thread pool to core count, not an oversubscribed multiple

- Status: Accepted
- Date: 2026-08-24
- Related: ADR-0002 (original concurrency model), ADR-0007 (the fix that
  turned out not to be sufficient on its own), `docs/benchmarks/FCLONES-COMPARISON.md`

## Context

ADR-0002 sized the I/O pool at `cores * 4` (capped at 64) on the classic
rationale for blocking I/O: oversubscribe relative to core count so more
read requests are in flight than there are CPUs, hiding per-request
latency. This is well-established wisdom for high-latency storage
(spinning disks, network filesystems).

After ADR-0007's sample-size fix didn't close the `few_large_duplicates`
benchmark gap against fclones (see that ADR's honest correction), the
actual cause needed isolating. Sweeping `--io-threads` from 1 to 16 on that
scenario (4-core container) found threads = 4 was the clear optimum —
noticeably faster than both under-provisioning (1–2 threads) and every
oversubscribed value tested (8, 16):

| `--io-threads` | Mean time |
|---:|---:|
| 1 | 52.2 ms |
| 2 | 41.5 ms |
| **4** | **36.8 ms** |
| 8 | 44.6 ms |
| 16 (previous default: `cores * 4`) | 50.8 ms |

Checking whether this was specific to large files or a general effect: the
same sweep on all three small-file benchmark scenarios showed the *same*
pattern — 4 threads beat 8 and 16 in every single one. Oversubscription
wasn't just failing to help on this environment's storage, it was actively
worse everywhere tested.

## Decision

Change `ScanOptions::io_threads`'s default from `(cores * 4).min(64)` to
`cores` (via `std::thread::available_parallelism()`, unchanged). Keep
`--io-threads` as a CLI override (added alongside
`--partial-hash-sample-size` in the same change) for anyone whose storage
genuinely benefits from oversubscription — the *reasoning* behind ADR-0002's
original default isn't wrong for high-latency media, it just doesn't match
what this benchmark environment's backing storage actually behaves like.

## Consequences

- Re-measuring the full fclones comparison with both this change and
  ADR-0007's applied: rusty_fclone now beats fclones' own default
  configuration on `few_large_duplicates` (40.2 ms vs. 46.2 ms) and is
  within measurement noise of fclones' best (hash-matched) configuration
  (40.2 ms vs. 38.6 ms, relative 1.04 ± 0.15 — the confidence interval
  spans below 1.0). The small-file scenarios, which already won before this
  change, improved further (from ~1.9–2.0x faster to ~2.6–2.7x faster) since
  they were *also* hurt by oversubscription, just not enough to lose outright.
  Full numbers in `docs/benchmarks/FCLONES-COMPARISON.md`.
- This default is empirically tuned on one environment: a 4-core Linux
  container with (presumably) low-latency backing storage. It has not been
  validated on real spinning disks or high-latency network filesystems,
  where the original oversubscription rationale may still hold. The
  `--io-threads` escape hatch exists precisely for that case — this ADR
  changes the *default*, not the ceiling on what's configurable.
- `DETECTION-LINUX-FASTPATH` (device-type-aware I/O tuning, still on the
  roadmap) would be the principled long-term fix — detecting whether
  storage is latency-bound or not, rather than guessing with one constant
  default either way.
