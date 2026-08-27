# ADR-0031: Real directory browse, three-panel Duplicate Review, sidebar collapse

- Status: Accepted
- Date: 2026-08-27
- Related: ADR-0022 (GUI redesign against the v1 design handoff — the
  precedent this ADR follows for reconciling a design handoff against
  what the real backend actually supports), ADR-0025 (protected/reference
  folders), ADR-0030 (perceptual "similar" images, whose read-only cards
  now render inside Duplicate Review's third panel unchanged)

## Context

`Deduplication app UI design.zip` (committed at the repo root) was
updated to a v2 handoff: a working HTML/JS design reference
(`RustyFClone-design.html` + `support.js`) whose own README is explicit
that it's a design reference, not production code, corrected against this
project's ADRs through ADR-0030. Diffing it against the v1 handoff ADR-
0022 already reconciled, most of what it now describes — Trash/Move/Copy
action kinds, protected folders, saved scan profiles, perceptual "similar"
matching — was *already real* in `rusty_fclone-gui`, ahead of the v1
mockup that hadn't caught up yet. Three things in the v2 handoff were
genuinely new, not yet in the app:

1. **A "Browse…" folder picker on Scan Setup**, opening a modal over a
   filesystem tree. The v2 README itself flags this as a deliberately
   unresolved question: "mocked against a static filesystem tree here —
   the real app still has no native file/directory picker ... Flag this
   to the team: either build a real in-app tree browser matching this
   modal, or treat it as illustrative until a native picker lands."
2. **A three-panel Duplicate Review layout** — a real filesystem tree
   (rooted at `/` in the mock, colored by scan status), a nested
   duplicate-group tree grouped by real path hierarchy, and the existing
   compare/action panel — replacing the single flat group list ADR-0022
   shipped, each panel independently collapsible.
3. **A sidebar that collapses to a 64px icon-only rail**, freeing width
   for those three Review panels on a narrower window.

The v2 README's own flagged question (#1) needed an actual decision
before implementation, not a guess, the same way ADR-0022 refused to
guess at the "Delete Duplicate Folder" button's real behavior and instead
recorded the decision.

## Decision

- **A real directory-listing command, not a fabricated tree.** New
  `list_directory` Tauri command (`commands.rs`) and `DirEntryPayload`
  wire type list a real path's immediate subdirectories via
  `std::fs::read_dir` — sorted case-insensitively, hidden entries
  skipped, degrading to an empty list (never an `Err`) for an unreadable
  directory — or, given no path, the platform's real browse-starting
  roots (home directory, plus `/` on Unix or each present drive letter on
  Windows). This resolves the v2 README's flagged question as: build the
  real in-app browser, backed by a real filesystem read, not an
  illustrative mock. It's still not a *native OS* picker dialog (Tauri's
  `dialog` plugin) — that remains a separate, larger, still-undecided
  question (`GUI-UX-001`'s Open questions) — but it is real data, not a
  fixed `Applications`/`Library`/`System`/`Users/you`/... mock with no
  relationship to the machine it's running on.
- **Scan Setup's "Browse…" button opens a modal over this real tree**,
  lazily expanding directories on demand (one `list_directory` call per
  expand) and writing the selected path into the existing plain-text root
  field on confirmation — the field itself stays hand-editable text,
  unchanged from ADR-0022; the modal is only ever a faster way to fill it
  in, never a new required step.
- **Duplicate Review's file-system panel is rooted at the scan root, not
  the design handoff's whole-disk `/`.** Every real duplicate this
  screen ever shows is guaranteed to live under the root that was
  actually scanned — browsing the rest of the filesystem from here would
  filter nothing, since nothing outside the scan root can ever match, and
  would mean the Review screen reading directories a scan never touched
  for no operational reason. Rooting at the scan root instead keeps this
  panel's real reads scoped to real scan output, a natural extension of
  ADR-0022's "one scan root, not a folder checklist" decision rather than
  a break from it. The Scan Setup browse modal, which genuinely needs to
  let the user pick an arbitrary location, keeps the platform-rooted
  tree.
- **A row's color/badge tier (direct / ancestor / none) is computed from
  every real path a duplicate touches, not just each item's first/
  representative path** — a file group's every copy, an exact folder
  match's every folder, a contained match's subset and superset — the
  same breadth the v2 mock's own `directCounts` computation used
  (`[...groups, ...similarGroups].forEach(g => g.files.forEach(f =>
  touched.add(dirOf(f.path))))`), so a folder gets a badge as soon as
  *any* copy of *any* duplicate lives there, not only the one the
  duplicate-group panel happens to sort first.
- **The duplicate-group panel's nesting and the "Item X of N" navigation
  share one sorted, filtered list** (`reviewItems()`), so clicking a
  file-system row to filter, or a duplicate-group panel item to select,
  can never desynchronize what the compare panel shows from what's
  highlighted in either tree — the same single-source-of-truth shape
  ADR-0022's original flat list already had, just re-derived under a
  richer, folder-aware sort instead of scan-arrival order.
- **The sidebar's collapse state is independent of Review's three-panel
  collapse state** — collapsing the main nav to free horizontal width and
  collapsing, say, the file-system panel to free width *within* Review
  are two different reasons a user might want more room, and nothing
  here couples them.
- **No new Tauri capability or permission grant.** `list_directory` is a
  plain Rust function reading the real filesystem directly, the same
  pattern `read_preview` (ADR-0028) and `profiles::default_profiles_dir`
  (ADR-0029) already established for touching the real filesystem/OS
  directories without adopting the `dialog`/`fs` plugin family — this
  project's now-repeated alternative to that still-undecided prerequisite
  when a command's own scope doesn't actually need it (`list_directory`
  only ever reads directory *names*, never file contents, and returns
  nothing outside directories that genuinely exist).

## Consequences

- `rusty_fclone-gui` gains `list_directory`/`DirEntryPayload`, four new
  hermetic unit tests (real subdirectories only, sorted case-
  insensitively, correct `hasChildren`, graceful degradation on an
  unreadable/missing path), and no new dependency or Tauri capability
  entry.
- Duplicate Review's previous single flat `group-list`/`review-layout`
  CSS is retired in favor of `review-3col`/`fs-panel`/`dup-tree-panel`/
  `review-main-panel`; `group-row`/`group-swatch`/`group-row-name`/
  `group-row-meta` are kept and reused for the duplicate-group panel's
  item rows rather than duplicated.
- The file-system panel's badges/filter only ever reflect the *current*
  scan's root — switching scan roots (`startScan`) resets the panel's
  loaded-directory cache, open-path set, and active filter, since a prior
  scan's tree has no bearing on a new one.
- No `rusty_fclone_core` API changed — `list_directory` is entirely new
  surface in the GUI crate, reading the OS filesystem directly rather
  than through any core scan/action function.
- Verified this session via a scratch, non-committed headless-Chromium
  (Playwright) script driving the real `index.html`/`app.js`/`style.css`
  files with a mocked `window.__TAURI__`, plus the four new Rust unit
  tests — not a real Tauri window/Xvfb pass (no display or `xdotool`
  toolchain available in this environment this session); see
  `GUI-UX-001`'s Verification plan and Open questions for the full
  account of what that covered and what a real window pass would still
  need to add.
