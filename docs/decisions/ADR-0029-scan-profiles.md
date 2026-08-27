# ADR-0029: Persisted scan profiles (flat JSON file, `dirs` crate reused)

- Status: Accepted
- Date: 2026-08-27
- Related: ADR-0017/ADR-0027 (`CLI-SCAN-HISTORY`/`CLI-HISTORY-AUDIT`'s
  SQLite persistence — a deliberately different choice than this one),
  ADR-0028 (`GUI-MEDIA-PREVIEW`, the prior Phase 3 unit),
  `docs/roadmap/DEDUP-GAP-IMPLEMENTATION-PLAN.md` (`SCAN-PROFILES`,
  Phase 3, second unit)

## Context

The GUI's Scan Setup screen builds a `{root, ScanOptions}` combination —
directory, size/extension filters, cache paths, toggles — from scratch on
every launch. The plan's own description names the gap directly: "Saved
scan setups (root + `ScanOptions` preset) in the GUI, upgrading the
current session-only... into something persisted and re-runnable,"
pairing naturally with `DETECTION-SCAN-FILTERS` since filters are exactly
the kind of per-tree configuration worth saving.

Two things needed deciding: where a saved profile lives on disk, and how
the backend resolves that location without either (a) requiring a new
Tauri capability/permission grant, or (b) becoming untestable because the
real location is the host machine's actual config directory.

## Decision

- **A flat JSON file, not SQLite.** `CLI-SCAN-HISTORY`/`CLI-HISTORY-AUDIT`
  chose `rusqlite` because scan/action history is an append-only,
  potentially large, query-shaped log. A handful of named scan profiles is
  the opposite shape — small, read-and-rewritten-whole, no query need
  beyond "list them all" — exactly what a `Vec<ScanProfilePayload>`
  round-trips through `serde_json` without any new dependency at all
  (`rusty_fclone-gui` already depends on `serde_json`).
- **The `dirs` crate, added as a direct dependency — but not a new one in
  practice.** Tauri's own `app.path().app_config_dir()` already resolves
  the OS-appropriate per-user config directory via `dirs::config_dir()`
  internally; `dirs` 6.0.0 was already present in `Cargo.lock` as one of
  Tauri's transitive dependencies before this change. Declaring it
  directly here adds zero new supply-chain surface — same crate, same
  already-vetted version — while letting `profiles::default_profiles_dir`
  stay a plain function with no `AppHandle` parameter.
- **No `AppHandle`, by design — testability drove this.** Tauri's
  `app.path().app_config_dir()` needs an `AppHandle`, and its default
  behavior under `tauri::test`'s mock IPC harness (the identifier defaults
  to an empty string in `mock_context`) resolves to the *real* host
  config directory, not an isolated tempdir — exercising it in an
  automated test would write into the actual machine's `~/.config`
  instead of a hermetic location, breaking this project's established
  "every filesystem-touching test uses `tempfile::tempdir()`" convention.
  Resolving the directory via `dirs::config_dir()` directly, in a function
  with no Tauri dependency at all, keeps the three new commands
  (`list_scan_profiles`/`save_scan_profile`/`delete_scan_profile`) plain
  functions, and keeps the actual storage logic (`profiles::load`/
  `upsert`/`remove`) taking an explicit `&Path` — fully unit-testable
  against a tempdir, the same shape `preview::build_data_url` already
  established for `GUI-MEDIA-PREVIEW`.
- **The real directory resolution itself is a trusted, untested boundary**
  — same category as `trash::delete`'s non-Linux behavior (ADR-0024) or
  reflink's non-CoW-filesystem path (ADR-0014): delegated entirely to a
  well-established crate, verified once by hand in this environment (see
  Consequences) rather than by an automated test that would need to touch
  real host state to exercise it. No test-only parameter was added to the
  command signatures to work around this — an IPC parameter that only
  tests ever populate would be scope creep against this project's own
  minimalism, not a real product need.
- **`ScanOptionsPayload` is reused directly as the persisted shape**,
  rather than introducing a second, parallel options type. It already
  represents "only what the user changed from default" (every scan
  tunable is `Option<T>`), which is exactly the right shape for a saved
  preset — and it already has the `Deserialize` impl needed for the IPC
  side; this change only added `Serialize`/`Clone`/`Default` so the same
  struct works for reading it back out of the saved JSON file too.
- **Saving under a name already in use is a deliberate overwrite, not an
  error** — matches how "Save" behaves for an existing preset in most
  apps, and keeps the upsert logic (`profiles::upsert`) simple: find by
  name, replace or push, persist the whole list.

## Consequences

- `rusty_fclone-gui` gains a new `profiles` module and three commands
  (`list_scan_profiles`, `save_scan_profile`, `delete_scan_profile`);
  `dirs` becomes a direct dependency (already present transitively,
  zero new supply-chain surface). No new Tauri capability/permission
  grant.
- Scan Setup gained a "Saved scan profiles" card: a name field plus
  "Save current setup," and a list of saved profiles each with "Load"/
  "Delete." Loading a profile reverse-maps the persisted, parsed
  `ScanOptionsPayload` back into the screen's string-based form fields
  (numbers and extension/path lists become the same comma-joined text the
  inputs already display and re-parse on save).
- `profiles::default_profiles_dir()`'s real OS-directory resolution was
  manually verified once in this environment (a scratch test, removed
  before committing): it resolved to `/root/.config/rusty-fclone` here,
  and a round-tripped save/reload matched the expected JSON shape exactly
  — see PROJECT-STATUS.md's Validation entry for this unit. It is not
  covered by an automated test for the reasons above; `profiles::load`/
  `upsert`/`remove`'s actual logic (given an explicit directory) is fully
  unit-tested instead.
- Saved profiles are scoped to `{name, root, ScanOptions}` only — no
  action-kind, keep-rule, reference-folder, or archive-directory
  configuration is saved alongside a profile (those live in
  `state.actionKind`/`state.keepRule`/`state.referencePaths`/
  `state.archiveDir`, deliberately untouched here), matching the plan's
  own scoping ("root + `ScanOptions` preset").
- The GUI's new "Saved scan profiles" card is not yet manually verified
  through a rendered window in this environment — no display/`xdotool`
  available, the same standing gap every GUI-facing unit this session has
  carried.
