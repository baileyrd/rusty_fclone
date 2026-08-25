# Project Status
- Last verified main commit: `6ab7a5e` (PR #24, merged) — a docs-loop
  pass (PRs #20-#24: README's CLI-scope line since superseded by the GUI
  below, `AGENTS.md`/`WORKFLOW.md`/`SYSTEM-ARCHITECTURE.md` drift fixes,
  and a `docs-audit.md` resolution record). This branch adds the `GUI`
  unit (new `rusty_fclone-gui` crate) on top.
- Tagged: `v0.1.0` at commit `b616294`, GitHub Release published with all
  four platform archives attached (verified via the GitHub API after
  `.github/workflows/release.yml`'s first real dispatch succeeded — see
  `docs/decisions/ADR-0018-release-binaries.md`). `v0.2.0` pending — the
  workspace version was bumped to `0.2.0` but the tag itself hasn't been
  pushed yet (tag pushes require a maintainer's own credentials in this
  environment); everything merged since `v0.1.0` will be tagged once that
  happens.
- Verified at: 2026-08-25
- Current milestone: none in progress. See `docs/roadmap/ROADMAP.md`.
- Health: green — workspace (now three crates) builds, lints, and tests
  clean on the pinned toolchain

## Completed
- `DETECTION-BASELINE`, `DETECTION-BENCHMARK`, `DETECTION-BENCHMARK-VS-FCLONES`,
  `DETECTION-ADAPTIVE-SAMPLE-SIZE`, `DETECTION-IO-THREAD-SIZING` — the full
  detection engine, benchmarked and tuned to beat or match fclones on all
  four synthetic scenarios. Merged to `main` via PR #1 and #2. See
  `docs/benchmarks/FCLONES-COMPARISON.md` for the numbers.
- `ACTION-LAYER` — delete/hardlink redundant copies, dry-run by default.
  `rusty_fclone_core::action` module (`plan`/`apply`, ADR-0009) plus
  `--action <report|delete|hardlink>` and `--apply` CLI flags. Merged via
  PR #3.
- Two known gaps closed via PR #4: symlink-cycle safety net; streaming
  full-file hashing (ADR-0002 addendum).
- Structured observability (`tracing`) — ADR-0010. Merged via PR #5.
- Path storage: `Arc<Path>` instead of `PathBuf`. ADR-0011. Merged via PR #6.
- `DETECTION-TRAVERSAL-COLLAPSE-FUSION` — ADR-0012. Merged via PR #7.
- `DETECTION-DEVICE-AWARE-IO-SIZING` — ADR-0013. Merged via PR #8.
- `ACTION-REFLINK` — ADR-0014, `FCLONE-ACTION-001` 0.2.0. Merged via PR #9.
- `CLI-UX`: `--format text|json`, `ScanEvent::Progress`, confirmation
  prompt. ADR-0015, `CLI-UX-001` 0.1.0. Merged via PR #10 — closed out the
  original "build it all and close all gaps" batch (PRs #4–#10, plus a
  docs-only #11).
- `DETECTION-INCREMENTAL-CACHE`: new opt-in `cache` module backed by
  `redb` — a file whose `(size, mtime)` match a cached entry reuses its
  full hash, skipping both the partial-hash and full-hash stages for that
  file. `ScanOptions::cache_path`/CLI `--cache <path>`, off by default.
  ADR-0016, `FCLONE-DETECTION-001` 0.1.8 (NFR-004). Implemented, tested
  (60/60 tests: 7 new `cache` unit tests + 2 `pipeline` integration
  tests), manually smoke-tested via `-vvv` trace output (zero hits cold,
  exactly one hit per file warm, correct results throughout). Merged via
  PR #12. Benchmark verification of the cache-off path was inconclusive
  in this environment (noisy shared-container load swung the criterion
  comparison between "+144% regressed" and "-6.8% improved" across
  consecutive runs of identical code) — the code path is structurally
  unaffected (a `None` short-circuit), so not treated as a real
  regression signal; see ADR-0016's consequences.
- `CLI-SCAN-HISTORY`: new `history` module (`rusty_fclone-cli` only, no
  core-crate change) backed by SQLite (`rusqlite`, `bundled` feature) — a
  summary of each completed scan (files/bytes scanned, duplicate
  groups/files, and any action's kind/applied/bytes-reclaimed/files-
  acted-on) is appended as one row when `--history <path>` is set, off by
  default. Deliberately scoped to per-scan summaries only, not per-file/
  per-group detail, and no query/report subcommand yet (both explicitly
  deferred, matching this project's established scoping pattern).
  ADR-0017, `CLI-UX-001` 0.2.0. Implemented, tested (fmt/clippy/test/bench
  all green, 66/66 tests — 4 new `history` unit tests + 2 new CLI-level
  tests), and manually smoke-tested (two real scans — plain, then
  `--action delete --apply` — produced two correctly-populated rows,
  confirmed via a direct SQL query). Merged via PR #13.
- `RELEASE-BINARIES`: `.github/workflows/release.yml`, triggered on `v*`
  tag pushes and manual `workflow_dispatch`, builds `rusty-fclone` for
  `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`,
  `x86_64-apple-darwin`, and `x86_64-pc-windows-msvc`, then uses
  `softprops/action-gh-release` to attach each platform's archive to the
  tag's GitHub Release. ADR-0018. Merged via PR #15; a follow-up fix
  (PR #16) added an optional `tag` `workflow_dispatch` input after
  discovering GitHub can only dispatch a workflow from a ref where the
  workflow file itself already exists — `v0.1.0` predates `release.yml`,
  so it must be dispatched from `main` with `tag=v0.1.0` instead of
  directly from the tag. That dispatch (run #1, `workflow_dispatch`,
  `conclusion: success`) attached all four platform archives to `v0.1.0`'s
  release, confirmed via the GitHub API:
  `rusty-fclone-v0.1.0-x86_64-unknown-linux-gnu.tar.gz` (2,444,918 B),
  `rusty-fclone-v0.1.0-aarch64-apple-darwin.tar.gz` (2,161,166 B),
  `rusty-fclone-v0.1.0-x86_64-apple-darwin.tar.gz` (2,310,563 B),
  `rusty-fclone-v0.1.0-x86_64-pc-windows-msvc.zip` (2,246,772 B).
- `DETECTION-FCLONES-CACHE-IMPORT`: new opt-in `fclones_import` module —
  reads an existing upstream-`fclones` `--cache` `sled` database
  directly (its on-disk schema reverse-engineered from fclones 0.35.0's
  own source, not documented anywhere) and reuses a file's full hash
  fclones already computed, when fclones used its `xxhash3` algorithm
  (the only one byte-compatible with this project's own xxh3-128 hash)
  and the entry isn't stale. Tried after a `--cache` miss, before any
  real I/O; an imported hit is also written to `--cache` if set.
  `ScanOptions::fclones_import_path`/CLI `--import-fclones-cache <path>`,
  off by default, independent of `--cache`. ADR-0019,
  `FCLONE-DETECTION-001` 0.1.9 (NFR-005). Implemented, tested (76/76
  tests — 9 new `fclones_import` unit tests, including one asserting a
  decoded hash matches a value captured from a real fclones run), and
  additionally verified end-to-end against the actual `fclones` 0.35.0
  binary in this environment (both a small-file and a large-file
  duplicate pair, confirming both the exact-match and default-prefix-
  length lookup paths via `-vvv` trace output). Merged via PR #17.
- README's Options block and examples synced with the CLI's actual
  `--help` output (`--cache`, `--import-fclones-cache`, `--history`,
  `-y`/`--yes`, `--format`, `-v`/`--verbose` were all missing; the Status
  section and two other spots still said reflink support wasn't built).
  Merged via PR #18.
- Workspace version bump `0.1.0` → `0.2.0` (`Cargo.toml`
  `workspace.package.version`) to tag everything merged since `v0.1.0`.
  No functional change.
- A `docs-loop` pass (whole tracked doc surface, 31 docs, prompted by
  README's missing UI/GUI mention): fixed 12 findings across four docs —
  README's CLI-only-scope note (since superseded by the `GUI` unit
  below), `AGENTS.md` (action-layer list, C-toolchain-rule precedent
  note, an internal skill-reference reword), `WORKFLOW.md` (a stale
  bootstrap-phase Authority section, a hardcoded ADR count), and
  `SYSTEM-ARCHITECTURE.md` (reflink shipped, not deferred; a stale
  `traversal::traverse` return-type claim; a broken ADR path/range). Step
  5 re-verification confirmed `scripts/check_references.py` went from 2
  `broken` references to 0. `docs-audit.md` (committed, with a resolution
  record added afterward) has the full findings table. Merged via PRs
  #20-#24.

## In progress
- `GUI` (this branch): new `rusty_fclone-gui` crate — a Tauri (v2) desktop
  GUI covering the same scan-and-act workflow as the CLI, reversing the
  v1 "no GUI" non-goal. ADR-0020, `GUI-UX-001` 0.1.0. Implemented, tested
  (85/85 workspace tests — 9 new `rusty_fclone-gui` tests: 5 `payload`
  unit tests, 4 `commands` IPC-level tests via `tauri::test`'s mock
  runtime, asserting on real filesystem state). Also manually verified
  end-to-end in this environment via Xvfb (no real display available): the
  compiled binary launched, rendered the real frontend, and a full scan →
  duplicate-group display → preview action → apply action cycle was
  driven through the actual UI with `xdotool`, confirmed against real
  filesystem state before/after. `.github/workflows/ci.yml` now installs
  Tauri's Linux system-webview dev packages before building — see
  ADR-0020's C-toolchain-exception note. `release.yml` is unchanged
  (still CLI-only); see `GUI-RELEASE-BUNDLES` in the roadmap.

## Blocked
- None.

## Next
- Follow-on units intentionally left open by earlier scoping decisions
  (each needs its own design work before starting): `DETECTION-STREAMING-OVERLAP`
  proper (full pipeline overlap, needs a `ScanEvent` finality-contract
  decision first), `DETECTION-LINUX-FASTPATH` proper (io_uring/FIEMAP,
  needs an async runtime and unsafe FFI, its own ADR), and — if wanted —
  a query/report surface over `--history`'s accumulated data (explicitly
  out of scope for `CLI-SCAN-HISTORY` itself).
- `GUI-RELEASE-BUNDLES`: packaged, installable GUI distribution via
  `tauri build`'s bundler, needing per-platform prerequisites beyond
  CI's current build-and-test install step, plus real (non-placeholder)
  application icons.
- A native file/directory picker for the GUI's root-path field (currently
  a plain text input) — deferred pending a look at Tauri's `dialog`
  plugin's own permission/capability shape (`GUI-UX-001`'s open
  questions).

## Validation
- `cargo fmt --all --check`: pass (2026-08-25)
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass (2026-08-25)
- `cargo test --workspace`: pass, 85/85 (2026-08-25)
- `cargo bench --workspace --no-run`: pass (2026-08-25)
- `cargo doc --workspace --all-features --no-deps`: pass (2026-08-25)
- Manual CLI smoke tests across the project: verbosity flags, `RUST_LOG`
  override, default output silent on success, action dry runs, JSON
  format, progress checkpoints, confirmation prompt decline/accept,
  cold/warm `--cache` behavior, `--history` across two real scans, and
  `--import-fclones-cache` against a real `fclones` binary and its real
  cache database (2026-08-24)
- Manual GUI smoke test (2026-08-25): compiled binary launched under
  Xvfb, real frontend rendered (caught and fixed one stale-embedded-
  asset build during this pass), a full scan → duplicate-group render →
  preview → apply cycle driven via `xdotool` against a real tempdir with
  a known duplicate pair, filesystem state confirmed via `ls` before and
  after the apply step.

## Risks and decisions needed
- The action layer is the first genuinely destructive capability in this
  codebase. Its safety model (dry-run default, two-flag confirmation, plus
  the interactive confirmation prompt) is documented and tested, but has
  not been used against a real, valuable directory tree outside this
  session's smoke tests — treat it with appropriate caution before
  pointing it at anything you care about.
- `DETECTION-DEVICE-AWARE-IO-SIZING`'s rotational-disk detection logic is
  unverified against a real spinning disk (this environment's storage
  resolves to the safe `cores` fallback).
- `ACTION-REFLINK`'s success path is unverified end-to-end in this
  environment (no CoW-capable filesystem available to test against).
- `CLI-UX-001`'s JSON schema isn't versioned or promised stable yet.
- `DETECTION-INCREMENTAL-CACHE`'s benchmark verification was inconclusive
  in this noisy environment (see above); its only invalidation signal is
  `(size, mtime)` — a file whose content changes without its mtime
  updating (contrived, or an unreliable filesystem) would be served a
  stale hash, the same trust model `make` and most incremental build
  tools accept.
- `CLI-SCAN-HISTORY`'s schema isn't versioned; a future incompatible
  change would need a migration story that doesn't exist yet. No query/
  report tooling exists yet either — reading history back is manual SQL
  or a future unit.
- `DETECTION-STREAMING-OVERLAP` proper needs a decision on how to relax or
  redesign `ScanEvent`'s "no group revision after emission" finality
  contract (ADR-0004) before it can be implemented — not yet made.
- `DETECTION-FCLONES-CACHE-IMPORT` is Unix only (fclones' Windows file-id
  encoding isn't reproducible via the `file-id` crate this project
  depends on) and only recovers a small file's hash when fclones used one
  of its two documented default prefix lengths (4 KiB/16 KiB) — a tree
  cached with an explicit non-default `--max-prefix-size` won't be found.
  Both are deliberate, documented scope cuts (ADR-0019): a missed
  optimization, never a wrong result.
- `GUI`'s icon assets are placeholder solid-color squares, not real
  application art — fine for `cargo build`/`clippy`/`test`, not for a
  real release (ADR-0020's consequences).
- `GUI` was only verified on Linux (this environment's only available
  platform) — macOS rendering is unverified; Windows rendering is
  unverified too (only `cargo build` has been attempted on Windows so
  far, via a real user's build, not a running/rendered window). No
  automated frontend/DOM test exists (`app.js` is covered by the manual
  Xvfb pass only); `GUI-UX-001`'s open questions track this. Three real
  gaps surfaced by that Windows build attempt, all since fixed: the MSVC
  C++ toolchain prerequisite for `embed-resource` wasn't documented
  (README, ADR-0020; `GUI-UX-001` 0.1.1); a missing `icons/icon.ico`
  blocked the build outright, not just release bundling as originally
  assumed (`GUI-UX-001` 0.1.2); and, after switching to the GNU toolchain
  (no admin rights available to install MSVC), the crate's unused
  `cdylib` output overflowed MinGW's classic linker's export-ordinal
  field on Tauri's large dependency tree — trimmed `[lib] crate-type`
  down to just what's needed (`GUI-UX-001` 0.1.3). `.icns` (macOS) is
  still missing and could carry the same "blocks debug builds too" risk
  as `icon.ico` did; genuinely unverified, since no macOS build attempt
  has happened yet.
- `GUI-RELEASE-BUNDLES` (packaged, installable GUI distribution) is not
  started — `release.yml` still only builds the CLI binary.
