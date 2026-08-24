# ADR-0002: Cross-platform I/O scope and two-pool concurrency model

- Status: Accepted
- Date: 2026-08-24

## Context

Two related questions shape the engine's runtime architecture:

1. **Platform scope for v1's I/O layer.** A Linux-tuned engine (io_uring,
   `FIEMAP`-based extent-ordered reads on spinning disks, reflink-aware
   backends) is meaningfully faster on the hardware it targets, but leaves v1
   as a Linux-only tool and requires a separately designed portable fallback
   later.
2. **How I/O-bound and CPU-bound work share threads.** Directory traversal
   and file reads are latency-bound (blocking `read()` benefits from *more*
   threads than cores, to keep the OS request queue full); hashing is
   CPU-bound (benefits from being capped at core count). Mixing both kinds of
   work in one undifferentiated pool means hash-heavy threads starve I/O
   throughput and vice versa.

## Decision

- **Platform scope**: cross-platform from day one, on a portable
  blocking-thread-pool I/O model (`std::fs` + explicit worker threads). No
  io_uring, `FIEMAP`, or other Linux-specific fast path in v1.
- **Concurrency model**: two separate thread pools connected by bounded
  channels (`crossbeam-channel`):
  - An **I/O pool** (`rusty_fclone_core::io_pool::IoPool`) — a small,
    hand-rolled fixed-size pool of `std::thread` workers, oversubscribed
    relative to core count (default: `cores * 4`, capped at 64), each
    blocking on `read`/`seek` calls.
  - The **CPU pool** — rayon's global pool, capped at core count by default,
    used for hashing and per-group orchestration (`into_par_iter()` over
    size-groups and over group members).
- Directory traversal uses jwalk, which parallelizes the walk itself via its
  own rayon-backed mechanism — no additional traversal-specific pool needed.

## Implementation note (discovered while building v1)

The original design discussion (see `docs/specifications/detection/`)
considered shipping raw bytes from the I/O pool to a separate hashing step on
the CPU pool for *every* hash, to keep hash computation strictly off I/O
threads. In practice, xxh3 is cheap enough (multiple GB/s, generally faster
than disk throughput) that this round-trip buys nothing for the *streaming
full-file* case: it would mean either buffering an entire large file just to
hand it across a channel, or building a chunked streaming-hash protocol for
marginal benefit. v1 instead has the I/O pool return the bytes it read (full
file or partial-hash ranges), and hash computation happens in the *calling*
rayon thread — which is correct, because in v1's architecture every I/O read
is issued from inside a `rayon::into_par_iter()` closure (per size-group,
per group member), so the hash still executes on the CPU pool. The two pools
remain genuinely separate; only the "which thread hashes" question resolved
differently than first discussed. Streaming (non-buffering) full-file
hashing for very large files is a documented roadmap item, not a scope
failure — see `docs/roadmap/ROADMAP.md`.

## Consequences

- v1 leaves real throughput on the table on Linux relative to a tuned
  io_uring/extent-ordered implementation. That gap is intentional and
  revisitable — see the roadmap.
- The I/O pool blocks its calling rayon thread on a channel `recv()` while
  waiting for a read to complete. This is an accepted compromise for v1: it
  is waiting on a result, not on the syscall itself, and keeps the
  implementation simple. Revisit if profiling shows rayon thread starvation
  under real workloads.
- Full-file hashing buffers the entire file in memory before hashing. Fine
  for the common case; very large files (multi-GB) are a known follow-up.

## Addendum (2026-08-24): oversubscription default revised

The I/O pool's oversubscription factor described above (`cores * 4`) was
the *default* sizing when this ADR was written. Benchmark evidence against
fclones showed that default actively hurting throughput on the environment
tested — see ADR-0008, which changes the default to `cores` (no multiplier)
while keeping `--io-threads` as an override. The two-pool *architecture*
described above is unchanged; only the I/O pool's default size changed.

## Addendum (2026-08-24): full-file hashing and `--verify` now stream

The "Consequences" section above flagged full-file hashing buffering the
entire file in memory as a known follow-up. Closed: `IoPool::hash_full_file`
and `IoPool::files_equal` (used by the full-hash stage and `--verify`
respectively) now read in fixed 1 MiB chunks and hash/compare incrementally,
never holding more than a couple of chunk-sized buffers regardless of file
size. Unlike the partial-hash and full-hash-of-small-files cases (where the
I/O pool still just returns bytes and the calling rayon thread hashes them,
per the implementation note above), these two operations run entirely
inside the I/O worker thread — the chunk-by-chunk interleaving of read and
hash/compare doesn't have a clean seam to hand off to a separate thread
without either buffering a chunk's worth of unnecessary synchronization or
building a more elaborate streaming protocol for no real benefit, the same
reasoning that already applied to the original bytes-vs-hash-there
decision. `--verify`'s memory profile improves further still: the old
implementation buffered *every* file in a hash-matched group simultaneously
to compare them; `files_equal` compares one candidate against the reference
at a time, so peak memory no longer scales with group size at all.
