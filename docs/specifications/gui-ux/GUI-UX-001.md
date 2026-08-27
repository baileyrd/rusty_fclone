# GUI-UX-001 — Desktop GUI (Tauri)
- Version: 0.3.4
- Status: Implemented (v1)
- Owners: baileyrd
- Depends on: `FCLONE-DETECTION-001`, `FCLONE-ACTION-001`
- Supersedes: none

## Purpose and scope

A desktop GUI (`rusty_fclone-gui`, ADR-0020) for scanning a directory,
reviewing duplicate groups as they're found, and previewing or applying an
action on each group — the same capability `rusty_fclone-cli` already
provides, through a Tauri window instead of a terminal. This spec covers
the GUI's own Rust commands, wire format, and frontend behavior; it does
not restate `FCLONE-DETECTION-001`/`FCLONE-ACTION-001`'s engine-level
semantics, which are unchanged.

## Non-goals

- Any change to detection or action semantics — same boundary `CLI-UX-001`
  already draws for the CLI (ADR-0005: core stays UI-agnostic, of either
  kind).
- A native file/directory picker dialog — the scan root is a plain text
  field in v1 (see Open questions).
- Batch actions across multiple duplicate groups at once — each group's
  action is planned/applied independently, one `run_action` call per
  group, matching how a user reviews groups one at a time.
- Packaged, installable distribution (`.deb`/`.AppImage`/`.dmg`/`.msi`) —
  `release.yml` doesn't build GUI bundles yet; see
  `docs/roadmap/ROADMAP.md`'s `GUI-RELEASE-BUNDLES`.
- Persisting GUI-side state (window size/position, last-used scan root or
  options) across launches.
- Any GUI-specific safety relaxation of the action layer's dry-run-by-
  default model (ADR-0009) — apply is never implicit; see FR-011 for the
  current (confirmation-dialog) mechanism.
- Scanning more than one root directory in a single scan — matches
  `rusty_fclone_core::scan`'s one-root contract exactly (ADR-0022).
- Near-duplicate/fuzzy matching ("Similar content" in the UI) — an
  explicit `FCLONE-DETECTION-001` non-goal; the control is shown but
  disabled, not silently ignored (ADR-0022).
- Batch actions across multiple folder matches at once, or across every
  pair within one `Exact` cluster in a single `invoke` call — the
  frontend loops `run_folder_action` once per `removed`/`kept` pair
  (FR-018), same one-call-per-unit-of-work model FR-008 already uses for
  file groups.
- Rules & Automation actually applying to a scan — the screen is a
  local, unpersisted preview only; no rule engine exists in
  `rusty_fclone_core` (ADR-0022).
- Reproducing the design handoff's fake OS window chrome (rounded
  corners, drop shadow, macOS traffic-light titlebar) — the real Tauri
  window already provides real chrome; that wrapper existed only to
  display the mockup on an infinite design canvas (ADR-0022).

## Context and terminology

- **Tauri command**: a `#[tauri::command]`-annotated Rust function,
  invoked from the frontend via `window.__TAURI__.core.invoke`.
- **Tauri event**: a backend-to-frontend push, via `AppHandle::emit`,
  received in JS via `window.__TAURI__.event.listen`. Used here for
  `scan-event`, the streaming duplicate-group/progress/error/finished
  feed described below.
- **Preview vs. apply**: same distinction as the CLI's `--action` (preview
  only) vs. `--action ... --apply` (actually mutates). The GUI's
  `run_action` command takes an explicit `apply: bool` for this, rather
  than two separate commands.

## Requirements

- `GUI-UX-001-FR-001`: The frontend SHALL invoke a `start_scan` command
  with the root path and scan options; the command SHALL start the scan
  on a background thread and return immediately (not block until the scan
  completes).
- `GUI-UX-001-FR-002`: For each `rusty_fclone_core::ScanEvent` the scan
  produces, the backend SHALL emit exactly one `scan-event` Tauri event,
  in the order the engine produced them (ADR-0004's streaming/ordering
  contract, carried across the IPC boundary).
- `GUI-UX-001-FR-003`: A `scan-event` for `ScanEvent::DuplicateGroup`
  SHALL have the shape
  `{"type":"duplicate_group","size":<u64>,"paths":[<string>,...]}`,
  matching `CLI-UX-001-FR-002`'s duplicate-group fields (minus the
  CLI-only inline `action`, which the GUI requests separately via
  `run_action`).
- `GUI-UX-001-FR-004`: A `scan-event` for `ScanEvent::Error` SHALL have the
  shape `{"type":"error","path":<string>,"message":<string>}`, matching
  `CLI-UX-001-FR-003`.
- `GUI-UX-001-FR-005`: A `scan-event` for `ScanEvent::Progress` SHALL have
  the shape
  `{"type":"progress","filesScanned":<u64>,"bytesScanned":<u64>}`.
- `GUI-UX-001-FR-006`: A `scan-event` for `ScanEvent::Finished` SHALL have
  the shape
  `{"type":"finished","filesScanned":<u64>,"bytesScanned":<u64>,"duplicateGroups":<u64>,"duplicateFiles":<u64>}`
  and SHALL be the last `scan-event` for that scan.
- `GUI-UX-001-FR-007`: The frontend SHALL provide controls for every
  `ScanOptions` field (`followSymlinks`, `crossFilesystems`,
  `verifyMatches`, `smallFileThreshold`, `partialHashSampleSize`,
  `ioThreads`, `cachePath`, `fclonesImportPath`); an omitted/empty numeric
  or path field SHALL use the engine's own default (i.e. the frontend
  SHALL NOT hardcode a different default than `ScanOptions::default()`).
- `GUI-UX-001-FR-008`: The frontend SHALL invoke a `run_action` command
  with a duplicate group (`size`, `paths`), an action kind
  (`"delete"|"trash"|"hardlink"|"reflink"`), an optional `keepReason`
  string, and `apply: bool`; the backend SHALL call `action::plan`
  unconditionally and SHALL call `action::apply` if and only if `apply` is
  `true`. `keepReason` SHALL default to a placeholder when omitted
  (`SELECTION-RULES` — the frontend resolves both the kept path, via
  reordering `paths`, and the reason before calling this command; the
  backend has no independent way to know "why").
- `GUI-UX-001-FR-009`: `run_action`'s response SHALL include the plan
  (`kept`, `keepReason`, `planned` paths, `bytesReclaimed`) always, and
  the apply report (`succeeded`, `failed`, `bytesReclaimed`) if and only
  if `apply` was `true`.
- `GUI-UX-001-FR-010`: `run_action` SHALL reject an action kind outside
  `{"delete","trash","hardlink","reflink"}` with an `Err`, without calling `plan`
  or `apply`.
- `GUI-UX-001-FR-011`: The frontend SHALL require an explicit, separate
  confirmation step — naming the action, the number of files affected,
  and the bytes to be reclaimed — before invoking `run_action` with
  `apply: true` for a file duplicate group; declining that confirmation
  SHALL NOT call `run_action` at all. (Revised in 0.2.0: the original
  mechanism was an apply checkbox defaulting to unchecked on every
  newly rendered group; the redesigned single-button Review screen has
  no such checkbox, so a confirmation dialog is the mechanism instead.
  The underlying requirement — apply is never implicit — is unchanged.)
- `GUI-UX-001-FR-012`: `start_scan`'s root path and `ScanOptionsPayload`'s
  `cachePath`/`fclonesImportPath` SHALL each be trimmed of surrounding
  whitespace and, if present, one layer of surrounding matching quote
  characters (`"..."` or `'...'`) before use — so a path copied via
  Windows Explorer's "Copy as path" (which wraps it in double quotes)
  works when pasted directly into any of these fields. A path with only
  one matching side quoted (e.g. a genuine leading `"` with no trailing
  one) SHALL be left unchanged rather than guessed at.
- `GUI-UX-001-FR-013`: The frontend SHALL be able to invoke a
  `find_duplicate_folders` command with the scan root, the full set of
  `DuplicateGroup`s a prior `start_scan` produced (`size`/`paths`, the
  same shape `run_action` already accepts), and the scan options; the
  backend SHALL call `rusty_fclone_core::find_folder_duplicates` with
  them and return its `FolderMatch` results, each shaped as
  `{"type":"exact","folders":[<string>,...],"fileCount":<u64>,"bytes":<u64>}`
  or
  `{"type":"contained","subset":<string>,"superset":<string>,"fileCount":<u64>,"bytes":<u64>}`.
  `root` SHALL go through the same quote/whitespace normalization as
  FR-012. A nonexistent/non-directory root SHALL be rejected with an
  `Err`, matching `find_folder_duplicates`'s own `ScanError::InvalidRoot`
  contract.
- `GUI-UX-001-FR-014`: The frontend SHALL render four screens (Dashboard,
  Scan Setup, Duplicate Review, Rules & Automation) selected by
  client-side navigation state, not separate windows or page loads.
  Dashboard, Scan Setup, and Duplicate Review SHALL be driven entirely
  by real data from `start_scan`/`run_action`/`find_duplicate_folders` —
  no mock/sample data. Rules & Automation's "Ignore tiny files" and
  "Auto-clean Downloads" toggles SHALL remain a local-only preview
  (toggle state held in frontend memory, reset on relaunch) with no
  backend persistence or scan-time effect, and SHALL say so in the
  screen's own UI text (ADR-0022). "Keep newest copy" is the one
  exception, reversed by `SELECTION-RULES`: see FR-022.
- `GUI-UX-001-FR-015`: Once every `scan-event` for a scan has been
  received (a `Finished` event) and at least one `DuplicateGroup` was
  found, the frontend SHALL automatically invoke `find_duplicate_folders`
  with that scan's root, the groups it collected, and the scan options,
  and SHALL merge the results into the Duplicate Review list without a
  separate user-initiated trigger.
- `GUI-UX-001-FR-016`: The Duplicate Review screen SHALL let the user
  choose which path in a file duplicate group is treated as kept; the
  frontend SHALL implement this by reordering that group's `paths` so
  the chosen path is first before calling `run_action` (matching
  `action::plan`'s existing "first path is kept" contract), without any
  `rusty_fclone_core` API change.
- `GUI-UX-001-FR-017`: Scan Setup's file-type filter controls SHALL only
  affect which already-found duplicate groups the Duplicate Review
  screen displays (client-side, by file extension) and SHALL NOT be sent
  to `start_scan` or otherwise change what the engine scans or reports.
- `GUI-UX-001-FR-018`: The frontend SHALL be able to invoke a
  `run_folder_action` command with a `removed`/`kept` folder path pair,
  the full set of `DuplicateGroup`s the originating scan produced, the
  scan options, an action kind (`"delete"|"trash"|"hardlink"|"reflink"`), and
  `apply: bool`; the backend SHALL call
  `rusty_fclone_core::folder_action::plan_folder` unconditionally and
  SHALL call `apply_folder` if and only if `apply` is `true` — the
  folder-level counterpart of FR-008/FR-009/FR-010 (ADR-0023). A stale or
  unconfirmed pair (`plan_folder`'s `Err`) SHALL be surfaced as a
  rejected `invoke` promise without calling `apply_folder`.
- `GUI-UX-001-FR-019`: For a `FolderMatch::Contained` match, the Duplicate
  Review screen's action button SHALL be enabled and, when confirmed,
  SHALL call `run_folder_action` with `subset` as `removed` and
  `superset` as `kept` — no keep-choice control, matching the CLI's
  `folder_match_pairs` convention (`CLI-UX-001` FR-013). For a
  `FolderMatch::Exact` cluster, the screen SHALL let the user choose
  which folder is kept (defaulting to the alphabetically-first one, same
  convention as the CLI) via a per-folder badge, mirroring FR-016's
  file-level keep-choice mechanism, and SHALL call `run_folder_action`
  once per non-kept folder in the cluster when confirmed. Confirming
  SHALL require the same explicit confirmation-dialog step FR-011
  requires for file groups (naming the action, folder and file counts,
  and bytes to be reclaimed) before any `run_folder_action` call with
  `apply: true`.
- `GUI-UX-001-FR-020`: Scan Setup SHALL expose real, wired fields for
  `FCLONE-DETECTION-001`'s scan filters (min/max size, include/exclude
  extensions, exclude paths) in a dedicated "Include/exclude filters"
  card, sent to `start_scan` as part of `ScanOptionsPayload` — distinct
  from the Rules & Automation screen's toggles (FR-014's own scope,
  explicitly local-only preview, unaffected by this requirement)
  (`DETECTION-SCAN-FILTERS`). A blank field SHALL map to `ScanOptions`'s
  own "no filtering" default (`None`/empty), never to a zero value.
- `GUI-UX-001-FR-021`: The Duplicate Review and folder-review screens'
  action-kind selector SHALL default to `"trash"`, not `"delete"` —
  recoverable-by-default matches this project's stated safety posture
  (ADR-0009) better than a permanent-delete default. `"delete"` SHALL
  remain selectable as an explicit choice, unchanged in behavior
  (`ACTION-TRASH`, ADR-0024).
- `GUI-UX-001-FR-022`: The frontend SHALL be able to invoke a `choose_keep`
  command with a duplicate group and a rule name
  (`"alphabetical"|"newest"|"oldest"|"shortest_path"|"longest_path"`); the
  backend SHALL call `select::choose_keep` and return the chosen path and
  its one-line reason, without planning or applying anything. The Rules &
  Automation screen's "Keep newest copy" toggle SHALL be real (not local-
  only preview, unlike the screen's other two toggles): enabling it SHALL
  call `choose_keep` with rule `"newest"` for every group in Duplicate
  Review that has no manual keep-choice override, and use its result as
  that group's default kept path and displayed reason. A manual
  keep-choice badge SHALL always take precedence over the rule
  (`SELECTION-RULES`).
- `GUI-UX-001-FR-023`: Scan Setup SHALL expose a real, wired "Protected
  folders" field, sent as `referencePaths` to the `run_action`,
  `choose_keep`, and `run_folder_action` commands (not `start_scan` or
  `find_duplicate_folders`, since detection itself doesn't need it) — an
  empty list SHALL be identical to no guardrail. The Duplicate Review
  screen's rule-preview lookup SHALL also resolve via `choose_keep`
  whenever at least one reference folder is configured, even under the
  default `"alphabetical"` rule, so the "keeping this file" badge
  reflects the guardrail before Apply rather than only after
  (`ACTION-REFERENCE-FOLDERS`, ADR-0025).

## Architecture and interfaces

`rusty_fclone-gui` (new crate, ADR-0005/ADR-0020):

```rust
// src/commands.rs
#[tauri::command]
fn start_scan<R: Runtime>(app: AppHandle<R>, root: String, options: ScanOptionsPayload) -> Result<(), String>;
#[tauri::command]
fn run_action(group: GroupPayload, kind: String, keep_reason: Option<String>, apply: bool,
              reference_paths: Vec<String>) -> Result<ActionResultPayload, String>;
#[tauri::command]
fn choose_keep(group: GroupPayload, rule: String, reference_paths: Vec<String>) -> Result<ChooseKeepPayload, String>;
#[tauri::command]
fn find_duplicate_folders(root: String, groups: Vec<GroupPayload>, options: ScanOptionsPayload) -> Result<Vec<FolderMatchPayload>, String>;
#[tauri::command]
fn run_folder_action(removed: String, kept: String, groups: Vec<GroupPayload>, options: ScanOptionsPayload, kind: String, apply: bool,
                      reference_paths: Vec<String>) -> Result<FolderActionResultPayload, String>;

// src/payload.rs — serde DTOs, kept out of rusty_fclone-core (ADR-0020)
struct ScanOptionsPayload { /* mirrors ScanOptions, all fields optional */ }
enum ScanEventPayload { DuplicateGroup { .. }, Error { .. }, Progress { .. }, Finished(ScanSummaryPayload) }
struct GroupPayload { size: u64, paths: Vec<String> }
struct ActionResultPayload { plan: ActionPlanPayload, applied: Option<ApplyReportPayload> }
enum FolderMatchPayload { Exact { folders: Vec<String>, fileCount: u64, bytes: u64 },
                          Contained { subset: String, superset: String, fileCount: u64, bytes: u64 } }
struct FolderActionResultPayload { plan: FolderActionPlanPayload, applied: Option<FolderApplyReportPayload> }
```

Frontend (`ui/app.js`): `folderMatchPairs(item)` mirrors the CLI's
`folder_match_pairs` — a `Contained` match always yields the single
`[{removed: subset, kept: superset}]` pair; an `Exact` cluster yields one
pair per non-kept folder, the kept folder read from `state.keepChoice`
(defaulting to the alphabetically-first folder, matching the CLI's
`folders.iter().min()`) — the same `state.keepChoice` map FR-016 already
uses for file groups, keyed the same way (`item.key`). `applyFolderAction`
loops one `run_folder_action` call per pair sequentially, summing
`bytesReclaimed`/`failed` across calls for the single post-action message
(FR-018's one-call-per-pair contract).

Frontend (`ui/`, plain HTML/CSS/JS, no bundler, no framework —
`tauri.conf.json`'s `app.withGlobalTauri: true`; rebuilt in 0.2.0 against
the design handoff, ADR-0022): `index.html` (a single `#app` mount
point), `icons.js` (a fixed set of inline-SVG icon builders, `icon(name,
size)`), `app.js` (all state, rendering, and `invoke`/`listen` wiring —
a single in-memory `state` object, a `render()` that rebuilds the DOM
from it on every state change except free-text input, and one render
function per screen), `style.css` (CSS custom properties for the
dark/light design tokens, one component class per UI primitive). Every
`app.js` render helper builds elements via `document.createElement`/
`textContent` for anything derived from scan data (paths, error
messages) — never `innerHTML` string interpolation — so a maliciously
named file can't inject markup into the page; `icons.js`'s fixed,
hardcoded SVG strings are the only `innerHTML` use in the frontend.

## Data/state and invariants

- Same as `CLI-UX-001`'s: `ScanProgress`'s counters are cumulative, not
  deltas; `Finished` is always the last `scan-event` for a scan (FR-006).
- The frontend holds all state (scan options, collected duplicate groups,
  folder matches, view/navigation, theme, session-scoped scan history) in
  one in-memory `state` object (`app.js`) — no separate store, no
  re-fetch from the backend between receiving a `duplicate_group` event
  and calling `run_action` on it. None of it persists across a relaunch
  (ADR-0022) — including the session-scoped Dashboard/Recent-Scans data,
  which is real but not written anywhere.
- Free-text inputs (scan root, cache path, fclones-import path) mutate
  `state` directly on every keystroke without triggering the normal
  full-DOM `render()` — a full re-render on every keystroke would drop
  input focus, since `render()` replaces the DOM subtree wholesale
  rather than patching it. Every other state change (toggles, chip
  selection, navigation) does trigger a full `render()`.
- Multiple concurrent scans are not prevented at the command level
  (`start_scan` has no "already running" guard); the frontend's own Scan
  button is disabled while a scan is in flight as its only guard against
  this, matching the CLI's single-scan-per-process model in practice but
  not as a backend-enforced invariant.
- A successful `run_action`/`run_folder_action` does not remove the
  acted-on group/match from `state.groups`/`state.folderMatches` — the
  Duplicate Review list reflects what the scan found, not live
  post-action filesystem state, same precedent FR-008's `run_action`
  already established (`sessionBytesReclaimed` and the per-item status
  badges are what reflect the action's outcome instead).

## Errors, failure, recovery, and observability

- A `scan()` failure to start (invalid root) is reported as one
  `scan-event` of `type: "error"` with an empty `path`, rather than
  failing `start_scan`'s own `Result` — since `start_scan` has already
  returned by the time the background thread would discover this,
  matching the "results stream as events" model FR-002 establishes.
- Per-file scan errors (`ScanEvent::Error`) don't stop the scan or
  disable further group rendering — same error-tolerance contract as the
  engine itself (ADR-0004), just carried through unchanged.
- `run_action` returning `Err` (unknown kind) is surfaced to the frontend
  as a rejected `invoke` promise; the frontend displays it inline on that
  group's card rather than a global error banner, so one group's failure
  doesn't interrupt review of the others.
- `run_folder_action` returning `Err` (a stale/unconfirmed pair, per
  `plan_folder`'s fail-closed re-verification, ADR-0023) stops
  `applyFolderAction`'s pair loop at that point rather than continuing to
  the remaining pairs — any pairs already applied earlier in the loop
  keep their effect (per-pair actions, once applied, aren't rolled back);
  the partial result (bytes reclaimed so far, plus the error) is shown in
  the same inline status message FR-011's confirmation flow already uses.

## Security, privacy, and compatibility

- No new filesystem access beyond what `rusty_fclone-core` already
  performs — the GUI backend is a thin translation layer, not a second
  implementation of scanning or acting.
- `tauri.conf.json`'s `capabilities/default.json` grants only
  `core:default` (window/webview basics) — `start_scan`/`run_action` are
  plain application commands, not gated by Tauri's plugin ACL system
  (verified via `tauri::test`'s mock IPC harness, which uses an empty
  resolved-ACL context and still successfully invokes both commands).
- Path fields are JSON strings built from `.display().to_string()`, the
  same lossy-for-non-UTF-8-paths tradeoff `CLI-UX-001` already accepts.

## Acceptance criteria

- FR-001/FR-002 (streaming, non-blocking `start_scan`) are exercised by
  `commands::tests::start_scan_accepts_a_valid_root_and_returns_immediately`
  (IPC-level, via `tauri::test`) and by a manual end-to-end pass (real
  compiled binary, Xvfb + `xdotool`): a real scan against a tempdir with a
  known duplicate pair rendered the expected group.
- FR-003 through FR-006 (event shapes) are exercised by
  `payload::tests::scan_event_payload_serializes_with_a_snake_case_type_tag`
  and `payload::tests::duplicate_group_converts_to_a_scan_event_payload_with_display_paths`
  (shape/casing), plus the manual end-to-end pass confirming the frontend
  correctly parses real emitted events (status line and group card
  rendered with the right numbers).
- FR-007 (options mirror `ScanOptions`) is exercised by
  `payload::tests::scan_options_payload_applies_defaults_for_omitted_fields`.
- FR-008 through FR-010 (`run_action` contract) are exercised by
  `commands::tests::run_action_delete_preview_does_not_touch_the_filesystem`,
  `commands::tests::run_action_delete_apply_removes_the_redundant_copy`,
  and `commands::tests::run_action_rejects_an_unknown_kind` — all IPC-level
  via `tauri::test::get_ipc_response`, asserting on real filesystem state
  before/after, not just the response shape. The apply path was
  additionally confirmed manually against the redesigned frontend: a
  real file was deleted through the rendered UI's confirmation dialog +
  Apply button, verified against the filesystem directly (both the kept
  and removed path matched the UI's displayed choice).
- FR-011 (apply confirmation gate) is exercised by the manual end-to-end
  pass: the confirmation dialog appeared with the correct action/file
  count/byte text before any mutation, and only proceeded after
  accepting it; no automated DOM-level test exists (see Open questions).
- FR-012 (quote/whitespace normalization) is exercised by
  `payload::tests::normalize_path_input_*` (5 tests: double quotes,
  single quotes, whitespace inside and outside the quotes, an unquoted
  path left alone, a mismatched single quote left alone) and
  `payload::tests::scan_options_payload_normalizes_pasted_quoted_paths_too`
  (the same normalization applied through `ScanOptionsPayload`'s
  `cachePath`/`fclonesImportPath`), plus
  `commands::tests::start_scan_treats_a_windows_copy_as_path_quoted_root_as_the_real_directory`
  — an IPC-level test that invokes `start_scan` with a real quoted
  tempdir path and listens for the resulting `scan-event` stream,
  confirmed to fail with the exact "does not exist" error a real Windows
  user hit before this fix, and to pass with it applied (verified both
  ways during development, not just written and trusted).
- FR-013 (`find_duplicate_folders`) is exercised by
  `commands::tests::find_duplicate_folders_reports_a_contained_folder_match`
  (IPC-level, a real tempdir with a fully-duplicated subfolder inside a
  bigger one, asserting on the actual response shape) and
  `commands::tests::find_duplicate_folders_rejects_a_nonexistent_root`,
  plus `payload::tests::folder_match_exact_serializes_with_a_snake_case_type_tag_and_camel_case_fields`/
  `folder_match_contained_serializes_with_subset_and_superset_paths`
  (payload shape), plus the manual end-to-end pass below confirming the
  frontend calls it automatically after a scan and renders both `Exact`
  and `Contained` results correctly, including the shallowest-first
  redundancy suppression (ADR-0021) showing up correctly end-to-end
  through the real IPC round trip.
- FR-014 through FR-017 (redesigned frontend: four real-data screens,
  automatic folder-dedup pass, keep-choice via path reordering, display-
  only file-type filter) are exercised by the manual end-to-end pass
  below: a real scan against a tempdir containing both file-level
  duplicates and a folder-level `Contained` match (a subdirectory fully
  duplicated inside a larger one with an extra file) rendered correctly
  on Dashboard, Scan Setup, and Duplicate Review; choosing the non-
  default copy to keep and applying `delete` removed the correct file
  and kept the chosen one, confirmed against the filesystem directly. No
  automated DOM-level test exists for any of these (see Open questions).
- FR-018 (`run_folder_action`) is exercised by
  `commands::tests::run_folder_action_delete_preview_does_not_touch_the_filesystem`,
  `commands::tests::run_folder_action_delete_apply_removes_the_folder`,
  and `commands::tests::run_folder_action_rejects_a_stale_scan` — all
  IPC-level, asserting on real filesystem state (a previously-shipped
  disabled button, this closes the gap ADR-0023 deliberately left open),
  plus `payload::tests::folder_action_plan_converts_with_camel_case_fields`
  (payload shape) and the manual end-to-end pass below.
- FR-019 (folder-match action button, keep-choice) is exercised entirely
  by the manual end-to-end pass below — no IPC-level test drives the
  frontend's pair-selection logic (`folderMatchPairs`), since that logic
  runs in `app.js`, not behind an `invoke` boundary; see Open questions.
- FR-023 (reference-folder guardrail) is exercised by
  `commands::tests::run_action_reference_path_overrides_the_chosen_keep_and_is_never_acted_on`
  and `commands::tests::run_folder_action_reference_path_protects_a_file_and_blocks_the_prune`
  — both IPC-level, asserting on real filesystem state (a protected file
  survives and, at the folder level, blocks the directory prune) — plus
  every existing `run_action`/`choose_keep`/`run_folder_action` test
  updated to pass `referencePaths: []`. `app.js`'s `referencePathsList`/
  `ensureRuleKeepChoice` changes have no automated coverage (same standing
  gap as the rest of `app.js`; see Open questions) and no manual Xvfb/
  `xdotool` pass this session — `xdotool` isn't installed in this
  environment, so this requirement, like FR-020 through FR-022 before it,
  has IPC-level verification only.

## Verification plan

Unit/IPC tests in `rusty_fclone-gui` (32 tests: 16 in `payload::tests`, 16
in `commands::tests`), run as part of `cargo test --workspace`. Manual
end-to-end verification of the redesigned frontend (this environment has
no display, so via Xvfb): a built binary was launched, screenshotted at
each step, and driven with `xdotool` through Dashboard (empty state) →
Scan Setup (typing a real root path, confirmed the value survives
keystroke-by-keystroke without losing focus) → Start Scan → Duplicate
Review (a real 4-item list: 3 file groups plus 1 folder `Contained`
match, correctly deduplicated per ADR-0021's redundancy suppression) →
choosing the non-default copy to keep → Apply Delete (confirmation
dialog text checked, accepted, real file deletion confirmed via `ls`
before/after) → Dashboard again (real updated stats: duplicate count,
reclaim estimate, storage breakdown by category, a session-scoped Recent
Scans row) → Rules screen → light-theme toggle. Every screen rendered
correctly in both themes.

FR-018/FR-019 (folder action) were additionally verified against two
fresh real trees in a separate manual pass: (1) a `Contained` match
(`small` fully duplicated inside `big`, which also had an extra file) —
the review card showed the enabled "Delete Duplicate Folder" button, the
confirmation dialog read "This will delete 1 file(s) across 1 folder,
reclaiming 4 B. Continue?", and accepting it pruned `small` entirely from
the real filesystem while `big`'s files (including the extra one) stayed
untouched; (2) an `Exact` match (`alpha`/`beta`, byte-identical) —
confirmed the default keep-choice picked `alpha` (alphabetically first,
matching the CLI), clicked the "Marked for removal" badge on `beta` to
switch the keep-choice, and confirmed the resulting delete removed
`alpha` and kept `beta` — proving the keep-choice selection actually
drives which folder is passed to `run_folder_action`, not just its
displayed label. Both cases confirmed via `find`/`ls` before and after,
not just the UI's own success message.

No automated frontend/DOM test suite exists (no JS test runner is part of
this project's dependency set) — frontend logic (`app.js`) is covered by
the manual end-to-end pass only, not by an automated test in CI.

## Traceability

See `docs/traceability/TRACEABILITY.md`.

## Open questions

- No native file/directory picker (Non-goals) — worth adding once Tauri's
  `dialog` plugin's permission/capability shape has been reasoned about
  the same way this spec reasoned about `core:default`.
- No automated frontend test coverage — `app.js`'s DOM logic (group
  rendering, result text, disabled-state handling) is only exercised
  manually. A headless-browser or WebDriver-based check would close this
  if the frontend grows past what manual verification can keep up with.
- Multiple concurrent scans aren't backend-guarded (Data/state and
  invariants) — revisit if the frontend ever allows triggering
  `start_scan` while one is already in flight.
- `release.yml` doesn't build or bundle the GUI (Non-goals) — a real
  release needs per-platform bundler prerequisites and real (non-
  placeholder) icon assets in every format `tauri build`'s bundler wants.
- Only verified on Linux (this development environment's only available
  platform, see `PROJECT-STATUS.md`) — macOS and Windows are unverified
  end-to-end. A real Windows build attempt did surface one real gap
  (fixed): the MSVC C++ toolchain requirement for `embed-resource`
  wasn't documented anywhere (ADR-0020's Consequences, README's GUI
  section).
- The Dashboard's "Space to reclaim (est.)" figure doesn't shrink as
  individual groups are resolved via Review in the same session — it's
  computed once from the groups a scan found, not re-derived after
  actions (ADR-0022's consequences). "Reclaimed this session" (the
  sidebar figure) is accurate and live-updating since it's summed from
  real `run_action` results; only the Dashboard estimate has this
  staleness.
- The Duplicate Review screen's cards (`compare-card`/`review-action-bar`
  and their contents) don't visually pick up the light theme the way
  Dashboard and Scan Setup do — noticed during FR-018/FR-019's manual
  verification pass on both the pre-existing file-level card and the new
  folder-level one equally, so it predates this change and isn't specific
  to folder actions; not investigated or fixed here since it's outside
  this change's scope (no CSS was touched to add FR-018/FR-019). Worth a
  dedicated look, since `GUI-REDESIGN`'s own manual pass (`PROJECT-
  STATUS.md`) reported "every screen rendered correctly in both themes."
- Reading `CLI-SCAN-HISTORY`'s persisted history, and exporting a real
  file (Dashboard's "Import history"/"Export (JSON)"), both need new
  backend work that doesn't exist yet — a GUI-side SQLite reader for the
  former, a save-file dialog (or the `dialog`/`fs` plugin work already
  tracked above) for the latter.

## Change history

- 0.3.4 (2026-08-27): Added the reference-folder guardrail's GUI surface
  (FR-023, `ACTION-REFERENCE-FOLDERS`, first unit of `docs/roadmap/
  DEDUP-GAP-IMPLEMENTATION-PLAN.md`'s Phase 2). A new "Protected folders"
  field on Scan Setup, sent as `referencePaths` to `run_action`,
  `choose_keep` (both gained the parameter), and `run_folder_action` —
  not `start_scan`/`find_duplicate_folders`, since detection itself
  doesn't need it. `ensureRuleKeepChoice` (Duplicate Review) now also
  resolves via `choose_keep` under the default `"alphabetical"` rule
  whenever a reference folder is configured, so the "keeping this file"
  badge reflects the guardrail before Apply. ADR-0025.
- 0.3.3 (2026-08-26): Reversed `FCLONE-ACTION-001`'s "configurable
  keep-strategy" v1 non-goal on the GUI side too (`SELECTION-RULES`,
  third and final Phase 1 unit of
  `docs/roadmap/DEDUP-GAP-IMPLEMENTATION-PLAN.md`). New `choose_keep`
  command (FR-022) makes Rules & Automation's "Keep newest copy" toggle
  real — the first of that screen's three toggles to stop being local-
  only preview (FR-014 revised); "Ignore tiny files" and "Auto-clean
  Downloads" are unaffected. `run_action` (FR-008/FR-009 revised) gained
  an optional `keepReason` parameter, defaulted to a placeholder when
  omitted, and its response's plan payload gained a `keepReason` field.
  A manual keep-choice badge always overrides the rule.
- 0.3.2 (2026-08-26): Added `"trash"` as a fourth action kind (FR-008/
  FR-018 revised) and FR-021 — the Duplicate Review/folder-review action
  selector now defaults to `"trash"` instead of `"delete"`, with permanent
  delete kept as an explicit choice (`ACTION-TRASH`, ADR-0024).
  `parse_action_kind` gained a `"trash"` branch.
- 0.3.1 (2026-08-26): Added FR-020 — a new "Include/exclude filters" card
  on Scan Setup (min/max size, include/exclude extensions, exclude paths),
  wired to `ScanOptionsPayload` and sent to `start_scan` for real, unlike
  Rules & Automation's existing preview-only toggles which are unchanged
  by this work (`DETECTION-SCAN-FILTERS`, first unit of the phased plan
  in `docs/roadmap/DEDUP-GAP-IMPLEMENTATION-PLAN.md`). `ScanOptionsPayload`
  gained five new optional fields, all defaulting to no filtering when
  omitted or blank.
- 0.3.0 (2026-08-25): Enabled the Duplicate Review screen's "Delete
  Duplicate Folder" button, shipped disabled in 0.2.0 pending ADR-0023's
  decision. New `run_folder_action` command (FR-018) wraps
  `rusty_fclone_core::folder_action::plan_folder`/`apply_folder`, mirroring
  `run_action`'s preview/apply split. `Contained` matches act directly
  (subset removed against superset); `Exact` clusters gained a per-folder
  keep-choice badge (FR-019), mirroring FR-016's file-level mechanism and
  defaulting to the alphabetically-first folder, matching the CLI's
  `folder_match_pairs` convention (`CLI-UX-001` FR-013) so the two
  surfaces make the same default choice on the same match. The prior
  Non-goal ("a real folder-level delete action") is resolved and removed.
- 0.2.0 (2026-08-25): Rebuilt the frontend against a design handoff
  (`Deduplication app UI design.zip`) — four real-data screens
  (Dashboard, Scan Setup, Duplicate Review, Rules & Automation) replacing
  the single-page root-path-and-options form. FR-014 through FR-017
  added; FR-011 revised (confirmation dialog replaces the apply
  checkbox as the "never implicit" mechanism — the requirement itself is
  unchanged). Several deliberate deviations from the mockup, all
  decided and recorded before implementing rather than discovered in
  review: no folder-level delete action exists (button disabled, not
  guessed at — an explicit user decision during this change); one scan
  root instead of a folder checklist; "Similar content" match mode
  shown disabled (fuzzy matching is a detection non-goal); two fake
  toggles replaced with three real `ScanOptions` toggles; file-type
  chips filter the Review list only, defaulting to all-on (the mockup's
  partial preselection would have silently hidden real duplicates); the
  folder-dedup pass (FR-013's `find_duplicate_folders`, landed just
  ahead of this change) now runs automatically after every scan;
  choosing which copy to keep reorders `paths` client-side rather than
  adding a core API; Dashboard/Recent-Scans are real but session-scoped,
  not persisted; Rules & Automation is an explicit local-only preview; a
  system font stack replaces the mockup's Google Fonts dependency
  (this app works offline); compare cards handle groups with more than
  two copies; the design's fake OS window chrome (traffic lights,
  shadow, rounded window) is dropped since a real Tauri window already
  has real chrome. Full rationale for each: ADR-0022. Default window
  size increased from 960×640 to 1200×840 (`tauri.conf.json`) to fit the
  new layout comfortably; still resizable, with a 860×560 minimum.
- 0.1.5 (2026-08-25): Added `find_duplicate_folders` (FR-013) — a new
  Tauri command wrapping `rusty_fclone_core::find_folder_duplicates`
  (ADR-0021), taking the scan root, the duplicate groups a prior
  `start_scan` produced, and the scan options, and returning
  `FolderMatchPayload::Exact`/`Contained` results. Backend-only: lands
  ahead of the frontend redesign (see Open questions) that will surface
  folder-level matches in the Duplicate Review screen.
- 0.1.4 (2026-08-25): Added FR-012 — `start_scan`'s root and
  `ScanOptionsPayload`'s `cachePath`/`fclonesImportPath` now strip
  surrounding whitespace and one layer of matching quote characters
  before use (`payload::normalize_path_input`). Surfaced by a real
  Windows user pasting a path copied via Explorer's "Copy as path" (which
  wraps it in double quotes) into the root-path field: the scan failed
  with `root path does not exist or is not a directory:
  "C:\Users\...\Downloads"` — the quotes were literal characters in the
  string, invisible in the error until read closely. A real GUI-usage
  gap, not a build/toolchain one like 0.1.1–0.1.3.
- 0.1.3 (2026-08-25): Trimmed `rusty_fclone-gui`'s `[lib]` `crate-type`
  from `["staticlib", "cdylib", "rlib"]` (Tauri's default scaffold, aimed
  at mobile targets this project doesn't build) down to the default
  (`rlib`-equivalent) — `main.rs` only ever needs to link the lib as a
  normal Rust dependency. Surfaced by a real Windows GNU-toolchain build:
  the unused `cdylib` output made the linker (`ld.exe`/BFD, MinGW's
  classic linker) build a giant standalone DLL with far more exported
  symbols than its 16-bit ordinal field can address, failing with `export
  ordinal too large: 109277`. The MSVC toolchain doesn't hit this (its
  linker handles large export tables differently), which is presumably
  why it went unnoticed until a real GNU-target Windows build happened.
  No behavior change — `cargo run -p rusty_fclone-gui` still starts the
  same app the same way; only the (unused) DLL/static-lib artifacts stop
  being built.
- 0.1.2 (2026-08-25): Added `icons/icon.ico` (a placeholder PNG-in-ICO,
  same treatment as the existing placeholder PNGs) and registered it in
  `tauri.conf.json`'s bundle icon list. A real Windows `cargo build`
  attempt failed outright — `icons/icon.ico not found; required for
  generating a Windows Resource file during tauri-build` — since
  `tauri-build` needs an `.ico` for *every* Windows build (debug
  included), not only `tauri build`'s release bundler as ADR-0020
  originally assumed. `.icns` (macOS) is still missing for the same
  reason and unverified for the same "might block debug builds too" risk
  — no macOS build attempt has surfaced it yet.
- 0.1.1 (2026-08-25): Documented the Windows build prerequisite (MSVC
  C++ toolchain, `embed-resource`'s `vswhom-sys` dependency) in ADR-0020
  and README — surfaced by a real Windows build attempt failing with
  `C1083: Cannot open include file: 'windows.h'` (built from a plain
  terminal, not an "x64 Native Tools Command Prompt for VS"). No
  functional/requirement change.
- 0.1.0 (2026-08-25): Initial implementation and specification.
  `rusty_fclone-gui` crate, `start_scan`/`run_action` commands, vanilla-JS
  frontend. ADR-0020.
