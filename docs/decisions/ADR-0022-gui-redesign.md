# ADR-0022: GUI redesign against real data

- Status: Accepted
- Date: 2026-08-25
- Related: ADR-0020 (GUI via Tauri, the frontend this replaces), ADR-0021
  (folder-level duplicate detection, surfaced here for the first time),
  ADR-0009 (action-layer safety model — this ADR's confirm-dialog gate is
  a new mechanism for the same "never implicit" requirement, not a
  relaxation of it)

## Context

A high-fidelity design handoff (`Deduplication app UI design.zip`,
committed at the repo root) specifies a full rebuild of
`rusty_fclone-gui`'s bare-bones vanilla-HTML frontend into a 4-screen app
(Dashboard, Scan Setup, Duplicate Review, Rules & Automation), built as a
working HTML/CSS/JS reference. Its own README is explicit that it's a
*design reference*, not production code, and flags two things the
implementer must resolve rather than blindly wire up: a "Delete
Duplicate Folder" button with no real backend (ADR-0021 deliberately has
no folder-level delete action), and a Dashboard that assumes scan-history
reading/JSON export the backend doesn't have.

Beyond those two flagged points, recreating the mockup pixel-for-pixel
against the *real* app surfaces several more places where the mockup's
assumptions don't match what `rusty_fclone_core`/`rusty_fclone-cli`
actually support, or where the mockup fakes something a real desktop app
already has for free. This ADR records every deliberate deviation and
why, so the gap between "what the design shows" and "what shipped" is
documented, not just discovered by reading a diff.

## Decision

- **No fake OS window chrome.** The design's `.dc.html` artboard runs
  inside an infinite pan/zoom canvas, so it fakes a window: rounded
  corners, a drop shadow, and a macOS-style traffic-light titlebar. The
  real Tauri app already runs inside a real OS window with real chrome
  (native traffic lights on macOS, native minimize/maximize/close
  elsewhere) — reproducing a *fake* titlebar inside a *real* window
  would look broken (two title bars) on every platform. The rebuilt
  frontend drops the outer wrapper, shadow, and fake titlebar entirely;
  the sidebar and content fill the real window directly.
- **User asked, and answered: no folder-level delete action is wired
  up.** The "Delete Duplicate Folder" button is rendered disabled, with
  a tooltip explaining why (ADR-0021 has no such action) and what to do
  instead (remove the underlying files individually from the file-level
  Review list). No guessed behavior, no per-file-pipeline workaround —
  a real product decision on what this button should do is still
  needed before it does anything.
- **One scan root, not a folder checklist.** `rusty_fclone_core::scan`
  takes exactly one root directory; the mockup's multi-folder checklist
  (`~/Pictures`, `~/Documents`, ...) has no backend to scan more than one
  location per run. The Scan Setup screen shows a single directory text
  field instead, with an explicit hint ("Scanning multiple locations at
  once isn't supported yet") rather than silently dropping the
  unchecked folders' worth of UI.
- **"Similar content" match mode is visibly disabled, not silently
  ignored.** Near-duplicate/fuzzy matching is an explicit non-goal of
  `FCLONE-DETECTION-001` — the engine only ever does exact byte-identical
  matching. The segmented control still shows both options (matching the
  mockup) but "Similar content" is a disabled, tooltipped option rather
  than a control that looks clickable and does nothing different.
- **The two fake toggles are replaced with three real ones.** "Include
  subfolders" and "Skip system files" don't correspond to any
  `ScanOptions` field — traversal is always fully recursive, and there's
  no system-file-skip flag. They're replaced with toggles for
  `follow_symlinks`, `cross_filesystems`, and `verify_matches` — every
  boolean `ScanOptions` field not already covered by another control on
  this screen, each one doing exactly what its label says.
- **File-type chips filter the Review list, not the scan.** The mockup's
  chips read as scan-time filters ("Photos", "Documents", ...); the
  engine has no per-type scan filter, so re-implementing that would mean
  a new `ScanOptions` field and traversal-level filtering — real scope,
  not a UI recreation. Instead the chips filter which already-found
  duplicate groups are *shown* in Review, client-side, by file
  extension. All five are on by default (the mockup pre-deselects two) —
  defaulting to a subset would silently hide real duplicates behind a
  cosmetic control on first use, which is a correctness trap a display
  filter shouldn't have.
- **The folder-dedup pass runs automatically after every scan.** There's
  no separate "find folder duplicates" trigger in the mockup — folder
  matches just appear in the Review list. The frontend reproduces that:
  once a scan's `finished` event arrives and at least one duplicate group
  was found, it calls the new `find_duplicate_folders` command (this
  branch's earlier commit) with that scan's root, groups, and options,
  and merges the results into the Review list once they come back.
- **Choosing which copy to keep reorders `paths`, not a new core API.**
  The mockup lets a user click either compare card to choose which copy
  is "kept." `action::plan` always keeps `group.paths[0]` — there's no
  parameter to choose a specific path. Rather than add one to
  `rusty_fclone_core`, the frontend already reconstructs the
  `GroupPayload` it sends to `run_action` (it did before this change
  too, for the CLI-mirroring reason `GUI-UX-001`'s architecture section
  already documents) — so choosing a copy just reorders that group's
  `paths` array, putting the chosen path first, before calling
  `run_action`. Same real contract, no engine change.
- **Apply is gated by a confirmation dialog, not a checkbox.** The
  mockup's Review screen has one "Apply {Kind}" button with no separate
  preview toggle — but ADR-0009/`GUI-UX-001`'s Non-goals require apply to
  never be implicit. Clicking Apply shows a native confirmation dialog
  naming the action, file count, and bytes; only accepting it calls
  `run_action` with `apply: true`. This replaces the previous frontend's
  unchecked-by-default checkbox as the safety gate — same requirement,
  a mechanism that fits a one-button design instead of a two-control one.
- **Dashboard and Recent Scans are real, session-scoped, not persisted.**
  Every stat card, the storage breakdown, and the Recent Scans table are
  computed from scans actually run in the current app session — never
  mock data. None of it persists across a relaunch: there's no GUI-side
  reader for `CLI-SCAN-HISTORY`'s SQLite database and no file-write
  dialog to export one, so "Import history" and "Export (JSON)" are
  rendered disabled with a tooltip pointing at the CLI's `--history`
  flag as today's real persistent option, rather than silently
  no-op'ing or writing a file without asking.
- **Rules & Automation is a local, unpersisted preview.** No rule
  engine, no persistence, and no scan-time enforcement exist anywhere in
  `rusty_fclone_core`. The screen still renders (toggle state lives in
  frontend memory only, reset on relaunch) with an explicit "preview
  only" subtitle and footer note, rather than being cut entirely or
  pretending to do something real.
- **System font stack, not the mockup's Google Fonts webfont.** The
  mockup specifies Manrope via a `fonts.googleapis.com` stylesheet link.
  This app works fully offline today; adding a webfont fetch on startup
  would be a real regression (a blank-until-loaded flash, or a broken
  fallback with no network), not a cosmetic simplification. The rebuilt
  frontend uses a system UI font stack instead everywhere the mockup
  specified Manrope.
- **Compare cards handle more than two copies.** The mockup only ever
  mocks 2-file groups (Copy A/Copy B side by side). A real
  `DuplicateGroup` can have any number of paths ≥ 2. The Review screen's
  compare row wraps onto additional rows instead of assuming exactly
  two, and labels cards "Copy 1"/"Copy 2"/... instead of letters.
- **Per-file "Modified"/"Details" and per-folder file-name previews are
  dropped, not fabricated.** The mockup's compare cards show a modified
  date and image dimensions/duration per file, and a folder card shows a
  truncated file-name preview string. Neither `DuplicateGroup` nor
  `FolderMatch` carries that data — only path, size (per file/group) and
  file count/bytes (per folder match). Rather than fabricate placeholder
  values, those rows are simply not rendered.

## Consequences

- No `rusty_fclone_core` API changed for this ADR — every real-data
  adaptation above works against the existing `scan`/`run_action`/
  `find_folder_duplicates` surface (the last exposed to the GUI by the
  companion `find_duplicate_folders` command, landed just ahead of this
  change).
- `GUI-UX-001`'s FR-011 (apply defaults unchecked) is revised in place to
  describe the confirmation-dialog mechanism rather than the checkbox
  that no longer exists in this frontend — the underlying requirement
  (apply is never implicit) is unchanged.
- The Dashboard's "Space to reclaim (est.)" figure is computed once from
  the groups a scan found and does not re-shrink as individual groups
  are resolved via Review in the same session — an accepted staleness
  tradeoff for a same-session estimate, not a promise of a live-updating
  total. `sessionBytesReclaimed` (the sidebar's "Reclaimed this session"
  figure) is the accurate, live-updating number, since it's summed from
  real `run_action` results as they happen.
- A native file/directory picker is still out of scope (unchanged from
  `GUI-UX-001`'s existing Non-goal) — every path field, including the
  new single scan-root field, is a plain text input.
- Folder-level matches are informational-only in this version: reviewed,
  displayed, but with no real action to take on them from the GUI. A
  product decision on what "Delete Duplicate Folder" should actually do
  (a new core action, or a defined per-file-pipeline mapping) is a
  prerequisite for enabling it, not attempted here.
