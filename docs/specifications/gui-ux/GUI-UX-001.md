# GUI-UX-001 — Desktop GUI (Tauri)
- Version: 0.1.0
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
  default model (ADR-0009) — `apply` defaults to unchecked in the
  frontend, mirroring the CLI's `--apply`-required gate exactly.

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
  (`"delete"|"hardlink"|"reflink"`), and `apply: bool`; the backend SHALL
  call `action::plan` unconditionally and SHALL call `action::apply` if
  and only if `apply` is `true`.
- `GUI-UX-001-FR-009`: `run_action`'s response SHALL include the plan
  (`kept`, `planned` paths, `bytesReclaimed`) always, and the apply report
  (`succeeded`, `failed`, `bytesReclaimed`) if and only if `apply` was
  `true`.
- `GUI-UX-001-FR-010`: `run_action` SHALL reject an action kind outside
  `{"delete","hardlink","reflink"}` with an `Err`, without calling `plan`
  or `apply`.
- `GUI-UX-001-FR-011`: The frontend's apply control SHALL default to
  unchecked (preview) on every newly rendered duplicate group — never
  pre-checked, and never remembered from a previous group's choice.

## Architecture and interfaces

`rusty_fclone-gui` (new crate, ADR-0005/ADR-0020):

```rust
// src/commands.rs
#[tauri::command]
fn start_scan<R: Runtime>(app: AppHandle<R>, root: String, options: ScanOptionsPayload) -> Result<(), String>;
#[tauri::command]
fn run_action(group: GroupPayload, kind: String, apply: bool) -> Result<ActionResultPayload, String>;

// src/payload.rs — serde DTOs, kept out of rusty_fclone-core (ADR-0020)
struct ScanOptionsPayload { /* mirrors ScanOptions, all fields optional */ }
enum ScanEventPayload { DuplicateGroup { .. }, Error { .. }, Progress { .. }, Finished(ScanSummaryPayload) }
struct GroupPayload { size: u64, paths: Vec<String> }
struct ActionResultPayload { plan: ActionPlanPayload, applied: Option<ApplyReportPayload> }
```

Frontend (`ui/`, plain HTML/CSS/JS, no bundler —
`tauri.conf.json`'s `app.withGlobalTauri: true`): `index.html` (root-path
field, options `fieldset`, status line, group list), `app.js` (`invoke`/
`listen` calls, group rendering, action controls), `style.css`.

## Data/state and invariants

- Same as `CLI-UX-001`'s: `ScanProgress`'s counters are cumulative, not
  deltas; `Finished` is always the last `scan-event` for a scan (FR-006).
- The frontend holds duplicate-group data purely in the DOM/JS memory
  (each group's `size`/`paths` round-trip back to `run_action` from what
  was rendered) — no separate state store, no re-fetch from the backend
  between receiving a `duplicate_group` event and calling `run_action` on
  it.
- Multiple concurrent scans are not prevented at the command level
  (`start_scan` has no "already running" guard); the frontend's own Scan
  button is disabled while a scan is in flight as its only guard against
  this, matching the CLI's single-scan-per-process model in practice but
  not as a backend-enforced invariant.

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
  additionally confirmed manually: a real file was deleted through the
  rendered UI's Apply checkbox + Run button, verified against the
  filesystem directly.
- FR-011 (apply defaults unchecked) is exercised by the manual end-to-end
  pass (the Apply checkbox was unchecked by default in the rendered DOM
  before being explicitly checked for the apply test above); no automated
  DOM-level test exists (see Open questions).

## Verification plan

Unit/IPC tests in `rusty_fclone-gui` (9 tests: 5 in `payload::tests`, 4 in
`commands::tests`), run as part of `cargo test --workspace`. Manual
end-to-end verification (this environment has no display, so via Xvfb): a
built binary was launched, screenshotted at each step, and driven with
`xdotool` through a full scan → group render → preview → apply cycle
against a real tempdir with a real duplicate pair, with filesystem state
checked directly (`ls`) before and after the apply step.

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

## Change history

- 0.1.0 (2026-08-25): Initial implementation and specification.
  `rusty_fclone-gui` crate, `start_scan`/`run_action` commands, vanilla-JS
  frontend. ADR-0020.
