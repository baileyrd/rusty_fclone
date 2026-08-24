# AGENTS.md
## Scope
Applies to the whole repository. A nearer `AGENTS.md` (none currently exist
under `crates/`) would override this file.

## Project shape
- Purpose: `rusty_fclone` is a duplicate-file finder — a spiritual successor
  to [fclones](https://github.com/pkolaczk/fclones) — starting from the
  fastest practical detection engine (see
  `docs/architecture/SYSTEM-ARCHITECTURE.md`).
- Rust structure: a two-member Cargo workspace.
  - `crates/rusty_fclone-core` — the detection engine library. No CLI
    concerns (argument parsing, stdout formatting, exit codes) belong here.
  - `crates/rusty_fclone-cli` — the `rusty-fclone` binary; a thin
    `clap`-based consumer of `rusty_fclone-core`.
- Architectural boundaries:
  - The engine's public API is streaming (`ScanHandle: Iterator<Item =
    ScanEvent>`), not a collected `Vec`. Don't change `scan()` to return a
    collected result — see ADR-0004.
  - Blocking file reads happen only through `io_pool::IoPool`, never
    directly on a rayon thread — see ADR-0002.
  - No dependency that requires a C toolchain (keeps the cross-platform
    build simple — see ADR-0002/ADR-0006). If a change needs one, that's an
    ADR-worthy decision, not a routine dependency bump.

## Coordination
Follow `WORKFLOW.md` for handoffs and review — it governs process, not
project architecture.

## Canonical commands
- Format: `cargo fmt --all --check`
- Lint: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- Test: `cargo test --workspace`
- Docs/build: `cargo doc --workspace --all-features --no-deps`

## Change rules
- Every architecture-level decision (algorithm, concurrency model, public
  API shape, platform scope, dependency policy, license) gets an ADR under
  `docs/decisions/` — see `docs/decisions/adr-cadence` guidance in the
  `rust-repo-lifecycle` skill this repo was bootstrapped with. Routine
  implementation mechanics (internal function names, minor refactors) don't
  need one.
- Update `docs/specifications/detection/FCLONE-DETECTION-001.md` and
  `docs/traceability/TRACEABILITY.md` whenever a requirement's
  implementation or verification state changes.
- Update `docs/PROJECT-STATUS.md` after every merge to `main`.
- No dependency added without a one-line justification in the relevant ADR
  (see ADR-0006 for the current foundational set).

## Definition of done
- `cargo fmt`, `cargo clippy -D warnings`, and `cargo test` all pass on the
  pinned toolchain (`rust-toolchain.toml`).
- New or changed behavior has a test exercising it directly (prefer a unit
  test in the relevant module over an end-to-end-only check).
- Traceability and spec docs reflect the current implementation state, not
  an aspirational one.
- `docs/PROJECT-STATUS.md` reflects the change before it's reported as done.
