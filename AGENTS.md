# AGENTS.md
## Scope
Applies to the whole repository. A nearer `AGENTS.md` (none currently exist
under `crates/`) would override this file.

## Project shape
- Purpose: `rusty_fclone` is a duplicate-file finder — a spiritual successor
  to [fclones](https://github.com/pkolaczk/fclones) — with a detection
  engine and an action layer (delete/trash/hardlink/reflink/move/copy) on
  top of it, consumed by both a CLI and a desktop GUI (see
  `docs/architecture/SYSTEM-ARCHITECTURE.md`).
- Rust structure: a three-member Cargo workspace.
  - `crates/rusty_fclone-core` — the detection engine (`scan`) and action
    layer (`action::plan`/`action::apply`) library. No CLI or GUI
    concerns (argument parsing, stdout formatting, exit codes, `serde`)
    belong here.
  - `crates/rusty_fclone-cli` — the `rusty-fclone` binary; a thin
    `clap`-based consumer of `rusty_fclone-core`. `main` is a two-line
    wrapper around a testable `run(cli: Cli) -> ExitCode` — keep it that
    way so CLI-level behavior stays unit-testable without spawning a
    subprocess.
  - `crates/rusty_fclone-gui` — a Tauri (v2) desktop app consuming
    `rusty_fclone-core` (ADR-0020). Rust backend commands in
    `src/commands.rs`, `serde` wire DTOs in `src/payload.rs` (kept out of
    `rusty_fclone-core` itself), plain HTML/CSS/JS frontend in `ui/`.
- Architectural boundaries:
  - The engine's public API is streaming (`ScanHandle: Iterator<Item =
    ScanEvent>`), not a collected `Vec`. Don't change `scan()` to return a
    collected result — see ADR-0004.
  - Blocking file reads happen only through `io_pool::IoPool`, never
    directly on a rayon thread — see ADR-0002.
  - No dependency that requires a C toolchain (keeps the cross-platform
    build simple — see ADR-0002/ADR-0006). If a change needs one, that's an
    ADR-worthy decision, not a routine dependency bump — the escape hatch
    has been used twice, deliberately: `rusqlite`'s `bundled` feature
    (ADR-0017) vendors and compiles a C build of SQLite, accepted because
    it keeps the "self-contained binary" property (no runtime dependency)
    rather than requiring a system SQLite install; `rusty_fclone-gui`'s
    Tauri backend (ADR-0020) links against the system webview
    (`libwebkit2gtk-4.1`/`libgtk-3`/`libsoup-3.0` on Linux, real
    build-time system-library dependencies, not vendored) — CI installs
    them explicitly (`.github/workflows/ci.yml`) rather than the rule
    being silently exempted for that one crate.
  - Any destructive capability (the action layer, and anything added after
    it) must default to a no-op preview and require an explicit, separate
    confirmation flag to actually mutate the filesystem — see ADR-0009. This
    is a standing safety rule, not a one-off choice for `--action`/`--apply`.

## Coordination
Follow `WORKFLOW.md` for handoffs and review — it governs process, not
project architecture.

## Canonical commands
- Format: `cargo fmt --all --check`
- Lint: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- Test: `cargo test --workspace`
- Benchmark: `cargo bench -p rusty_fclone-core` (CI only compiles benches
  via `cargo bench --workspace --no-run`; run this locally for real numbers)
- Docs/build: `cargo doc --workspace --all-features --no-deps`

## Change rules
- Every architecture-level decision (algorithm, concurrency model, public
  API shape, platform scope, dependency policy, license) gets an ADR under
  `docs/decisions/` — see the `rust-repo-lifecycle` skill's own
  `references/adr-cadence.md` for the cadence guidance this repo was
  bootstrapped with (that reference lives inside the skill, not this
  repo). Routine implementation mechanics (internal function names, minor
  refactors) don't need one.
- Update the relevant spec (`docs/specifications/detection/FCLONE-DETECTION-001.md`,
  `docs/specifications/action/FCLONE-ACTION-001.md`,
  `docs/specifications/cli-ux/CLI-UX-001.md`,
  `docs/specifications/gui-ux/GUI-UX-001.md`, or a future one) and
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
