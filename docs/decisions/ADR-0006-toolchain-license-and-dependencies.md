# ADR-0006: Toolchain policy, license, and foundational dependencies

- Status: Accepted
- Date: 2026-08-24

## Context

Bootstrap-phase decisions that need to be recorded explicitly rather than
assumed: license, whether the Rust toolchain is pinned, crate naming, and
which crates the detection engine depends on.

## Decision

- **License**: dual `MIT OR Apache-2.0` — the de facto standard for Rust
  crates (matches fclones itself), maximizing compatibility for anything
  that wants to depend on `rusty_fclone-core` as a library.
- **Toolchain**: pinned via `rust-toolchain.toml` (`channel = "1.94.1"`,
  with `rustfmt` and `clippy` components) for reproducible builds across
  contributors, CI, and time. Bump deliberately when a new feature is
  needed, not opportunistically.
- **Crate naming**: `rusty_fclone-core` / `rusty_fclone-cli`, matching the
  repository name exactly, over the shorter `fclone-core` / `fclone-cli`.
- **Foundational dependencies** (all pure Rust, no C toolchain requirement —
  consistent with ADR-0002's cross-platform-first scope):
  - `jwalk` — parallel directory traversal (rayon-backed internally).
  - `rayon` — the CPU-bound thread pool for hashing (ADR-0002).
  - `crossbeam-channel` — bounded/unbounded channels gluing pipeline stages
    together; also used directly by the hand-rolled I/O pool.
  - `xxhash-rust` (`xxh3` feature) — partial/full content hashing (ADR-0001).
  - `file-id` — cross-platform stable file identity (device+inode on
    Unix, volume+file-index on Windows), used for both hardlink pre-dedup
    and filesystem-boundary detection (ADR-0001, ADR-0003).
  - `thiserror` — error type derivation (`ScanError`, `FileError`).
  - `clap` (`derive` feature, CLI crate only) — argument parsing.
  - `tempfile` (dev-dependency) — test fixtures.
  - Deliberately *not* used in v1: a custom thread-pool crate (the I/O pool
    is small enough to hand-roll on `std::thread`, per the "don't add
    dependencies speculatively" principle — see ADR-0002).

## Consequences

- No C compiler / cross-compilation toolchain is required to build this
  project on any target platform, which matters for the cross-platform
  scope committed to in ADR-0002.
- The toolchain pin means `rust-toolchain.toml` needs a deliberate bump (and
  a re-run of the full validation suite) whenever a contributor wants a
  newer compiler feature — tracked as ordinary maintenance, not exempted
  from review.

## Addendum (2026-08-24): benchmark tooling

Added `criterion` (default features) as a dev-dependency of
`rusty_fclone-core`, plus a `benches/detection.rs` bench target, to satisfy
the `DETECTION-BENCHMARK` roadmap unit. This is dev-only — it does not
appear in either crate's `[dependencies]` and has no effect on the shipped
binary or library — so it doesn't touch the "no C toolchain" constraint
above; noted here rather than as a new ADR because it's tooling, not an
architectural decision. `criterion`'s default features pull in a plotting/
reporting dependency tree (`plotters`, `tinytemplate`, etc.); this is normal
for any Rust project using it and is accepted as dev-only weight.

