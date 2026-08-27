# Project Status
- Last verified main commit: `51f9cb6` — merged `GUI-REVIEW-PANELS`
  (PR #52). Prompted by a refreshed `Deduplication app UI design.zip`
  (v2 handoff) committed to the repo root; see the Completed entry below
  for the full account. Prior checkpoint: `c67f63c` — merged
  `GUI-SCAN-LAYOUT-FIX` (PR #47). `docs/roadmap/
  DEDUP-GAP-IMPLEMENTATION-PLAN.md` was already **fully implemented in
  its entirety** as of `DASHBOARD-CHART-UPGRADE` (PR #46) — all three
  phases, every listed unit; `GUI-SCAN-LAYOUT-FIX` and `GUI-REVIEW-
  PANELS` are both follow-ups, not plan items. The user asked to see a
  real screenshot tour of the GUI, which meant installing `xdotool` in
  this environment (not present all session) and actually driving the
  compiled binary under Xvfb for the first time since `FOLDER-ACTION`.
  That pass found and fixed a real, previously-undiscovered layout bug
  on Scan Setup (see the `GUI-SCAN-LAYOUT-FIX` Completed entry) and
  re-verified several units' GUI surfaces that had only ever had
  IPC-level coverage.
- Tagged: `v0.1.0` at commit `b616294`, GitHub Release published with all
  four platform archives attached (verified via the GitHub API after
  `.github/workflows/release.yml`'s first real dispatch succeeded — see
  `docs/decisions/ADR-0018-release-binaries.md`). `v0.2.0` pending — the
  workspace version was bumped to `0.2.0` but the tag itself hasn't been
  pushed yet (tag pushes require a maintainer's own credentials in this
  environment); everything merged since `v0.1.0` will be tagged once that
  happens.
- Verified at: 2026-08-27
- Current milestone: a Scan Setup layout bug fix + a real Xvfb/`xdotool`
  GUI verification pass (not a `DEDUP-GAP-IMPLEMENTATION-PLAN.md` item —
  that plan is fully done) — implemented, validated, merged (PR #47).
- Health: green — workspace (three crates) builds, lints, and tests clean
  on the pinned toolchain

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

- `GUI`: new `rusty_fclone-gui` crate — a Tauri (v2) desktop GUI covering
  the same scan-and-act workflow as the CLI, reversing the v1 "no GUI"
  non-goal. ADR-0020, `GUI-UX-001` (0.1.0 → 0.1.4 across four follow-up
  fixes, all from real usage — see Risks and decisions needed below for
  what each one was). Implemented, tested (92/92 workspace tests — 16
  `rusty_fclone-gui` tests: `payload` unit tests plus `commands`
  IPC-level tests via `tauri::test`'s mock runtime, asserting on real
  filesystem state). Manually verified end-to-end in this environment via
  Xvfb (no real display available): the compiled binary launched,
  rendered the real frontend, and a full scan → duplicate-group display →
  preview action → apply action cycle was driven through the actual UI
  with `xdotool`, confirmed against real filesystem state before/after.
  **Also independently confirmed on a real Windows desktop** by an actual
  user, after working through the build-time gaps above: correct window
  rendering, and a real scan/duplicate-group/action cycle. Merged via
  PR #25, with four small follow-up fixes (PRs #26–#28 plus the FR-012
  quote-stripping fix) as real usage surfaced real gaps.
  `.github/workflows/ci.yml` now installs Tauri's Linux system-webview
  dev packages before building — see ADR-0020's C-toolchain-exception
  note. `release.yml` is unchanged (still CLI-only); see
  `GUI-RELEASE-BUNDLES` in the roadmap.

- `DETECTION-FOLDER-DEDUP`: new `rusty_fclone_core::folder_dedup` module
  and public `find_folder_duplicates(root, groups, options) ->
  Result<Vec<FolderMatch>, ScanError>` — a post-scan pass (not a
  `scan()`/`ScanEvent` streaming extension, since a folder verdict needs
  the whole tree's picture) that runs its own lightweight second,
  stat-only traversal to learn every directory's complete file set
  (including files with no duplicate anywhere, which a normal scan never
  surfaces), then reports `FolderMatch::Exact` clusters and
  `FolderMatch::Contained` subset/superset pairs. A directory is only
  eligible as an `Exact`/subset side once every file in its subtree has a
  duplicate somewhere in the tree; a directory with extra files of its
  own can still be a `Contained` superset. Shallowest-first
  claim-and-skip-descendants suppression keeps a top-level folder match
  from flooding the output with every implied nested subdirectory match.
  No new destructive action — detection and reporting only. ADR-0021,
  `FCLONE-DETECTION-001` 0.2.0 (FR-010–FR-013, NFR-006). New CLI
  `--find-duplicate-folders` flag (`CLI-UX-001` 0.2.2, FR-012), off by
  default, with `--format text` and `--format json`
  (`folder_exact`/`folder_contained` NDJSON events) output. Implemented,
  tested (101/101 workspace tests — 7 new `folder_dedup` unit tests + 2
  new CLI-level tests), and manually smoke-tested against a real
  three-directory tree (`photos/vacation` fully duplicated inside
  `backup`, which also has an unrelated extra file elsewhere) confirming
  the exact text and NDJSON output, including the `Contained`
  subset/superset direction. GUI surfacing was scoped as a separate
  follow-up, not bundled into this change — done, see `GUI-REDESIGN`
  below.

- `GUI-REDESIGN`: a high-fidelity design handoff (`Deduplication app UI
  design.zip`, committed at repo root) specified a full rebuild of
  `rusty_fclone-gui`'s bare-bones frontend into a 4-screen app
  (Dashboard, Scan Setup, Duplicate Review, Rules & Automation). Shipped
  in two PRs to keep each change small and reviewable: `find_duplicate_folders`
  (PR #31, a new Tauri command exposing `DETECTION-FOLDER-DEDUP`'s engine
  to the GUI backend), then this branch, the actual frontend rebuild —
  `ui/app.js`/`ui/style.css`/`ui/icons.js` rewritten from scratch, driven
  entirely by real data from `start_scan`/`run_action`/
  `find_duplicate_folders`, never mocked. Asked the user one question
  before implementing (the mockup's "Delete Duplicate Folder" button has
  no real backend — ADR-0021 deliberately has no folder-level delete
  action): decided to disable it with an explanation rather than wire it
  to a guessed behavior. Every other mockup/reality gap found while
  implementing was decided and recorded rather than silently papered
  over — one scan root instead of a folder checklist, "Similar content"
  match mode shown disabled (fuzzy matching is a detection non-goal),
  two fake toggles replaced with three real `ScanOptions` toggles,
  file-type chips filter the Review list only (defaulting to all-on, not
  the mockup's partial preselection, since a display filter silently
  hiding real duplicates by default would be a correctness trap), the
  folder-dedup pass now runs automatically after every scan, choosing
  which copy to keep reorders `paths` client-side instead of adding a
  core API, Dashboard/Recent-Scans are real but session-scoped (not
  persisted — no GUI-side history reader or export exists yet), Rules &
  Automation is an explicit local-only preview, a system font stack
  replaces the mockup's Google Fonts dependency (this app works
  offline), and the mockup's fake OS window chrome (traffic lights,
  shadow, rounded window) is dropped since a real Tauri window already
  has real chrome. Full rationale: ADR-0022. `GUI-UX-001` 0.2.0
  (FR-011 revised, FR-014 through FR-017 added). Implemented, tested
  (105/105 workspace tests — no Rust logic changed in this branch beyond
  the prior PR's `find_duplicate_folders`; the frontend has no automated
  test suite, same open gap as before this change), and manually
  end-to-end verified via Xvfb + `xdotool`: a real scan against a tempdir
  with both file- and folder-level duplicates rendered correctly across
  all four screens and both themes, choosing a non-default copy to keep
  and applying delete removed the correct file (confirmed via `ls`
  before/after), and the Dashboard reflected real post-scan numbers.

- `FOLDER-ACTION` (core capability): new `rusty_fclone_core::folder_action`
  module — `plan_folder`/`apply_folder`, the same plan/apply split as the
  existing per-file `action` module, acting on every file in one folder
  ("removed") against its confirmed partner in another ("kept") for a
  `FolderMatch`. Deliberately reuses `action::apply` per file pair
  (a single-action `ActionPlan` per pair) rather than any new
  delete/hardlink/reflink code. `plan_folder` re-verifies every file's
  partner and current on-disk size against the supplied
  `DuplicateGroup`s before producing a plan — fails closed (no plan at
  all, not a partial one) if anything doesn't match, guarding against a
  stale scan or a caller-passed folder pair that doesn't actually hold
  the claimed relationship. `Delete` prunes the emptied folder tree only
  after every file succeeds; `Hardlink`/`Reflink` never touch the
  directory. ADR-0023, `FCLONE-ACTION-001` 0.3.0 (FR-009 through
  FR-011). Implemented, tested (112/112 workspace tests — 7 new
  `folder_action` unit tests: pairing, missing-partner rejection,
  stale-size rejection, nonexistent-folder rejection, delete-with-prune,
  hardlink-without-prune, per-file-failure-without-prune). Core only —
  no CLI/GUI caller yet, a deliberate follow-up (same "capability lands
  before the UI that surfaces it" pattern as `DETECTION-FOLDER-DEDUP` →
  its CLI flag → `GUI-REDESIGN`).

- `FOLDER-ACTION` (CLI wiring): `--find-duplicate-folders` now combines
  with `--action <report|delete|hardlink|reflink>`/`--apply` —
  `rusty_fclone-cli::report_folder_matches` calls `folder_action::
  plan_folder`/`apply_folder` per matched folder pair (`folder_match_pairs`
  picks the alphabetically-first folder as `kept` for an `Exact` cluster,
  pairs subset-against-superset for `Contained`) and prints/emits the
  result per pair, gated by the same preview/`--apply`/confirmation-prompt
  safety model as file-level action. Found and fixed a real bug along the
  way: the CLI's existing live per-group action (inside the streaming scan
  loop) was consuming the file evidence the post-scan folder-dedup pass
  needed, so a folder match could silently vanish once its files had
  already been individually deleted. Fixed by deferring all individual-
  group reporting/action until after the folder-dedup pass whenever
  `--find-duplicate-folders` is set, skipping only the groups a folder
  match already covers (`group_fully_covered_by`) so unrelated/uncovered
  duplicate pairs still get acted on normally. ADR-0023 (unchanged),
  `CLI-UX-001` 0.3.0 (FR-013). Implemented, tested (115/115 workspace
  tests — 3 new CLI-level tests: apply removes the subset folder, dry run
  touches nothing, an unrelated duplicate pair outside any folder match
  still gets acted on), and manually smoke-tested against a real
  filesystem (preview mode, real `--apply` folder pruning confirmed via
  `find` before/after, and exact NDJSON field shapes for both `Exact` and
  `Contained` matches).

- `FOLDER-ACTION` (GUI wiring): a new `run_folder_action` Tauri command
  wraps `folder_action::plan_folder`/`apply_folder`, mirroring
  `run_action`'s preview/apply split. The Duplicate Review screen's
  "Delete Duplicate Folder" button (disabled since `GUI-REDESIGN`) is now
  enabled: a `Contained` match acts directly (subset removed against
  superset); an `Exact` cluster gained a per-folder keep-choice badge
  (mirroring the existing file-level mechanism, defaulting to the
  alphabetically-first folder — same convention the CLI already uses).
  Confirming still goes through the same `window.confirm` safety gate
  file-level actions use. ADR-0023 (unchanged), `GUI-UX-001` 0.3.0
  (FR-018/FR-019). Implemented, tested (119/119 workspace tests — 7 new
  `rusty_fclone-gui` tests: 3 IPC-level `run_folder_action` tests
  covering preview, apply, and a stale-scan rejection, plus a payload
  conversion test, mirroring `run_action`'s own test shape), and manually
  end-to-end verified via Xvfb + `xdotool` against two real trees: a
  `Contained` match (confirmation dialog text checked, real folder prune
  confirmed via `find` before/after, kept side untouched) and an `Exact`
  match (default keep-choice confirmed alphabetically-first, then
  switched via the badge and confirmed the resulting delete actually
  followed the switched choice — not just its displayed label). Found
  along the way, but out of scope to fix here since it predates this
  change and isn't specific to folder actions: the Duplicate Review
  screen's cards don't pick up the light theme correctly (both the
  pre-existing file-level card and the new folder-level one are affected
  equally) — see `GUI-UX-001`'s open questions.

- `DEDUP-GAP-IMPLEMENTATION-PLAN`: added
  `docs/roadmap/DEDUP-GAP-IMPLEMENTATION-PLAN.md`, synthesizing two
  user-supplied market-research documents (a duplicate-file-finder
  capability/UX playbook and a per-product competitive analysis) against
  this repo's actual, verified current state into a proposed three-phase
  set of roadmap-shaped units. Deliberately not added to
  `docs/roadmap/ROADMAP.md` itself at the time (pending sign-off), since
  that file drives `WORKFLOW.md`'s automatic next-unit selection. Merged
  via PR #36.

- `DETECTION-SCAN-FILTERS`: the first table-stakes unit from that plan.
  New `ScanOptions` fields — `min_size`, `max_size`,
  `include_extensions`, `exclude_extensions`, `exclude_paths` — applied
  during traversal, before any hashing. Directory subtrees named in
  `exclude_paths` are pruned via jwalk's `process_read_dir` before
  traversal descends into them (not filtered from results afterward);
  size and extension filters apply per-file right after `stat`, before
  the `get_file_id` syscall, so an excluded file costs one stat and
  nothing else. `FCLONE-DETECTION-001` 0.2.1 (FR-014, NFR-007). CLI
  gained `--min-size`/`--max-size`/`--include-ext`/`--exclude-ext`
  (repeatable)/`--exclude-path` (repeatable), `CLI-UX-001` 0.3.1
  (FR-014). GUI's Scan Setup screen gained a real "Include/exclude
  filters" card wired to `ScanOptionsPayload` and sent to `start_scan`
  for real — deliberately left the Rules & Automation screen's existing
  three preview-only toggles untouched rather than silently making only
  one of them (the size-related one) real while the screen still
  blanket-labels itself "Preview only," `GUI-UX-001` 0.3.1 (FR-020). No
  ADR — routine implementation, no architecture-level decision (per
  `AGENTS.md`'s change rules). Implemented, tested (129/129 workspace
  tests — 9 new `rusty_fclone-core` `traversal` tests: size-bounds unit
  tests, extension-list unit tests including the exclude-wins-over-
  include and no-extension edge cases, and three real-traversal tests
  including one proving an excluded directory's contents are never
  visited, not just filtered afterward; 1 new `rusty_fclone-gui`
  `payload` test asserting the five new fields round-trip and
  `exclude_paths` gets the same quote-stripping normalization as other
  path fields), `cargo fmt`/`clippy -D warnings`/`bench --no-run`/`doc`
  all pass. No dedicated CLI-level test asserts a non-default filter
  value end-to-end (see Risks below — same standing gap as `--cache`/
  `--io-threads`, neither of which has one either). Manually smoke-tested
  against a real filesystem (see Validation below) — `--min-size`,
  `--exclude-ext`, and `--exclude-path` each confirmed to change scan
  results correctly. The GUI's new Scan Setup fields are not yet manually
  verified through the rendered UI (no Xvfb/`xdotool` pass this session).

- `ACTION-TRASH`: the second table-stakes unit from
  `DEDUP-GAP-IMPLEMENTATION-PLAN.md`. New `ActionKind::Trash` — moves a
  redundant copy to the OS trash/recycle bin (via the new `trash` crate
  dependency, `rusty_fclone-core`-only) instead of deleting it
  permanently. `FCLONE-ACTION-001` 0.4.0 (FR-011 revised, FR-012).
  `folder_action::apply_folder`'s directory-prune gate (previously
  `plan.kind == ActionKind::Delete`) now also fires for `Trash`, since
  both leave `removed` file-less the same way. CLI gained `--action
  trash`, `CLI-UX-001` unchanged (reuses the existing generic `--action
  <ACTION>` flag). GUI's action selector gained a "Trash" option and now
  defaults to it instead of permanent `Delete` (`Delete` stays selectable,
  unchanged in behavior), `GUI-UX-001` 0.3.2 (FR-008/FR-018 revised,
  FR-021). ADR-0024 records the decision, modeled directly on ADR-0014's
  "dependency, not hand-rolled per-platform FFI" reasoning for reflink —
  verified in a scratch project that `trash` builds cleanly on Linux with
  no C-toolchain requirement (`AGENTS.md`'s dependency policy) before
  adding it. Implemented, tested (132/132 workspace tests — 3 new tests:
  a core `action` test (trash removes the redundant copy, keeps the kept
  file), a `folder_action` test (trash prunes the emptied folder just
  like delete), and a CLI test (`--action trash --apply` actually
  trashes); one existing GUI `payload` test extended to cover the new
  `"trash"` word), `cargo fmt`/
  `clippy -D warnings`/`bench --no-run`/`doc` all pass. Manually
  smoke-tested against real filesystems in this environment: a real
  file-level `--action trash --apply` run (the redundant copy vanished
  from its original path and reappeared, recoverable, at
  `~/.local/share/Trash/files/`; the kept file untouched) and a real
  `--find-duplicate-folders` + `--action trash --apply` run (the subset
  folder's file trashed, the now-empty folder pruned, the superset
  untouched). The GUI's new "Trash" option is not yet manually verified
  through the rendered UI (no Xvfb/`xdotool` pass this session — same gap
  `DETECTION-SCAN-FILTERS` already left open for its own GUI surface).

- `SELECTION-RULES`: the third and final table-stakes unit from
  `DEDUP-GAP-IMPLEMENTATION-PLAN.md`'s Phase 1, reversing
  `FCLONE-ACTION-001`'s v1 "configurable keep-strategy" non-goal. New
  `rusty_fclone_core::select` module — `Rule::{AlphabeticallyFirst,
  Newest, Oldest, ShortestPath, LongestPath}` and `choose_keep`, returning
  the chosen path plus a one-line reason (the playbook's cheap "why this
  one" explainability win). `action::plan` refactored into a thin wrapper
  over new `plan_with_keep`, which takes an explicit kept path instead of
  always `group.paths[0]` — every existing caller's behavior is
  unchanged. `FCLONE-ACTION-001` 0.5.0 (FR-013, FR-014). Deliberately no
  size-based rule (every path in a `DuplicateGroup` is the same size by
  definition, so it could never distinguish anything) and no folder-level
  `FolderMatch::Exact` rule support (stays alphabetically-first) — both
  recorded in the spec's Non-goals rather than silently out of scope. CLI
  gained `--keep-rule <alphabetical|newest|oldest|shortest-path|
  longest-path>`, `CLI-UX-001` 0.3.2 (FR-015) — the `--format json` action
  shape gained a `keep_reason` field, text output's `keep:` line now
  shows the reason. GUI's previously-fake "Keep newest copy" toggle
  (Rules & Automation) is now real via a new `choose_keep` Tauri command,
  applied live to every group in Duplicate Review that has no manual
  keep-choice override (a manual badge always wins); `run_action` gained
  an optional `keepReason` parameter passed through to its response,
  `GUI-UX-001` 0.3.3 (FR-008/FR-009/FR-014 revised, FR-022). No ADR —
  routine implementation, no architecture-level decision. Implemented,
  tested (146/146 workspace tests — 8 new `select` unit tests covering
  every rule plus metadata-unreadable fallback and cross-rule tie-
  breaking, 1 new `action` test for `plan_with_keep`, 1 new CLI test
  confirming `--keep-rule newest` end-to-end, 5 new GUI tests —
  `choose_keep` success/rejection, `run_action`'s `keepReason`
  default/passthrough, and `parse_keep_rule`), `cargo fmt`/
  `clippy -D warnings`/`bench --no-run`/`doc` all pass. Manually
  smoke-tested against a real filesystem: `--keep-rule newest` against a
  two-file tree with different modification times correctly kept the
  newer file and reported the right reason in both `--format text` and
  `--format json`. The GUI's real toggle is not yet manually verified
  through the rendered UI (no Xvfb/`xdotool` pass this session — same
  standing gap `DETECTION-SCAN-FILTERS`/`ACTION-TRASH` already left open
  for their own GUI surfaces).

- `ACTION-REFERENCE-FOLDERS`: first unit of `DEDUP-GAP-IMPLEMENTATION-
  PLAN.md`'s Phase 2 — a protected/reference-folder guardrail, as a hard
  block rather than a dismissible warning. New `reference_paths:
  &[PathBuf]` parameter on `select::choose_keep`, `action::plan`/
  `plan_with_keep`, and `folder_action::plan_folder`: a path under any
  configured reference folder always wins as `keep` (overriding `Rule`
  and any caller-supplied `keep`) and is filtered out of `actions`/
  `pairs` independently of that override, so the guarantee holds even
  for a caller that bypasses `choose_keep` entirely. `FCLONE-ACTION-001`
  0.6.0 (FR-015 through FR-017). New `FolderActionPlan::
  protected_files_skipped` field; `apply_folder`'s directory-prune step
  (ADR-0023) now also requires it to be zero — caught during
  implementation as a real gap, not just a tidiness fix: without this
  guard, a protected file left inside `removed` after every *planned*
  pair succeeds would still be deleted by the prune's own
  `fs::remove_dir_all`. CLI gained a repeatable `--reference <path>`,
  `CLI-UX-001` 0.3.3 (FR-016). GUI gained a "Protected folders" field on
  Scan Setup, threaded into `run_action`/`choose_keep`/
  `run_folder_action` (not the detection-only commands); the Review
  screen's rule-preview lookup (`ensureRuleKeepChoice`) now also resolves
  via `choose_keep` under the default "alphabetical" rule whenever a
  reference folder is configured, so the "keeping this file" badge
  reflects the guardrail before Apply, `GUI-UX-001` 0.3.4 (FR-023).
  ADR-0025 — extends ADR-0009's safety model, an architecture-level
  decision per `AGENTS.md`. Implemented, tested (158/158 workspace
  tests — 2 new core `select` tests, 3 new core `action` tests, 2 new
  core `folder_action` tests, 2 new CLI tests, 2 new GUI `commands`
  tests), `cargo fmt`/`clippy -D warnings`/`bench --no-run`/`doc` all
  pass. Manually smoke-tested against real filesystems in this
  environment: a file-level `--action trash --reference <dir> --apply`
  run kept a protected file that alphabetical ordering would otherwise
  have lost to an unprotected copy, reporting "in a protected/reference
  folder" as the reason; a folder-level `--find-duplicate-folders
  --action delete --reference <dir> --apply` run against a subset folder
  containing a protected file left both the file and the folder itself
  untouched. The GUI's new field is not yet manually verified through
  the rendered UI — `xdotool` isn't installed in this environment (same
  standing gap `DETECTION-SCAN-FILTERS`/`ACTION-TRASH`/`SELECTION-RULES`
  already left open for their own GUI surfaces).

- `ACTION-MOVE-COPY`: second unit of `DEDUP-GAP-IMPLEMENTATION-PLAN.md`'s
  Phase 2 — archive-folder actions. New `ActionKind::Move(PathBuf)`/
  `Copy(PathBuf)`: `Move` relocates a redundant copy into a caller-chosen
  archive directory, mirroring its original path underneath it
  (collision-safe across files with the same name from different
  original directories) and reclaiming space at the scanned location
  like `Delete`/`Trash`; `Copy` does the same but leaves the original
  untouched and reclaims nothing — a consolidate-for-review step, not a
  cleanup one. `FCLONE-ACTION-001` 0.7.0 (FR-018/FR-019), reversing the
  "moving files ... not implemented" v1 non-goal. The archive destination
  is carried as data on the `ActionKind` variant itself rather than a new
  threaded parameter, so `ActionKind` no longer derives `Copy` (the
  trait) — every call site relying on that implicit copy now clones or
  borrows explicitly, with no behavior change for the four pre-existing
  variants (146 tests across the workspace confirmed unaffected before
  any new test was added). A destination that already exists is always a
  per-file failure, never a silent overwrite; `Move` never falls back to
  copy-then-remove on a cross-device `rename` failure, matching ADR-0014's
  own choice for reflink. `folder_action::apply_folder`'s directory-prune
  gate (ADR-0023) now also fires for `Move`. CLI gained `--action
  move`/`copy` plus a required `--archive-dir <path>` (validated
  explicitly, not via clap's generic required-if, so the error names the
  specific action needing it), `CLI-UX-001` 0.3.4 (FR-017). GUI gained a
  conditional "Archive folder" field in the Duplicate Review and
  folder-review action bars, shown only for `move`/`copy`, with Apply
  disabled until it's filled in; `Copy`'s reclaim-estimate text is
  rewritten rather than showing a byte figure that would never actually
  be freed, `GUI-UX-001` 0.3.5 (FR-024). ADR-0026 — extends the
  action-layer's data model, an architecture-level decision per
  `AGENTS.md`. Implemented, tested (172/172 workspace tests — 4 new core
  `action` tests, 2 new core `folder_action` tests, 3 new CLI tests, 5
  new GUI tests), `cargo fmt`/`clippy -D warnings`/`bench --no-run`/`doc`
  all pass. Manually smoke-tested against real filesystems in this
  environment: `--action move --archive-dir <dir> --apply` relocated a
  redundant file to its mirrored archive path and left the kept file
  untouched; `--action copy --archive-dir <dir> --apply` archived a copy
  while leaving both originals in place and reported 0 bytes reclaimed;
  repeating that same `copy` run against the same tree failed cleanly on
  the already-archived destination (confirmed via `find`) without
  touching either original or the first run's archived file;
  `--action move` without `--archive-dir` was rejected before any scan
  ran. The GUI's new field is not yet manually verified through the
  rendered UI — same standing gap as `ACTION-REFERENCE-FOLDERS`.

- `CLI-HISTORY-AUDIT`: third and final unit of `DEDUP-GAP-IMPLEMENTATION-
  PLAN.md`'s Phase 2, completing the phase. Closes the two gaps
  `CLI-SCAN-HISTORY` (ADR-0017) deliberately deferred. New `actions`
  SQLite table: one row per file/pair an *applied* action actually acted
  on (path, kind, bytes, success/failure, error text), FK'd to its
  `scans` row and written in the same transaction — scoped to applied
  actions only, since a preview plans but runs nothing real to audit yet.
  `handle_group`/`report_folder_matches` both record through one shared
  `record_action_outcomes` helper, correlating each planned action
  against its real `ApplyReport`/`FolderApplyReport` outcome, `CLI-UX-001`
  0.3.5 (FR-018). New `rusty-fclone history <list|stats>` command reads
  an existing `--history` database — `list [--limit N]` (newest first),
  `stats [--since TS] [--until TS]` (raw Unix timestamps, no new
  date-parsing dependency) — both supporting `--format text|json`.
  `history` is a reserved top-level keyword, dispatched manually in
  `main` via `args[1] == "history"` before `Cli::parse` runs, rather than
  a `#[command(subcommand)]` on `Cli` itself — `Cli::root`'s required
  positional argument makes that combination ambiguous for clap without
  restructuring every existing scan invocation into a breaking
  `rusty-fclone scan <ROOT> ...` shape, which this unit's scope didn't
  call for. Every existing `rusty-fclone <ROOT> ...` invocation is
  unaffected; the cost is a real directory literally named `history`
  needing `./history` to disambiguate. GUI's Dashboard "Export (JSON)"
  button is wired for real via a plain `<a download>`/object-URL
  (downloads the session's in-memory scan history, no new Tauri plugin
  needed); "Import history" stays an explicit disabled placeholder, its
  tooltip now naming the specific blocker (needs Tauri's `dialog`/`fs`
  plugin for a real filesystem path — already tracked as a separate
  prerequisite for the GUI's root-path picker), `GUI-UX-001` 0.3.6
  (FR-025). ADR-0027. Implemented, tested (185/185 workspace tests — 8
  new core-independent CLI tests: `history` module gained 7 new tests
  (11 total, from 4), `main` module gained 6 new tests), `cargo fmt`/
  `clippy -D warnings`/`bench --no-run`/`doc` all pass. Manually
  smoke-tested against a real filesystem in this environment: two real
  scans (one `report`-only, one `--action trash --apply` across 3
  redundant files) against a `--history` database, followed by
  `history list` (`--format text` and `--format json`) and `history
  stats`, confirmed the exact row counts, per-action detail (right path/
  kind/bytes/succeeded), and aggregate totals (12 bytes reclaimed across
  3 files) matched what actually happened on disk; direct inspection of
  the `actions` table confirmed each row's `scan_id` correctly pointed to
  the trash-and-apply scan, not the report-only one; a plain
  `rusty-fclone <ROOT>` invocation against a real directory confirmed
  unaffected by the new keyword dispatch. The GUI's Export button is not
  yet manually verified through a rendered window — same standing gap as
  every prior GUI surface this session.

- `GUI-MEDIA-PREVIEW`: first unit of `DEDUP-GAP-IMPLEMENTATION-PLAN.md`'s
  Phase 3 — inline thumbnail/audio-player preview in Duplicate Review's
  compare-cards, the playbook's single most-cited UX failure category
  across the products studied. New `read_preview` command/`preview`
  module returns a `data:<mime>;base64,<...>` URI for a small, supported
  image or audio file. Deliberately no new Tauri capability/permission
  grant (didn't adopt the `asset:`/`dialog`/`fs` plugin prerequisite this
  project keeps deferring elsewhere) and no new dependency (base64 is
  hand-rolled, ~30 lines, tested against RFC 4648's own vectors plus a
  real 69-byte PNG file round-tripped byte-for-byte through a real
  decoder — extra assurance since it's hand-rolled rather than a
  battle-tested crate). Photo-category previews render inside the
  existing 56x56 thumbnail slot (`object-fit: cover`); audio-category
  previews render as a full-width `<audio controls>` row, since playback
  controls need real width. Video is explicitly not attempted (typical
  file sizes make whole-file base64 embedding impractical — multi-
  hundred-MB memory spikes, a frozen UI thread — without a streaming
  prerequisite this project hasn't adopted). HEIC/TIFF are excluded from
  preview despite being in `app.js`'s existing `EXT_CATEGORY`'s "photo"
  bucket, since most target webview engines (WebKitGTK, WebView2,
  WKWebView) don't render either natively. A rejected/unsupported path
  (unsupported extension, over the 25 MB size cap, or a real I/O error)
  falls back to the existing generic file icon, never a visible error.
  `GUI-UX-001` 0.3.7 (FR-026). ADR-0028 — this project's first Phase 3
  unit, each of which the plan requires its own ADR and spec revision
  for, unlike Phase 1/2's routine-implementation units. Implemented,
  tested (195/195 workspace tests — 7 new `preview` module tests, 3 new
  `commands` tests), `cargo fmt`/`clippy -D warnings`/`bench --no-run`/
  `doc` all pass. Not yet manually verified through a rendered window in
  this environment (no display/`xdotool`) — the same standing gap every
  GUI-facing unit this session has carried.

- `SCAN-PROFILES`: second unit of `DEDUP-GAP-IMPLEMENTATION-PLAN.md`'s
  Phase 3 — saved scan setups. New `list_scan_profiles`/
  `save_scan_profile`/`delete_scan_profile` commands and a `profiles`
  module persist a named `{root, ScanOptions}` preset as a flat JSON file
  under the OS config directory (`dirs::config_dir()`, already present in
  `Cargo.lock` as one of Tauri's own transitive dependencies — declaring
  it directly added zero new supply-chain surface), not SQLite — a
  deliberately different choice than `CLI-SCAN-HISTORY`/`CLI-HISTORY-
  AUDIT`'s append-only log, since a handful of named presets is a
  read-and-rewrite-whole shape, not a query-shaped one. `GUI-UX-001`
  0.3.7 → 0.3.8 (FR-027), narrowing the spec's prior "no persisted GUI
  state" non-goal to exclude this explicit, user-initiated save (window
  size/position and any unsaved in-progress state are still never
  persisted). Scan Setup gained a "Saved scan profiles" card: a name
  field plus "Save current setup," and a list of saved profiles each
  with "Load"/"Delete" — saving under a name already in use overwrites
  that profile rather than erroring or duplicating it. `ScanOptionsPayload`
  gained `Serialize`/`Clone`/`Default` so the same struct doubles as the
  persisted shape, avoiding a second parallel options type. Deliberately
  no `AppHandle` in the three new commands — Tauri's own
  `app.path().app_config_dir()` needs one, but its behavior under
  `tauri::test`'s mock IPC harness resolves to the *real* host config
  directory (the mock identifier defaults to empty string), which would
  make an IPC-level test write into this machine's actual `~/.config`
  instead of a hermetic tempdir. Resolving the directory via
  `dirs::config_dir()` directly instead keeps `profiles::load`/`upsert`/
  `remove` taking an explicit `&Path`, fully unit-tested against a
  tempdir the same way `preview::build_data_url` already was for
  `GUI-MEDIA-PREVIEW`; the real directory-resolution call itself
  (`profiles::default_profiles_dir()`) is a trusted, untested boundary,
  the same category as `trash`/reflink's non-Linux behavior (ADR-0014/
  ADR-0024). ADR-0029. Implemented, tested (204/204 workspace tests — 7
  new `profiles` module tests, 2 new `commands` tests for
  `save_scan_profile`'s empty/whitespace-only name rejection — the only
  `save_scan_profile` behavior testable at the IPC layer without
  touching real disk, since the directory resolution has no test seam by
  design), `cargo fmt`/`clippy -D warnings`/`bench --no-run`/`doc` all
  pass. Verified by hand once in this environment (a scratch test,
  removed before committing): `default_profiles_dir()` resolved to
  `/root/.config/rusty-fclone`, and a saved/reloaded profile matched the
  expected JSON shape exactly — see Validation below. Not yet manually
  verified through a rendered window in this environment (no display/
  `xdotool`) — the same standing gap every GUI-facing unit this session
  has carried.

- `DETECTION-PERCEPTUAL-IMAGES`: third and final unit of `DEDUP-GAP-
  IMPLEMENTATION-PLAN.md`'s Phase 3, completing the plan. Opt-in
  perceptual image similarity, reversing `SYSTEM-ARCHITECTURE.md`'s
  "near-duplicate/fuzzy matching" v1 non-goal for images specifically.
  New `rusty_fclone_core::perceptual` module and public
  `find_similar_images(root, scan_options, perceptual_options)` — a
  fully self-contained, deliberately separate pass from `scan()` (its
  own traversal, no dependency on any prior scan's `DuplicateGroup`s,
  since perceptually similar images are by definition not byte-identical
  and would never share one). Decodes each image via the `image` crate,
  restricted to pure-Rust codecs only (`default-features = false,
  features = ["jpeg", "png", "gif", "bmp"]` — verified in a scratch
  project to pull in zero C-linked/`-sys` dependencies, satisfying
  `AGENTS.md`'s no-C-toolchain rule the same way ADR-0024's `trash`
  precedent did), computes a hand-rolled 64-bit difference hash (dHash —
  small, fully-specified, RFC-free but well-documented transform, same
  "hand-roll a simple algorithm, depend on genuinely complex format
  decoding" split ADR-0028 already drew for base64 vs. image formats),
  and clusters images within a configurable Hamming-distance threshold
  (default 10/64) via union-find. `SimilarGroup` shares no fields or
  type with `DuplicateGroup` — the plan's "must stay opt-in and clearly
  separated from the hash-verified exact engine" requirement is enforced
  structurally, not just documented. No `--action`/`--apply`/`run_action`
  interaction anywhere: a similarity judgment is explicitly not this
  project's byte-identical guarantee, so building a destructive action on
  top of it would be a materially different safety posture, not a detail
  to bolt on later. `FCLONE-DETECTION-001` 0.2.1 → 0.3.0 (FR-015 through
  FR-017, NFR-008). CLI gained `--find-similar-images`/
  `--similarity-threshold <0-64>`, `CLI-UX-001` 0.3.5 → 0.3.6 (FR-019/
  FR-020) — entirely independent of `--action`/`--apply`/`--history`.
  GUI's previously-disabled "Similar content" match-sensitivity option
  (shipped disabled in `GUI-REDESIGN`, ADR-0022, with exactly this
  feature in mind) is now real, `GUI-UX-001` 0.3.8 → 0.3.9 (FR-028):
  selecting it runs the pass *alongside*, not instead of, the exact scan
  — a deliberate deviation from the mockup's either/or segmented-control
  framing, recorded the same way ADR-0022 already recorded several
  others — and Duplicate Review shows each result as its own,
  `var(--warning)`-tinted, read-only card ("not confirmed identical"),
  no keep-choice, no action bar beyond "Skip." ADR-0030. Implemented,
  tested (221/221 workspace tests — 11 new `perceptual` module tests, 3
  new CLI tests, 3 new GUI tests), `cargo fmt`/`clippy -D warnings`/
  `bench --no-run`/`doc` all pass. Manually verified end-to-end in this
  environment: real synthetic JPEG/PNG photos (a base image, a
  brightness-shifted "re-export," and a resized thumbnail — genuinely
  different byte content throughout) correctly clustered together via
  the compiled CLI binary's `--find-similar-images` in both `--format
  text` and `--format json`, while an unrelated photo was correctly
  excluded, and the exact engine simultaneously reported zero
  `DuplicateGroup`s for the same tree — concrete confirmation the two
  engines' results stay genuinely disjoint, not just structurally
  different in name (see Validation below). The GUI's new "Similar
  content" option is not yet manually verified through a rendered window
  in this environment (no display/`xdotool`) — the same standing gap
  every GUI-facing unit this session has carried.

- `DASHBOARD-CHART-UPGRADE`: the last remaining item from `DEDUP-GAP-
  IMPLEMENTATION-PLAN.md` — with this, the plan is fully implemented.
  Confirmed the Dashboard's "Storage breakdown" was already a real
  chart (a horizontal stacked bar), not numbers-only — and per the
  `dataviz` skill's own form guidance, a stacked bar is the textbook-
  correct form for part-to-whole data with a handful of categories, not
  a donut, so this upgrade brought the existing bar up to the skill's
  concrete mark/interaction specs rather than swapping chart types.
  `storageBreakdown()` gained a `bytes` field alongside its existing
  `pct`/`color`/`label`. Each segment is now a real `<button>` (native
  keyboard focus, no bespoke ARIA) separated from its neighbors by a
  visible 2px gap (previously touching), with a hover/focus tooltip
  showing its exact byte total and percentage — the same information on
  keyboard focus as on mouse hover, per the skill's interaction
  requirement. A new shared `showChartTooltip`/`hideChartTooltip` pair
  manages one tooltip element created once and appended to `<body>` (a
  sibling of `#app`, so it survives `render()`'s full-rebuild cycle
  instead of needing per-state-change recreation — the same "bypass
  `render()` for something that shouldn't trigger a full rebuild"
  precedent `pathInput` already established for keystroke input). The
  legend now always shows the exact byte total alongside the
  percentage, not percentage alone. `el()` gained a generic `on<Event>`
  prop handler (any prop key starting with `on` whose value is a
  function is wired via `addEventListener`), replacing its previous
  single-purpose `onClick` case, so future interactions don't need a
  bespoke branch added every time. `GUI-UX-001` 0.3.9 → 0.4.0 (FR-029).
  No ADR — routine implementation, no architecture-level decision.
  Ran `KIND_COLOR`'s six existing categorical hues (used app-wide —
  chips, group-row swatches, badges, and now this chart, unchanged by
  this work) through the `dataviz` skill's `validate_palette.js`
  against both theme surfaces, as the skill's own procedure requires
  before shipping any categorical palette: found a real, pre-existing
  issue — `photo` (blue, `--accent`) and `video` (purple, `--purple`)
  fall below the normal-vision hue-separation floor (ΔE 9.8, further
  below the protanopia/tritanopia floor), genuinely hard to tell apart
  by color alone for any viewer, plus several hues sit below the
  recommended contrast/lightness band against the surface, especially
  in light mode (this palette was tuned for a dark surface first, per
  ADR-0022's design-handoff origin). This chart mitigates the finding
  specifically — every color is always paired with a text legend entry
  and a hover/focus tooltip, so nothing here is identified by hue alone
  — rather than by changing `KIND_COLOR` itself, since re-deriving the
  app-wide categorical palette (every chip/swatch/badge that uses it)
  is a whole-app design decision well beyond a "small, cheap" chart
  upgrade's scope; recorded as a disclosed, deliberate scope boundary
  in `GUI-UX-001`'s open questions rather than silently patched or
  silently ignored. Pure `app.js`/`style.css` change — no Rust code
  touched, so `cargo fmt`/`clippy -D warnings`/`test`/`bench --no-run`/
  `doc` were re-run and confirmed unaffected (221/221 tests, unchanged
  count). **Update (2026-08-27, post-merge):** manually verified through
  a rendered window after all — `xdotool` was installed in this
  environment specifically for a follow-up screenshot pass (see the
  `GUI-SCAN-LAYOUT-FIX` entry below); the chart's gaps, rounded ends,
  byte-value legend, and hover tooltip all confirmed correct against
  real scan data.

- `GUI-SCAN-LAYOUT-FIX`: not a `DEDUP-GAP-IMPLEMENTATION-PLAN.md` item —
  that plan is fully done as of `DASHBOARD-CHART-UPGRADE` above — but a
  real bug found and fixed while giving the user a real screenshot tour
  of the GUI. `xdotool` (absent all session — every prior GUI unit's
  "not yet manually verified" note was because of this) was installed
  in this environment via `apt-get`, unlocking the first actual Xvfb +
  `xdotool` interactive pass since `FOLDER-ACTION`. That pass surfaced a
  real, previously-undiscovered layout bug on Scan Setup:
  `.scan-layout`'s `flex: 1; min-height: 0` — correct when it was
  `GUI-REDESIGN`'s (0.2.0) last element on the screen, stale once
  `DETECTION-SCAN-FILTERS`/`ACTION-REFERENCE-FOLDERS`/`SCAN-PROFILES`
  each appended another card after it without revisiting that sizing
  rule — caused the side column's now-taller content (three cards'
  worth) to overflow its flex-allocated height and visually spill onto
  the cards below it, invisible until someone actually rendered the
  fully-evolved screen (nobody had, since every unit that touched it
  after `GUI-REDESIGN` individually documented "not yet manually
  verified" rather than compounding into a real check). Fixed by
  removing `flex: 1; min-height: 0` from `.scan-layout`, letting it (and
  every card after it) size to natural content height, with `.content`'s
  existing `overflow-y: auto` handling the now-taller page — confirmed
  fixed via a real screenshot (every card in its own space, full scroll
  to the "Start Scan" footer, no overlap). `GUI-UX-001` 0.4.0 → 0.4.1.
  No ADR — bug fix, not new behavior. That same pass also re-verified,
  for the first time through a rendered window, several units whose GUI
  surfaces previously had only IPC-level coverage: `SCAN-PROFILES`'s
  "Saved scan profiles" card renders correctly (save/load/delete
  interaction itself not exercised); `GUI-MEDIA-PREVIEW`'s photo
  thumbnail renders a real decoded image (audio path not exercised, no
  audio file in the demo tree); `DETECTION-PERCEPTUAL-IMAGES`'s
  "Similar content" toggle, scan, and read-only similar-images card all
  work end-to-end against a real near-duplicate image pair, correctly
  shown separately from an exact-duplicate pair in the same review list.
  Light theme confirmed correct on Scan Setup; the Dashboard's content
  region specifically didn't repaint on the theme toggle itself in this
  sandboxed environment (only after a subsequent navigation) — traced to
  this environment's software GL fallback under bare Xvfb (an EGL
  warning on launch confirms no accelerated rendering), not an app bug,
  since `render()` fully replaces the relevant DOM subtree on every
  state change, which any standards-compliant engine repaints
  deterministically. Pure `app.js`/`style.css` change (the CSS fix) —
  221/221 workspace tests unaffected, confirmed via `cargo fmt`/
  `clippy -D warnings`/`test`/`bench --no-run`/`doc`.

- `GUI-REVIEW-PANELS`: prompted by a refreshed `Deduplication app UI
  design.zip` (v2 handoff) committed to the repo root — diffed against
  the v1 handoff `GUI-REDESIGN`/ADR-0022 already reconciled and found
  most of what it newly describes (Trash/Move/Copy, protected folders,
  saved profiles, perceptual matching) was already real and ahead of the
  mockup; only a "Browse…" folder picker, a three-panel Duplicate Review
  layout, and a sidebar collapse were genuinely new. New `list_directory`
  command/`DirEntryPayload` type read the real filesystem directly (no
  new Tauri capability/permission grant) — resolving the v2 README's own
  flagged, unresolved question about the mock filesystem tree as "build
  the real browser," never a fabricated one. Duplicate Review's previous
  flat group list is now three independently-collapsible panels: a real
  file-system tree (rooted at the scan root, not the handoff's whole-disk
  `/` mock — see ADR-0031) colored by scan status computed from every
  real path a duplicate touches, a nested duplicate-group tree grouped by
  real directory hierarchy, and the existing compare/action panel with a
  new breadcrumb. `cargo fmt`/`clippy -D warnings`/`test` all green
  (225/225 workspace tests — 63 in `rusty_fclone-gui`, 4 new
  `list_directory` tests; no other crate touched). No display/`xdotool`
  toolchain in this environment this session, so frontend verification
  used a scratch, non-committed headless-Chromium (Playwright) script
  instead — driving the real `index.html`/`app.js`/`style.css` with a
  mocked `window.__TAURI__` through the Browse modal, all three Review
  panels (including a real folder-match item), panel/sidebar collapse,
  and both themes, screenshotted at each step. ADR-0031; `GUI-UX-001`
  0.4.1 → 0.4.2. Implemented, validated, and merged to `main` (PR #52,
  merge commit `51f9cb6`). See `docs/roadmap/ROADMAP.md`'s
  `GUI-REVIEW-PANELS` row and `GUI-UX-001`'s FR-030/Verification
  plan/Open questions for the full account.

## In progress
- None — `GUI-REVIEW-PANELS` above is implemented, validated, and merged
  to `main` (PR #52).

## Blocked
- None.

## Next
- `docs/roadmap/DEDUP-GAP-IMPLEMENTATION-PLAN.md` is now fully implemented
  in its entirety — all three phases, every listed unit including the
  small Dashboard chart upgrade. Nothing from the plan remains
  outstanding.
- Follow-on units intentionally left open by earlier scoping decisions
  (each needs its own design work before starting): `DETECTION-STREAMING-OVERLAP`
  proper (full pipeline overlap, needs a `ScanEvent` finality-contract
  decision first), `DETECTION-LINUX-FASTPATH` proper (io_uring/FIEMAP,
  needs an async runtime and unsafe FFI, its own ADR).
- A GUI-side SQLite reader for `CLI-SCAN-HISTORY`/`CLI-HISTORY-AUDIT`'s
  persisted history (Dashboard's "Import history"), blocked on the same
  Tauri `dialog`/`fs` plugin prerequisite as the GUI's root-path picker.
- `GUI-RELEASE-BUNDLES`: packaged, installable GUI distribution via
  `tauri build`'s bundler, needing per-platform prerequisites beyond
  CI's current build-and-test install step, plus real (non-placeholder)
  application icons.
- A native file/directory picker for the GUI's root-path field (currently
  a plain text input) — deferred pending a look at Tauri's `dialog`
  plugin's own permission/capability shape (`GUI-UX-001`'s open
  questions); the same plugin work `CLI-HISTORY-AUDIT` left "Import
  history" blocked on above.

## Validation
- `GUI-SCAN-LAYOUT-FIX` verification (2026-08-27): a real Xvfb + `xdotool`
  pass (`xdotool` newly installed via `apt-get` in this environment) —
  built the GUI binary, generated a real demo tree (`image` crate: an
  exact-duplicate JPEG pair, a separate near-duplicate PNG pair with a
  uniform +6 brightness shift, an exact-duplicate text-file pair, and a
  unique file), launched it under Xvfb, and drove it with `xdotool`
  through Dashboard → Scan Setup (confirmed the `.scan-layout` fix: every
  card renders in its own space, no overlap, full scroll to the footer)
  → a real scan with "Similar content" enabled → Duplicate Review (4
  items: 2 exact-duplicate entries, 2 similar-images entries — the
  near-duplicate pair correctly clustered by `find_similar_images`, and
  the exact-duplicate image pair correctly *also* appearing as a
  trivially-distance-0 similar match, since the perceptual pass runs
  blind to exact results by design) → clicked into a similar-images
  entry (read-only warning-banner card, both thumbnails resolved via
  `read_preview`, no action bar) → Dashboard again (real populated
  stats, the upgraded storage-breakdown chart's gaps/legend/hover
  tooltip all confirmed via screenshot) → Rules screen → light-theme
  toggle (correct on Scan Setup; Dashboard's content region needed a
  subsequent navigation to repaint in this sandboxed environment — see
  the `GUI-SCAN-LAYOUT-FIX` Completed entry above for why this isn't
  treated as an app bug). `cargo fmt --all --check`, `cargo test
  --workspace` (221/221, unchanged — pure CSS fix), `cargo clippy
  --workspace --all-targets --all-features -- -D warnings`, `cargo bench
  --workspace --no-run`, and `cargo doc --workspace --all-features
  --no-deps` all re-confirmed clean. Screenshots sent to the user;
  scratch demo-generator project and generated files removed afterward.
- `cargo fmt --all --check`: pass (2026-08-27)
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass (2026-08-27)
- `cargo test --workspace`: pass, 221/221 (2026-08-27)
- `cargo bench --workspace --no-run`: pass (2026-08-27)
- `cargo doc --workspace --all-features --no-deps`: pass (2026-08-27)
- `DASHBOARD-CHART-UPGRADE` verification (2026-08-27): pure `app.js`/
  `style.css` change, no Rust surface — `cargo fmt --all --check`,
  `cargo test --workspace` (221/221, unchanged count), `cargo clippy
  --workspace --all-targets --all-features -- -D warnings`, `cargo bench
  --workspace --no-run`, and `cargo doc --workspace --all-features
  --no-deps` were all re-run and confirmed unaffected. `node --check`
  confirmed `app.js` parses. The `dataviz` skill's `scripts/
  validate_palette.js` was run against `KIND_COLOR`'s six existing
  categorical hues (`#5b8cff` photo, `#a78bfa` video, `#fbbf24`
  document, `#34d399` audio, `#f472b6` archive, `#3a4048`/`#cbd5e1`
  other for dark/light) on both theme surfaces (`#171a1f` dark,
  `#f7f8fa` light) — both runs FAILED, with `photo`/`video` specifically
  falling below the normal-vision hue-separation floor (ΔE 9.8,
  further below the protanopia/tritanopia floor) and several hues below
  the recommended contrast-vs-surface threshold, worse in light mode.
  This is a pre-existing, whole-app palette characteristic (unaffected
  by this change, sourced from ADR-0022's design handoff, used in
  chips/swatches/badges throughout the GUI) — not something this small
  chart upgrade changed or could reasonably fix on its own; the chart
  itself mitigates the consequence by never relying on color alone
  (mandatory text legend + hover/focus tooltip on every segment). See
  `GUI-UX-001`'s open questions and the Risks entry below.
- `DETECTION-PERCEPTUAL-IMAGES` verification (2026-08-27): `image` crate
  scratch-verified in a throwaway project (`default-features = false,
  features = ["jpeg", "png", "gif", "bmp"]`) to build with zero `-sys`/
  C-linked dependencies before being adopted as a real dependency.
  `perceptual::tests::*` (11 tests, `rusty_fclone-core`) cover the dHash
  algorithm and union-find clustering directly plus `find_similar_images`
  end-to-end against real files on disk. Manual smoke test against a real
  filesystem in this environment: a small Rust program generated four
  real image files via the `image` crate — `vacation_original.jpg`, a
  brightness-shifted `vacation_reexported.jpg`, a resized
  `vacation_thumbnail.png` (all genuinely different byte content), and
  an unrelated `unrelated_photo.jpg` — and the compiled
  `rusty-fclone --find-similar-images` correctly clustered the three
  "vacation" variants together (`max distance 0/64`, correctly matching
  despite the format/resolution changes) while excluding the unrelated
  photo, in both `--format text` and `--format json`; the same run's
  ordinary output simultaneously reported `0 duplicate groups` for the
  same tree, concretely confirming the exact and perceptual engines'
  results never overlap. `commands::tests::find_similar_images_groups_a_real_near_identical_pair`
  (`rusty_fclone-gui`) re-confirms the same clustering behavior at the
  GUI's IPC boundary. Scratch verification project and generated image
  files were removed after the check.
- `SCAN-PROFILES` verification (2026-08-27): no CLI surface exists for
  this unit (GUI-only), so covered by `profiles::tests::*`'s 7 hermetic
  tempdir-based tests (empty-when-missing, insert, overwrite-by-name,
  preserving other saved profiles, remove, no-op remove of an unknown
  name, and a full `ScanOptionsPayload` round trip through the saved JSON
  file) plus a one-time manual check of the real, untestable-by-design
  directory resolution: a scratch test (removed before committing) called
  `profiles::default_profiles_dir()` for real in this environment,
  confirmed it resolved to `/root/.config/rusty-fclone`, and confirmed a
  saved-then-reloaded profile's JSON matched the exact expected shape
  (camelCase fields, `null` for unset options, `[]` for an empty
  `excludePaths`) before the scratch directory was deleted. No Xvfb/
  `xdotool` pass confirming the new "Saved scan profiles" card renders,
  saves, loads, and deletes correctly through the rendered UI — see Risks
  below.
- `GUI-MEDIA-PREVIEW` verification (2026-08-27): no traditional CLI-against-
  filesystem smoke test applies here (this unit is GUI-only, with no CLI
  surface). Verified instead via the 10 automated `preview`/`commands`
  tests (RFC 4648 base64 vectors, mime-mapping including the HEIC/TIFF/
  video exclusions, size-cap and unsupported-extension rejection, IPC round
  trips), plus a self-initiated extra check: a real 69-byte PNG file was
  base64-encoded by `build_data_url`'s hand-rolled encoder and confirmed
  byte-for-byte round-trippable through Python's own standard-library
  base64 decoder, as added assurance for hand-rolled encoding logic before
  trusting it over a well-tested crate. No Xvfb/`xdotool` pass confirming
  the `<img>`/`<audio>` elements actually render in a live window — see
  Risks below.
- Manual CLI smoke test, `CLI-HISTORY-AUDIT` (2026-08-27): two real scans
  against `/tmp/history-smoke` (one report-only, one `--action trash
  --apply` across 3 redundant files) recorded against a `--history`
  database. `history list` (both `--format text` and `--format json`)
  showed both scans newest first with the correct per-scan fields;
  `history stats` reported "2 scans, 6 files scanned (24 bytes), 2
  duplicate groups (6 files), 12 bytes reclaimed across 3 files" —
  matching the two runs exactly. Direct inspection of the `actions` table
  confirmed 3 rows, each `scan_id`-linked to the trash-and-apply scan
  (not the report-only one), with the right path/kind/bytes/succeeded
  per row. A plain `rusty-fclone <ROOT>` scan against an unrelated real
  directory confirmed the new `history` keyword dispatch doesn't affect
  normal invocations.
- Manual CLI smoke test, `ACTION-MOVE-COPY` (2026-08-27): a real two-file
  duplicate pair (`photos/img.jpg`, `backup/img.jpg`) confirmed `--action
  move --archive-dir <dir> --apply` relocated the redundant copy to its
  mirrored archive path (`<dir>/<original-path-components>/img.jpg`) and
  left the kept file untouched. A second real pair confirmed `--action
  copy --archive-dir <dir> --apply` archived a copy while leaving both
  originals in place, reporting "reclaimed 0 bytes" — then a repeated
  `copy` run against the same tree failed cleanly (`archive destination
  already exists`) without touching either original or the first run's
  archived file, confirmed via `find` before/after. `--action move`
  without `--archive-dir` was rejected with a clear error before any scan
  ran.
- Manual CLI smoke test, `ACTION-REFERENCE-FOLDERS` (2026-08-27): a real
  two-file tree (`reference/z_protected.txt`, `other/a.txt`, both
  duplicates) confirmed `--action trash --reference reference --apply`
  kept `z_protected.txt` and trashed `a.txt`, despite alphabetical
  ordering favoring `a.txt` — the text output's `keep:` line reported
  "in a protected/reference folder" as the reason. A second real tree
  (`small/1.txt` duplicated inside `big/1.txt`, `big/extra.txt`) confirmed
  `--find-duplicate-folders --action delete --reference small --apply`
  left `small/1.txt` and the `small` directory itself untouched (0 files,
  0 bytes reclaimed), instead of the folder being pruned as it normally
  would be once its one file is deleted.
- Manual CLI smoke test, `SELECTION-RULES` (2026-08-26): a real two-file
  tree with deliberately different modification times confirmed
  `--keep-rule newest` kept the newer file (not the alphabetically-first
  one) and reported the correct reason ("most recent modification time")
  in both `--format text`'s `keep:` line and `--format json`'s
  `keep_reason` field.
- Manual CLI smoke test, `ACTION-TRASH` (2026-08-26): a real two-file
  duplicate pair confirmed `--action trash --apply` moved the redundant
  copy to the OS trash (found afterward at
  `~/.local/share/Trash/files/`) and left the kept file untouched; a
  second real three-file tree (two directories, one duplicate file each)
  confirmed `--find-duplicate-folders --action trash --apply` trashed
  the subset folder's file, pruned the now-empty folder, and left the
  superset folder untouched.
- Manual CLI smoke test, `DETECTION-SCAN-FILTERS` (2026-08-26): a real
  5-file tree (one 4-way duplicate spread across a subdirectory and a
  `.tmp` file, plus one small unique file) confirmed `--min-size`,
  `--exclude-ext`, and `--exclude-path` each correctly changed which
  files were scanned and which duplicate copies were reported, matching
  the automated tests' expectations exactly.
- Manual CLI smoke tests across the project: verbosity flags, `RUST_LOG`
  override, default output silent on success, action dry runs, JSON
  format, progress checkpoints, confirmation prompt decline/accept,
  cold/warm `--cache` behavior, `--history` across two real scans,
  `--import-fclones-cache` against a real `fclones` binary and its real
  cache database (2026-08-24), `--find-duplicate-folders` against a
  real three-directory tree in both `--format text` and `--format json`
  (2026-08-25), and `--find-duplicate-folders` combined with
  `--action delete --apply` against a real filesystem — preview mode,
  real folder pruning (subset removed, superset untouched, confirmed via
  `find` before/after), and exact NDJSON field shapes (2026-08-25)
- Manual GUI smoke test, folder action (2026-08-25): compiled binary
  launched under Xvfb, driven with `xdotool` against two real trees —
  a `Contained` match (enabled "Delete Duplicate Folder" button,
  confirmation dialog text checked, real folder prune confirmed via
  `find` before/after, kept side untouched) and an `Exact` match
  (default keep-choice confirmed alphabetically-first, switched via the
  badge, confirmed the resulting delete followed the switched choice).
  Both file-level and folder-level Review screens confirmed unaffected
  by keystroke-focus or other regressions
- Manual GUI smoke test, redesigned frontend (2026-08-25): compiled
  binary launched under Xvfb, driven with `xdotool` through Dashboard
  (empty state) → Scan Setup (typed a real path, confirmed no
  keystroke-focus loss) → Start Scan → Duplicate Review (a real 4-item
  list against a tempdir with both file- and folder-level duplicates,
  including a `Contained` folder match correctly deduplicated per
  ADR-0021) → chose the non-default copy to keep → Apply Delete
  (confirmation dialog text checked, accepted, real deletion confirmed
  via `ls` before/after — the kept and removed paths matched the UI's
  displayed choice exactly) → Dashboard again (real post-scan stats,
  storage breakdown, session-scoped Recent Scans row) → Rules screen →
  light-theme toggle. Every screen rendered correctly in both themes.

## Risks and decisions needed
- `KIND_COLOR`'s six app-wide categorical hues (chips, group-row swatches,
  badges, and now the Dashboard's storage-breakdown chart) fail the
  `dataviz` skill's palette validator on both theme surfaces — most
  notably, `photo` (blue) and `video` (purple) sit below the
  normal-vision hue-separation floor, genuinely hard to tell apart by
  color alone for any viewer, not just a CVD-specific concern (see
  `DASHBOARD-CHART-UPGRADE`'s Validation entry above for the full run).
  This predates `DASHBOARD-CHART-UPGRADE` (sourced from ADR-0022's
  design handoff) and wasn't introduced or worsened by it; the chart
  mitigates its own exposure to the issue (mandatory text legend + hover/
  focus tooltip, never color-alone identification) but the underlying
  app-wide palette is unchanged and the same risk still applies anywhere
  else in the GUI a viewer would need to distinguish a photo-category
  item from a video-category one by color alone (e.g. the file-type
  filter chips on Scan Setup, or a compare-card's category swatch). A
  dedicated palette-remediation pass — informed by the skill's
  color-formula method rather than reused as-is here, since re-deriving
  it touches every chip/swatch/badge in the app — is worth doing if this
  becomes a real user-reported confusion, not something to patch
  piecemeal inside an unrelated change.
- `DETECTION-PERCEPTUAL-IMAGES`'s dHash is a similarity heuristic, not a
  cryptographic or collision-resistant hash — this project's "zero false
  positives" claim continues to apply exclusively to the exact,
  hash-verified engine (`scan()`/`DuplicateGroup`) and was never extended
  to `SimilarGroup`, deliberately (ADR-0030). The default 10/64
  Hamming-distance threshold is a commonly-cited starting point, not
  independently tuned against a labeled dataset of real "same photo,
  different export" pairs versus true near-misses — worth revisiting if
  real usage surfaces it as too loose or too strict.
- `find_similar_images`'s clustering is pairwise (O(n²) Hamming-distance
  comparisons across every decoded image in the tree) — a deliberate
  simplicity-over-scale tradeoff for a first, opt-in version, not
  benchmarked against a real large photo library. Revisit only if real
  usage shows this is actually a bottleneck.
- `find_similar_images` only decodes JPEG/PNG/GIF/BMP (the `image` crate
  features enabled — chosen specifically to avoid any C-linked codec, per
  `AGENTS.md`'s no-C-toolchain rule) — WebP, AVIF, HEIC, and TIFF images
  are silently invisible to this pass (excluded by the extension filter,
  never attempted), narrower than `GUI-MEDIA-PREVIEW`'s own preview
  support.
- The GUI's "Similar content" wiring (the enabled seg-option,
  `similarReviewMain`, the extended `reviewItems`/`groupListRow`
  dispatch) has IPC-level test coverage for the underlying
  `find_similar_images` command, and was confirmed end-to-end through a
  real rendered window in `GUI-SCAN-LAYOUT-FIX`'s Xvfb + `xdotool` pass
  (2026-08-27) — selecting it, running a real scan, and the resulting
  card (banner, thumbnails, no action bar) all worked as designed. No
  similarity-threshold control is exposed in the GUI yet (always the
  10/64 default); the CLI's `--similarity-threshold` is the only tunable
  surface today.
- `SCAN-PROFILES`'s "Saved scan profiles" card was confirmed rendering
  correctly (name field, save button, empty-state text) in
  `GUI-SCAN-LAYOUT-FIX`'s Xvfb + `xdotool` pass (2026-08-27) — but that
  pass didn't actually save, load, or delete a profile through the
  rendered UI, so that specific interaction remains unverified
  end-to-end (hermetic unit-test coverage for the underlying storage
  logic and IPC-level coverage for `save_scan_profile`'s name validation
  still stand in for it). Separately, `profiles::default_profiles_dir()`
  (the real OS-config-directory lookup, via the `dirs` crate) has no
  automated test by design — exercising it for real would write into the
  host machine's actual config directory rather than a hermetic tempdir —
  and was instead verified once by hand (see Validation above); this is
  the same category of trust ADR-0014/ADR-0024 already established for
  `trash`/reflink's non-Linux platform behavior, now applied to path
  resolution instead of a destructive filesystem operation.
- `GUI-MEDIA-PREVIEW`'s rendered behavior (thumbnail/audio-player display
  in Duplicate Review's compare-cards) has IPC-level test coverage for
  `read_preview` and payload conversion, and the photo/`<img>` path was
  confirmed rendering a real decoded image through the rendered UI in
  `GUI-SCAN-LAYOUT-FIX`'s Xvfb + `xdotool` pass (2026-08-27) — the audio/
  `<audio controls>` path specifically wasn't exercised in that pass (no
  audio file in the demo tree), so it remains unverified end-to-end. The
  hand-rolled base64 encoder is additionally verified via RFC 4648 test
  vectors and a real-file round trip (see Validation above).
- `CLI-HISTORY-AUDIT`'s `history` keyword reservation means a real
  directory literally named `history` at a scan root now needs
  `rusty-fclone ./history` (or an absolute path) to disambiguate from the
  subcommand — narrow, documented in ADR-0027, not expected to matter in
  practice, but worth knowing if a user ever reports a confusing "unknown
  argument" error for a `history`-named directory.
- `CLI-HISTORY-AUDIT`'s GUI surface (`exportScanHistoryJson`'s `<a
  download>`/object-URL mechanism) has no automated test coverage (no JS
  test harness in this project) and no Xvfb/`xdotool` end-to-end pass
  confirming a real download actually completes through Tauri's webview
  — this technique is standard in ordinary browsers, but hasn't been
  confirmed specifically against this project's Tauri v2 webview
  configuration in this environment (no display available). If a real
  session reports the button doing nothing, this is the first place to
  check.
- `ACTION-MOVE-COPY`'s GUI surface (the conditional "Archive folder"
  field, and the `"copy"`-specific reclaim-text rewrite) has IPC-level
  test coverage for `run_action`/`run_folder_action`'s new `archiveDir`
  parameter, but no Xvfb/`xdotool` end-to-end pass confirmed the field
  actually appears/disappears correctly as the action-kind selector
  changes, or that Apply's disabled state reacts to it, through the
  rendered UI — `xdotool` isn't installed in this environment. The
  underlying core logic is fully unit-tested and additionally confirmed
  via real CLI smoke tests against a real filesystem (see Validation
  above), so the residual risk is UI wiring specifically, same shape as
  the other GUI surfaces below.
- `ActionKind` no longer derives `Copy` (the Rust trait) as of
  `ACTION-MOVE-COPY` — every call site that used to rely on an implicit
  copy was updated to clone or borrow explicitly, verified by the full
  existing test suite passing unmodified for the four pre-existing
  variants. Worth a second look if a future `ActionKind`-touching change
  reintroduces an implicit-copy assumption the compiler won't always
  catch as cleanly as it did here (most sites were straightforward
  borrow-checker errors, not silent behavior changes).
- `ACTION-REFERENCE-FOLDERS`'s GUI surface (the new "Protected folders"
  field) has IPC-level test coverage for `run_action`/`choose_keep`/
  `run_folder_action`'s new `referencePaths` parameter, but no Xvfb/
  `xdotool` end-to-end pass confirmed a protected file actually shows the
  right "keeping this file" badge and survives Apply through the rendered
  UI — `xdotool` isn't installed in this environment. The underlying
  core logic is fully unit-tested and additionally confirmed via real CLI
  smoke tests against a real filesystem (see Validation above), so the
  residual risk is UI wiring specifically, same shape as the other GUI
  surfaces below.
- `SELECTION-RULES`'s GUI surface (the real "Keep newest copy" toggle) has
  IPC-level test coverage for the new `choose_keep` command and
  `run_action`'s `keepReason` handling, but no Xvfb/`xdotool` end-to-end
  pass confirmed the toggle actually changes which file is highlighted
  and kept through the rendered UI. The async resolve-on-render pattern
  (`ensureRuleKeepChoice`) is also new to this codebase's frontend —
  worth a closer look the first time it's exercised through a real
  window, since it's the first place `app.js` fetches backend data
  outside of a scan-event stream or a direct user action.
- `ACTION-TRASH`'s GUI surface (the new "Trash" option in the action-kind
  selector, now the default) has automated coverage at the unit-test
  level only (`parse_action_kind`) — no Xvfb/`xdotool` end-to-end pass
  confirmed selecting it and applying it actually trashes a file through
  the rendered UI. The CLI path is manually verified against real
  filesystems (see Validation above); the underlying core/folder-action
  logic reused by both is fully unit-tested either way, so the residual
  risk is UI wiring specifically, not the action itself.
- `trash::delete`'s behavior on Windows/macOS is entirely delegated to and
  trusted from the `trash` crate's own platform-specific implementations
  (Recycle Bin Shell API, Finder Trash) — not independently re-verified in
  this environment, which only exercises the Linux freedesktop.org trash
  path. Same posture ADR-0014 already took for reflink's non-Linux paths.
- `DETECTION-SCAN-FILTERS`'s GUI surface (Scan Setup's new "Include/exclude
  filters" card) has automated coverage at the payload-conversion level
  only — no Xvfb/`xdotool` end-to-end pass confirmed the fields actually
  render, accept input, and change a real scan's results through the
  rendered UI (every other GUI unit in this project has had one). Also,
  no dedicated CLI-level test asserts a non-default `--min-size`/
  `--exclude-ext`/etc. value end-to-end (only a real manual smoke test —
  see Validation above); this matches the existing pattern for `--cache`/
  `--io-threads`, neither of which has a dedicated CLI-level test either.
- The action layer is the first genuinely destructive capability in this
  codebase. Its safety model (dry-run default, two-flag confirmation, plus
  the interactive confirmation prompt) is documented and tested, but has
  not been used against a real, valuable directory tree outside this
  session's smoke tests — treat it with appropriate caution before
  pointing it at anything you care about.
- `FOLDER-ACTION`'s blast radius is strictly larger than a single-group
  action (every file under a folder, plus the directory itself on a
  successful delete) — untested against a real, valuable directory tree.
  Both the CLI and GUI can now exercise it end-to-end (manually
  smoke-tested against real filesystems in each). Its directory-prune
  race (something else creates a file under `removed` between the last
  successful per-file delete and the `fs::remove_dir_all` prune) is a
  defined, handled outcome (`directory_removed: false`) but isn't
  covered by a test — narrow enough that reproducing it deterministically
  would need real concurrency (ADR-0023's consequences).
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
- `GUI` was originally only verified on Linux (this environment's only
  available platform) — **now also confirmed rendering correctly on a
  real Windows desktop**, via an actual user building and launching it
  (window chrome, layout, and controls all correct; a real scan against
  a real directory produced the expected duplicate-group listing). macOS
  is still entirely unverified. No automated frontend/DOM test exists
  (`app.js` is covered by the manual Xvfb pass and this real Windows use
  only); `GUI-UX-001`'s open questions track this. Four real gaps
  surfaced by real usage so far, all since fixed:
  - Building: the MSVC C++ toolchain prerequisite for `embed-resource`
    wasn't documented (README, ADR-0020; `GUI-UX-001` 0.1.1).
  - Building: a missing `icons/icon.ico` blocked the build outright, not
    just release bundling as originally assumed (`GUI-UX-001` 0.1.2).
  - Building: after switching to the GNU toolchain (no admin rights
    available to install MSVC), the crate's unused `cdylib` output
    overflowed MinGW's classic linker's export-ordinal field on Tauri's
    large dependency tree — trimmed `[lib] crate-type` down to just
    what's needed (`GUI-UX-001` 0.1.3).
  - **Using** (the first gap found by actually running the app, not just
    building it): pasting a path copied via Windows Explorer's "Copy as
    path" (which wraps it in double quotes) into the root-path field
    failed with "root path does not exist" — the quotes were literal
    characters in the string. Fixed by stripping surrounding whitespace
    and one layer of matching quotes from every user-typed path field
    (`GUI-UX-001` 0.1.4, FR-012).

  `.icns` (macOS) is still missing and could carry the same "blocks
  debug builds too" risk `icon.ico` did; genuinely unverified, since no
  macOS build attempt has happened yet.
  - The Duplicate Review screen's cards don't visually pick up the light
    theme correctly (Dashboard and Scan Setup do) — noticed during
    `FOLDER-ACTION` (GUI wiring)'s manual verification pass, affecting
    both the pre-existing file-level card and the new folder-level one
    equally, so it predates that change. Not investigated or fixed yet
    (`GUI-UX-001`'s open questions); contradicts `GUI-REDESIGN`'s own
    manual pass, which reported every screen rendering correctly in both
    themes — worth a dedicated look.
- `GUI-RELEASE-BUNDLES` (packaged, installable GUI distribution) is not
  started — `release.yml` still only builds the CLI binary.
