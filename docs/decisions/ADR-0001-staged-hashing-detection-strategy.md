# ADR-0001: Staged-hashing detection strategy

- Status: Accepted
- Date: 2026-08-24

## Context

The core question for a duplicate-file detector is how it decides two files
are identical. Two competing philosophies exist in this space:

1. **Staged hashing** (fclones, rmlint, jdupes): group by size, narrow with a
   cheap partial hash, confirm with a full hash.
2. **Streaming lockstep comparison**: no hashing — read candidate files in
   lockstep and compare bytes directly, evicting on the first mismatch.

Streaming comparison avoids hash CPU cost and can short-circuit early, but
scales poorly to large duplicate groups (N concurrent open handles, N reads
per chunk) and parallelizes less cleanly than a hash-based pipeline.

## Decision

Use staged hashing: **size → multi-point partial hash → full hash → (optional)
byte-verify**.

- **Hash algorithm**: xxh3-128 via the `xxhash-rust` crate (pure Rust, no C
  toolchain dependency — consistent with the cross-platform-first scope in
  ADR-0002). Non-cryptographic is acceptable: this is not an adversarial-input
  context, and 128 bits of a well-distributed hash is astronomically
  sufficient for accidental-collision avoidance.
- **Partial-hash sampling**: three points — head, middle, and tail — rather
  than a single prefix. Many file formats (video containers, Office
  documents, disk images) share an identical header/metadata block while
  differing later; a prefix-only sample lets those files fall through to a
  full hash unnecessarily, defeating the point of the partial stage.
- **Small-file threshold**: one shared constant (default 128 KiB) serves as
  both the partial-hash sample chunk length and the cutoff below which a file
  skips the partial stage entirely and goes straight to one full hash — below
  that size, reading a "partial" sample and then the full file would just
  read the same bytes twice.
- **Trust level**: hash-matched files are reported as duplicates by default
  (matches fclones/rmlint/jdupes and is the standard behavior for this class
  of tool). An opt-in `--verify` mode adds a full byte-compare pass on
  hash-matched candidates before reporting them, for callers who want zero
  collision risk ahead of an irreversible action (delete, hardlink).
- **Free pre-pass**: before any hashing, files are grouped by
  `(device, inode)` / platform file-id (stat-only, zero I/O). Existing
  hardlinks already share storage and are collapsed to one hashing
  representative, with all aliases carried through to the final report.

## Consequences

- The engine never needs to hold N full files open concurrently for
  comparison — good scaling to very large duplicate groups.
- Non-cryptographic hashing means this engine is unsuitable, as-is, for any
  future adversarial-input use case (e.g. content-addressed storage exposed
  to untrusted uploads) without revisiting the hash choice.
- `--verify` trades a full extra read pass for zero collision risk; it is not
  the default because it meaningfully slows large scans for a risk that is
  already negligible with a 128-bit hash.
