# ADR-0020: GUI via Tauri, reversing the v1 "no GUI" non-goal

- Status: Accepted
- Date: 2026-08-25
- Related: ADR-0004 (engine API/streaming contract this reuses), ADR-0005
  (crate boundaries, extended by this decision), ADR-0006 (dependency
  policy), ADR-0009 (action-layer safety model, preserved here), ADR-0015
  (CLI-UX's NDJSON event shape, mirrored by this decision's wire format),
  ADR-0017 (rusqlite-bundled precedent for the C-toolchain exception this
  decision extends)

## Context

`SYSTEM-ARCHITECTURE.md`'s Non-goals and `CLI-UX-001`'s Non-goals both
explicitly named "a GUI" as out of scope for v1 — a deliberate, stated
product decision, not an oversight. That decision is being reversed: a
GUI is now wanted, on top of the same detection/action engine the CLI
already consumes.

Two design questions: which toolkit, and how the GUI talks to
`rusty_fclone-core` without duplicating or bypassing the CLI's existing
safety/streaming contracts.

## Decision

- **Toolkit: Tauri (v2), not egui/iced/slint.** A Rust backend plus an
  HTML/CSS/JS frontend rendered in the OS's system webview. Chosen over
  the pure-Rust alternatives for a more customizable, standard web-tech UI
  surface — accepting the tradeoff below as a deliberate, informed choice,
  not a default.
- **New workspace crate: `rusty_fclone-gui`** (ADR-0005 extension), not
  code inside `rusty_fclone-cli`. Same reasoning as the existing
  core/CLI split: the GUI is a consumer of `rusty_fclone-core`'s public
  API, with no detection/action logic of its own — `rusty_fclone-core`
  gains no GUI-specific code, no `serde` dependency, and no awareness that
  a GUI exists.
- **The C-toolchain rule's exception, exercised a second time.**
  `AGENTS.md`'s "no dependency that requires a C toolchain" rule already
  has one precedent exception: `rusqlite`'s `bundled` feature (ADR-0017).
  Tauri's Linux backend links against the system webview
  (`libwebkit2gtk-4.1`, `libgtk-3`, `libsoup-3.0`, and related `-dev`
  packages at build time) — a real C-toolchain-and-system-library
  dependency, at a larger scale than ADR-0017's vendored SQLite. Accepted
  for the same reason the rule exists to weigh against, not exempt from:
  this is the one capability (a real, standard-webview GUI) that cannot be
  built any other way without a much larger hand-rolled cost (a pure-Rust
  toolkit would avoid this, at the cost of a less customizable UI — see
  "Alternatives"). CI's `ubuntu-latest` runner now installs those packages
  before building (`.github/workflows/ci.yml`); `release.yml` does not yet
  build or bundle the GUI (see Consequences).
- **Wire format mirrors `CLI-UX-001`'s NDJSON event shape, not a new
  design.** The GUI's `crate::payload` module defines its own `serde`
  DTOs (`rusty_fclone_core`'s types stay `serde`-free, deliberately, since
  the CLI's own JSON output already made that same choice — ADR-0015)
  with the same field names and event shapes as `CLI-UX-001-FR-002`
  through `FR-005`'s NDJSON schema, tagged the same way (`type` field,
  `snake_case` tag values). A reader who knows one wire format already
  knows the other. Emitted as Tauri events (`scan-event`) rather than
  NDJSON lines, since Tauri's IPC is the transport here, not stdout.
- **Streaming, not collected.** `start_scan` spawns a background
  `std::thread`, iterates the same `ScanHandle` the CLI consumes, and
  `emit`s one Tauri event per `ScanEvent` as it arrives — preserving
  ADR-0004's "duplicate groups appear as found, not batched until the
  whole tree finishes" contract at the GUI layer too, not just the CLI's.
- **The action-layer safety model is preserved, not re-decided.** The
  GUI's `run_action` command takes the same `(kind, apply)` split as the
  CLI's `--action`/`--apply` two-flag gate (ADR-0009): `apply: false`
  (the frontend's default, an unchecked checkbox) only calls
  `action::plan` and previews; `apply: true` is required to actually call
  `action::apply`. No GUI-specific bypass of "preview first."
- **No file-picker dialog for v1.** The root path is a plain text field,
  not a native directory-picker (which would need Tauri's `dialog`
  plugin, another dependency and another permission to reason about). A
  picker is a natural, low-risk follow-up, not attempted here — see
  `GUI-UX-001`'s open questions.
- **Vanilla JS frontend, no npm dependency.** `tauri.conf.json` sets
  `app.withGlobalTauri: true` so the frontend can call `invoke`/`listen`
  via the injected `window.__TAURI__` global, and `build.frontendDist`
  points at a plain static folder (`ui/`) with no bundler
  (`beforeBuildCommand`/`beforeDevCommand` both unset). Keeps this
  decision from also deciding a JS framework, and keeps the dependency
  surface to what `AGENTS.md`'s "no dependency added without
  justification" rule already requires reasoning about — one new
  ecosystem (Tauri/system-webview) is enough for one ADR.

## Alternatives considered

- **egui/eframe**: pure Rust, immediate-mode, no C toolchain or system
  webview — would have kept the "no C toolchain" rule intact rather than
  extending its exception. Rejected in favor of a more customizable,
  standard web-tech UI (an explicit, informed tradeoff — not a default;
  see "Decision" above for what's accepted in exchange).
- **iced**: pure Rust, Elm-architecture — same toolchain benefit as egui,
  steeper integration against the existing thread+channel scan model
  (would need a `Subscription` adapter around `ScanHandle`). Same
  rejection reasoning as egui.
- **slint**: pure Rust runtime, declarative `.slint` markup — GPL/
  royalty-free for open-source use, a commercial license needed
  otherwise. Not chosen; not directly comparable on cost/benefit to the
  above without a licensing decision this ADR doesn't need to make.

## Consequences

- New dependencies: `tauri`, `tauri-build` (workspace-visible via
  `rusty_fclone-gui`'s `Cargo.toml`, not added to `workspace.dependencies`
  since no other crate needs them), plus their transitive system-library
  requirements on Linux (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`,
  `librsvg2-dev`, `libayatana-appindicator3-dev`, `libssl-dev`,
  `libsoup-3.0-dev`) — and, on Windows, the MSVC C++ toolchain
  (`embed-resource`'s `vswhom-sys` dependency needs `cl.exe` and
  `windows.h` to embed the `.exe`'s icon/manifest, confirmed the hard way
  when a Windows build hit `C1083: Cannot open include file: 'windows.h'`
  from a plain terminal — the fix is building from an "x64 Native Tools
  Command Prompt for VS" so `vcvars64.bat` has set `INCLUDE`/`LIB` first,
  not a code change here). CI only validates the Linux path (its only
  runner); this environment has no Windows machine to validate the
  Windows path against, so it's documented, not CI-verified.
- `AGENTS.md`'s "no C toolchain" rule now carries two precedent
  exceptions (ADR-0017, this one) instead of one — both documented at the
  rule itself, not just in ADR history, so a reader doesn't take the rule
  as still-absolute.
- `.github/workflows/ci.yml` installs the Linux system packages above
  before `cargo fmt`/`clippy`/`test`/`bench --no-run`/`doc`, all of which
  now cover `rusty_fclone-gui` too as a workspace member.
- `.github/workflows/release.yml` is **unchanged** — it still only builds
  and packages `rusty_fclone-cli`. Producing installable GUI bundles
  (`.deb`/`.AppImage`/`.dmg`/`.msi` via `tauri build`'s bundler, which
  needs full per-platform prerequisites beyond what CI's build-and-test
  step installs, plus real application icons in every format the bundler
  targets) is a real follow-up, not attempted here — see
  `docs/roadmap/ROADMAP.md`'s new `GUI-RELEASE-BUNDLES` entry.
- Icon assets (`crates/rusty_fclone-gui/icons/*.png`) are placeholder
  solid-color squares generated for this change, not real application art
  — sufficient for `tauri::generate_context!`'s compile-time icon
  embedding (which only requires a valid PNG to exist on Linux targets)
  and for `cargo build`/`clippy`/`test`, insufficient for a real release.
  No `.ico`/`.icns` were generated at all (Windows/macOS-specific bundle
  icon formats, unused by any check this environment can run).
- Verified end-to-end in this environment via Xvfb (a virtual X display,
  since this container has no real display): the compiled binary launches,
  renders the real frontend (not a stale cached build — caught and fixed
  once during this change), and a full scan → duplicate-group display →
  preview action → apply action cycle was driven through the actual
  rendered UI with `xdotool` (not just unit tests), confirmed against real
  filesystem state before and after. Not verified: macOS or Windows
  rendering (no such environment available here), or any packaged/bundled
  distribution artifact.
- Test coverage is IPC-level, not full end-to-end-in-CI: `commands.rs`
  uses `tauri::test`'s mock runtime (`mock_builder`, `get_ipc_response`)
  to invoke `run_action`/`start_scan` through the real Tauri command
  dispatch path, asserting on real filesystem effects — deterministic and
  fast, unlike a real-webview run, and this is what CI actually exercises
  going forward (the Xvfb/`xdotool` pass above was a one-time manual
  verification for this change, not wired into CI).
