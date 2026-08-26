# Dedup Capability Gap — Implementation Plan

Synthesis of `Dedup App Capability & UX Playbook.md` and `Duplicate File
Finder Software Analysis.md` (both supplied by the user, not tracked in this
repo) against `rusty_fclone`'s actual current state, as verified against the
codebase and `docs/` on 2026-08-26 (main at `078320c`). Every "current state"
claim below is grounded in a specific file/module, not assumed from the
playbook's category list.

**Status of this document:** a proposal, not a commitment. Nothing here is
added to `docs/roadmap/ROADMAP.md` yet — that file is what `WORKFLOW.md`'s
`next` step reads to auto-select the next unit to implement, so it should
only contain units the project owner has actually greenlit. Once specific
phases/units below are approved, fold the chosen rows into `ROADMAP.md` (same
table shape, reused directly from this doc) and this plan can be trimmed to a
changelog note or removed.

---

## 1. Where this project already sits, unusually well

Two things the playbook calls out as the *rarest* and most valuable patterns
in the market are already true here, structurally, without anyone building
toward them on purpose:

- **A genuinely embeddable core engine.** `rusty_fclone-core` has zero CLI/GUI
  concerns (`AGENTS.md`'s own boundary rule) and is consumed identically by
  both `rusty_fclone-cli` and `rusty_fclone-gui`. This is exactly the
  Czkawka-style architecture the playbook flags as a 1-vendor differentiator
  ("if your product's real advantage is the engine, architect it as a
  reusable core from day one").
- **Precision-first, hash-verified detection with no filename/size/date
  heuristics.** The pipeline groups purely by size → staged partial hash →
  full hash (ADR-0001), the same "never filename, size, or timestamp alone"
  stance the playbook singles out as DuoBolt's positioning. Combined with
  hardlink-alias awareness and the folder-level `Exact`/`Contained` detection
  (ADR-0021, a capability only Duplicate Cleaner Pro also has), the detection
  side is already ahead of most of the eight products studied.
- **Already free, local-only, zero-telemetry, dual-licensed (MIT/Apache-2.0),
  cross-platform (Windows/macOS/Linux), with a real designed GUI** (not a
  bolted-on afterthought — `GUI-REDESIGN` was built against an actual design
  handoff). This is precisely the playbook's stated biggest open gap in the
  market: *"nobody combines commercial-grade UX with open-source-grade
  breadth."* `rusty_fclone` is structurally positioned to be that product —
  it just isn't finished yet. That framing should drive prioritization below:
  close the safety/usability gaps that make a tool feel "commercial-grade"
  before chasing detection breadth that makes it feel "open-source-grade."

## 2. Verified gaps

Checked directly against `crates/rusty_fclone-core/src/model.rs`,
`traversal.rs`, `action.rs`, `folder_action.rs`, and
`crates/rusty_fclone-gui/ui/app.js`.

| Gap | Current state (verified) | Playbook signal |
|---|---|---|
| No include/exclude scan filters | `ScanOptions` (`model.rs`) has no size/extension/path-glob filter field at all; `traversal.rs` only implements filesystem-boundary and symlink handling. The GUI's "Rules & Automation" screen shows filter-looking toggles (e.g. "Skip files smaller than 10 KB") but `app.js`'s own comment marks that whole screen `local-only preview -- no backend exists yet`. | 8/8 products have this; universal table stakes |
| No bulk rule-based auto-select | Every keep/remove choice in the GUI (`keepChoice` state in `app.js`) is a manual per-group, per-folder click, defaulting to alphabetically-first. No "keep newest/oldest/by-path" rule exists anywhere in the core, CLI, or a real (non-preview) GUI surface. | 7/8 products have this; its absence is called out as making a tool "feel broken on any real-world scan" |
| No trash/recycle-bin delete | `action.rs`: `ActionKind::Delete => fs::remove_file(&action.path)` — permanent, unconditionally. `ActionKind` has exactly `Delete`/`Hardlink`/`Reflink`, no trash-routed option. | 7/8 products default deletion through Recycle Bin/Trash; flagged as near-universal |
| No protected/reference-folder guardrail | No `reference`/`protected` concept anywhere in `action.rs`, `folder_action.rs`, or `model.rs` (confirmed via grep — zero matches). | Independently built by 3 unrelated teams (DuoBolt, Czkawka, dupeGuru) per the playbook — treated there as the single strongest "build this" signal in the whole study |
| No move/copy alternative action | `ActionKind` is delete/hardlink/reflink only — no "move redundant copies to an archive folder" or "copy" path. | dupeGuru offers Delete/Move/Copy explicitly; several others have an archive-folder option |
| No per-action audit trail / export | `--history` (ADR-0017) records one summary row per *scan*, not per file/group/action, and has no query/report subcommand (both explicitly deferred at the time). GUI's Dashboard "Export (JSON)"/"Import history" buttons are already known-disabled placeholders (`PROJECT-STATUS.md`, "Next" section). | Export/audit log present in half the products studied; a stated, already-tracked gap in this repo's own docs |
| No inline media preview | Duplicate Review lists paths/sizes only — no thumbnail or content preview for image/audio/video groups. | Called the single most-cited UX failure in the whole category (CCleaner's absence of it); this repo doesn't have it either yet |
| Perceptual/similar-content matching | Explicit "non-goal for v1" in `docs/architecture/SYSTEM-ARCHITECTURE.md`, noted there as reversible (the same way "no GUI" was, until `GUI` shipped). | 5/8 products have some form of this; real differentiator, but the biggest engineering lift on this list |
| No visualized scan summary beyond numbers | Dashboard shows a "storage breakdown" panel post-scan; not confirmed as a chart (donut/bar) vs. plain numbers in the current frontend. | Cheap, still a 1-vendor feature per the playbook (DuoBolt) |

## 3. Non-gaps — deliberately not on this list

- **Filename/date-based matching modes.** The playbook lists these as
  near-universal, but building them here would weaken this project's actual
  positioning (hash-verified precision, zero heuristic false positives) for
  a feature whose entire value proposition is "good enough, sometimes
  wrong." Not recommended.
- **NAS/network-share-specific handling.** Network shares already scan fine
  as ordinary paths (no special exclusion in `traversal.rs`); the playbook's
  "NAS/network drive scan" row is about explicit support/tuning, not basic
  functionality, which already works. Low priority, not urgent.
- **The pitfalls the playbook warns against** (cross-sell bundling in the
  installer, paywalled safety features, flattening every setting onto one
  screen) — none apply; this project is fully open source with no paywall,
  and the GUI's phased 4-screen design (`GUI-REDESIGN`) already avoids the
  AllDup "everything is a button" trap.

## 4. Proposed phases

Each unit below is written in the same shape as `docs/roadmap/ROADMAP.md`'s
existing rows, sized to land as one PR the way this project's units
consistently have. Phases are ordered by the "commercial-grade UX first"
framing from §1 — safety and everyday usability before detection breadth.

### Phase 1 — Table stakes (safety + everyday usability)

| Unit | Outcome | Depends on | Touches | Exit gate |
|---|---|---|---|---|
| `DETECTION-SCAN-FILTERS` | Real include/exclude filtering: min/max size, extension/glob include-exclude, path-prefix exclude — as new `ScanOptions` fields applied during traversal (skip before hashing, not after). CLI flags (`--min-size`, `--max-size`, `--exclude <glob>`, ...). GUI's existing "Rules" toggles wired to real `ScanOptions` instead of the local-only preview state. | `DETECTION-BASELINE` | `model.rs`, `traversal.rs`, `rusty_fclone-cli`, `rusty_fclone-gui` | `cargo fmt`/`clippy`/`test` green; new filter fields each have a unit test; manual scan against a tree with mixed sizes/extensions confirms exclusions actually skip files pre-hash (not just post-hash filtering) |
| `ACTION-TRASH` | New `ActionKind::Trash` — routes a redundant copy through the OS trash/recycle bin instead of `fs::remove_file`, via a cross-platform crate (needs its own one-line ADR justification per `AGENTS.md`'s dependency rule, and a check that it doesn't pull in a C toolchain requirement the way `rusqlite`/Tauri already did twice). CLI `--action trash`; GUI default action changed from permanent delete to trash, with permanent delete kept as an explicit opt-in. | `ACTION-LAYER` | `action.rs`, `folder_action.rs` (same `ActionKind` enum, so both get it for free per the existing reuse pattern), CLI, GUI | Every existing `ActionKind` test pattern repeated for `Trash`; manual smoke test confirms a trashed file is recoverable via the OS trash, not gone |
| `SELECTION-RULES` | A pure, testable core function — e.g. `select_keep(&DuplicateGroup, Rule) -> &Path` for `Rule::Newest/Oldest/Largest/ShortestPath/...` — plus a one-line reason string per choice (the playbook's cheap "why this one" trust-building win, without needing any AI ranking to justify it). CLI `--keep-rule <rule>` applies it across every group in one pass instead of the current alphabetically-first default. GUI's fake "Keep newest copy" toggle becomes real, and every keep-choice badge shows the reason. | `ACTION-LAYER` | new small module (`rusty_fclone-core`, e.g. `select.rs`), CLI, GUI | Unit tests per rule (tie-breaking behavior specified and tested); manual GUI pass confirms one click applies a rule across all groups instead of per-group manual selection |

### Phase 2 — Safety and trust differentiators

| Unit | Outcome | Depends on | Touches | Exit gate |
|---|---|---|---|---|
| `ACTION-REFERENCE-FOLDERS` | Protected/reference-folder guardrail, as a hard block inside `plan`/`plan_folder` (fails closed — a protected path is never placed in `actions`, not just flagged in the UI), matching the playbook's explicit "build it as a hard block, not a dismissible warning" recommendation. New `ScanOptions`/action-input field for reference paths; CLI `--reference <path>` (repeatable); GUI folder-marking UI. Needs its own ADR — this extends ADR-0009's safety model, which is an architecture-level decision per `AGENTS.md`. | `ACTION-LAYER`, `DETECTION-FOLDER-DEDUP` | `action.rs`, `folder_action.rs`, new ADR, CLI, GUI | Unit test proving a plan against a group containing a protected path either omits that path or (if every copy is protected) produces no plan at all — never a partial plan that silently drops the guard; manual test confirms the GUI can't even select a protected file for removal |
| `ACTION-MOVE-COPY` | `ActionKind::Move`/`ActionKind::Copy` to a user-chosen archive folder, alongside delete/hardlink/reflink/trash. | `ACTION-LAYER` (and ideally after `ACTION-TRASH`, so the `ActionKind` enum settles once) | `action.rs`, `folder_action.rs`, CLI, GUI | Same per-`ActionKind` test pattern as existing variants; manual test confirms a moved file's new path is correct and the original disappears |
| `CLI-HISTORY-AUDIT` | Closes the two explicitly-deferred gaps already named in `PROJECT-STATUS.md`: (1) optional per-action detail rows (file/group, kind, bytes) alongside the existing scan-summary row, and (2) a `history` query/report subcommand (at minimum: list recent scans, total bytes reclaimed over a date range). GUI's already-disabled "Export (JSON)"/"Import history" buttons get wired to real backend calls. | `CLI-SCAN-HISTORY` | `rusty_fclone-cli`'s `history` module, GUI | New rows/queries covered by unit tests mirroring the existing 4 `history` tests; manual test: two scans plus a query round-trip through the new subcommand match what actually happened on disk |

### Phase 3 — Reach (larger, separately-scoped bets)

These reverse a stated non-goal or add real scope; each needs its own ADR
and spec revision before starting, same as `DETECTION-LINUX-FASTPATH` already
sits as a scoped-but-undesigned item in `ROADMAP.md` today.

| Unit | Outcome | Notes |
|---|---|---|
| `GUI-MEDIA-PREVIEW` | Inline thumbnail/content preview in Duplicate Review for image (and where feasible audio/video) groups, addressing the playbook's most-cited single UX failure category. Doesn't require perceptual matching — valuable standalone for exact-duplicate image/audio groups today. | Good candidate to do *before* `DETECTION-PERCEPTUAL-IMAGES` — most of the preview plumbing (rendering a file's content in the GUI) is shared groundwork either way. |
| `DETECTION-PERCEPTUAL-IMAGES` | Opt-in perceptual image-similarity mode, reversing `SYSTEM-ARCHITECTURE.md`'s "non-goal for v1" note (explicitly marked reversible there, same as the GUI was). Must stay opt-in and clearly separated from the hash-verified exact engine — the core "zero false positives" precision guarantee (§1) is this project's actual differentiator and shouldn't be diluted by silently mixing a probabilistic mode into the default path. | New dependency (image decode + perceptual hash) needs its own ADR per `AGENTS.md`'s dependency-justification rule. Largest single lift on this list. |
| `SCAN-PROFILES` | Saved scan setups (root + `ScanOptions` preset) in the GUI, upgrading the current session-only "Recent Setups" into something persisted and re-runnable. | Natural pairing with `DETECTION-SCAN-FILTERS` — filters are exactly the kind of per-tree configuration worth saving. |
| Visual chart confirmation/upgrade | Confirm whether the Dashboard's existing "storage breakdown" is a real chart; if it's numbers-only, add a simple donut/bar (file-type distribution, the same shape the playbook credits DuoBolt for). | Small, cheap — worth folding into whichever phase touches the Dashboard next rather than a standalone unit. |

**Explicitly out of scope for this plan:** mobile apps and native cloud-
storage scanning. The playbook correctly flags both as wide-open market gaps,
but they're multi-crate/multi-platform undertakings (a mobile frontend,
OAuth+API integration per cloud provider) far larger than this project's
current single-desktop-app scope — worth a dedicated future proposal of their
own, not a line item here.

## 5. Suggested order and rationale

Phase 1 first, in the order listed: `DETECTION-SCAN-FILTERS` is
dependency-free and unblocks realistic large-tree usage; `ACTION-TRASH`
closes the single largest safety-perception gap versus the rest of the
market at low engineering cost; `SELECTION-RULES` is what actually makes the
GUI's already-built Rules screen real instead of decorative. Phase 2's three
units are independent of each other and can run in any order once Phase 1
lands (each needs `ACTION-LAYER`, already done). Phase 3 should wait until
Phase 1–2 land and get real usage — per the playbook's own "commercial-grade
UX first" framing, and because `GUI-MEDIA-PREVIEW`'s groundwork materially
de-risks `DETECTION-PERCEPTUAL-IMAGES` if both are eventually wanted.
