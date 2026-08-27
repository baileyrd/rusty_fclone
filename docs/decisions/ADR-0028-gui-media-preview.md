# ADR-0028: Inline media preview (data-URI, no new dependency, images/audio only)

- Status: Accepted
- Date: 2026-08-27
- Related: ADR-0020 (GUI via Tauri), ADR-0022 (GUI redesign, whose 4-screen
  design this extends rather than reworks), `docs/roadmap/
  DEDUP-GAP-IMPLEMENTATION-PLAN.md` (`GUI-MEDIA-PREVIEW`, Phase 3, first
  unit — the playbook's most-cited single UX failure category)

## Context

Duplicate Review's compare-cards show only a path and a size — no way to
actually *see* which copy of a photo, or hear which copy of a song, is
which before deciding what to keep. The gap-analysis research behind this
plan's Phase 3 calls this out as the single most-cited UX failure across
the products studied (CCleaner's total absence of it, specifically), and
recommends it precede the larger, riskier `DETECTION-PERCEPTUAL-IMAGES`
bet — most of the plumbing (rendering a file's content in the GUI) is
shared groundwork either way, and this half doesn't require a new
detection mode or its own precision tradeoffs.

Getting a local file's bytes in front of the webview needs one of:

1. Tauri's `asset:` protocol (a capability/permission grant plus scope
   configuration naming which paths it may serve) or the `dialog`/`fs`
   plugin family.
2. A Tauri command that reads the file and hands the frontend a
   `data:` URI to embed directly.

This project has deliberately not adopted (1) yet — the GUI's root-path
field still has no native file picker for the same reason, tracked as an
open item since `GUI-UX-001`, and `CLI-HISTORY-AUDIT`'s "Import history"
button was left explicitly blocked on it rather than half-wired. Adopting
it now, just for preview, would be a bigger prerequisite than this unit's
actual ask.

## Decision

- **Data-URI embedding via a new `read_preview` command, not the asset
  protocol.** `rusty_fclone-gui::preview::build_data_url` reads the whole
  file and returns `data:<mime>;base64,<...>`, which the frontend drops
  straight into an `<img src>`/`<audio src>`. No new capability grant, no
  scope configuration, no new permission surface — consistent with this
  project's demonstrated preference (`CLI-HISTORY-AUDIT`'s "Export
  (JSON)" via `<a download>`) for shipping what's genuinely deliverable
  without a plugin adoption decision riding along with it.
- **Base64 is hand-rolled, not a new dependency.** Unlike `trash`
  (ADR-0024) or `reflink-copy` (ADR-0014) — both genuinely
  platform-specific behavior not worth reimplementing — base64 encoding
  is a small, fully-specified, mechanical transform (RFC 4648) that's
  easy to get right and exhaustively testable against the RFC's own test
  vectors. ~30 lines, unit-tested against those vectors plus a real PNG
  file round-tripped byte-for-byte. Pulling in a dependency for this
  specifically would cut against the same "don't add a dependency the
  problem doesn't need" judgment this project has applied throughout.
- **Scoped to images and audio; video is explicitly not attempted.** The
  plan's own wording hedges on this ("and where feasible audio/video
  groups"). Whole-file base64 embedding is fine for a photo or a song
  (a 25 MB cap comfortably covers both) but not for typical video file
  sizes — hundreds of MB to multiple GB, which would spike memory and
  block the UI thread while reading. Doing video properly needs the same
  asset/stream protocol prerequisite this project has already deferred
  elsewhere; scoping it out here rather than attempting a degraded
  version keeps the honest-deferral pattern intact.
- **HEIC and TIFF are excluded from the "photo" preview set**, even
  though `app.js`'s existing `EXT_CATEGORY` already groups them under
  `"photo"` for filtering purposes. Most target webview engines
  (WebKitGTK, WebView2, WKWebView) don't render either format natively
  regardless of a correct MIME type on the `data:` URI — attempting it
  would silently show nothing or a broken-image icon, which is worse
  than falling back to the existing generic file icon.
- **Fails closed with a specific reason** (unsupported extension, file
  over the size cap, or a real I/O error) rather than ever returning a
  partial or silently-truncated preview — the same posture every other
  action/read path in this project already takes.
- **Frontend caches by path, not by group**, resolved lazily per
  compare-card the same way `ensureRuleKeepChoice` already resolves a
  keep-rule pick — one backend round trip per file, shared across every
  group that file happens to appear in, session-scoped only (no
  persistence, matching every other GUI render cache).

## Consequences

- `rusty_fclone-gui` gains a new `preview` module and `read_preview`
  command; no new external dependency, no new Tauri capability/permission
  grant.
- A photo/audio group's compare-card now costs one extra IPC round trip
  and a whole-file read per distinct path shown — bounded by the 25 MB
  cap, and only for files in a previewable category, so a scan dominated
  by document/archive duplicates pays nothing extra.
- The size cap and the HEIC/TIFF/video exclusions are real, felt
  limitations for a user with a library of oversized files or e.g.
  iPhone photos exported as HEIC — surfaced honestly (falls back to the
  existing generic icon, not an error toast) rather than silently
  degraded.
- `GUI-MEDIA-PREVIEW`'s groundwork (a working data-URI preview path,
  category detection already shared with `app.js`'s existing filter
  logic) is exactly what the plan's own ordering rationale expected it to
  de-risk for `DETECTION-PERCEPTUAL-IMAGES`, if that's picked up next.
- Not manually verified through a rendered window in this environment —
  no display/`xdotool` available, the same standing gap every GUI-facing
  unit this session has carried. The base64 encoder was additionally
  verified against a real 69-byte PNG file (round-tripped byte-for-byte
  through a real base64 decoder), beyond the RFC 4648 test-vector unit
  tests, as extra assurance given it's hand-rolled rather than a
  battle-tested crate.
