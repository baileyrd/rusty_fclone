# Project Status
- Last verified main commit: `43c3bb6` — merged `ACTION-REFERENCE-FOLDERS`
  (PR #40), Phase 2's first unit of `docs/roadmap/
  DEDUP-GAP-IMPLEMENTATION-PLAN.md`. This branch (`action-move-copy`)
  implements that plan's Phase 2, second unit: `ACTION-MOVE-COPY`.
- Tagged: `v0.1.0` at commit `b616294`, GitHub Release published with all
  four platform archives attached (verified via the GitHub API after
  `.github/workflows/release.yml`'s first real dispatch succeeded — see
  `docs/decisions/ADR-0018-release-binaries.md`). `v0.2.0` pending — the
  workspace version was bumped to `0.2.0` but the tag itself hasn't been
  pushed yet (tag pushes require a maintainer's own credentials in this
  environment); everything merged since `v0.1.0` will be tagged once that
  happens.
- Verified at: 2026-08-27
- Current milestone: `ACTION-MOVE-COPY` (Phase 2 of
  `DEDUP-GAP-IMPLEMENTATION-PLAN.md`, second of three independent units) —
  implemented, validated, not yet merged. See `docs/roadmap/ROADMAP.md`.
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

## In progress
- None — `ACTION-MOVE-COPY` above is implemented and validated on branch
  `action-move-copy`, not yet merged.

## Blocked
- None.

## Next
- Phase 2 of `docs/roadmap/DEDUP-GAP-IMPLEMENTATION-PLAN.md` continues
  once `ACTION-MOVE-COPY` merges. Its remaining unit, `CLI-HISTORY-AUDIT`,
  is independent and can start next.
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
- A GUI-side reader for `CLI-SCAN-HISTORY`'s persisted SQLite history,
  and a real file-export path for the Dashboard's "Import history"/
  "Export (JSON)" buttons — both need new backend work; they ship
  disabled with an explanatory tooltip in `GUI-REDESIGN`.

## Validation
- `cargo fmt --all --check`: pass (2026-08-27)
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass (2026-08-27)
- `cargo test --workspace`: pass, 172/172 (2026-08-27)
- `cargo bench --workspace --no-run`: pass (2026-08-27)
- `cargo doc --workspace --all-features --no-deps`: pass (2026-08-27)
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
