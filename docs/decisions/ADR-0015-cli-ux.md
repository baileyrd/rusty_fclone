# ADR-0015: CLI-UX — JSON output, scan progress, confirmation prompt

- Status: Accepted
- Date: 2026-08-24
- Related: ADR-0004 (streaming API/finality contract, extended here),
  ADR-0005 (core stays CLI-agnostic), ADR-0009 (action layer's existing
  dry-run/`--apply` safety model, extended here)

## Context

The v1 CLI only had plain-text output and a two-flag (`--action`+`--apply`)
safety model with no interactive confirmation. The roadmap's `CLI-UX` unit
named three gaps: machine-readable output, progress reporting on long
scans, and an interactive confirmation prompt as a second safety layer
beyond `--apply`. All three are pure CLI-crate concerns — nothing here
changes the detection or action algorithms, only how the CLI presents and
gates them (ADR-0005).

## Decision

### `ScanEvent::Progress`

A new `ScanEvent::Progress(ScanProgress)` variant, added to
`rusty_fclone_core`'s public API (extending ADR-0004): `ScanProgress {
files_scanned: u64, bytes_scanned: u64 }`, a cumulative running counter —
there's no way to know the total file count in advance without a separate
full pre-walk, so this is not a percentage. Emitted from
`pipeline::run_scan`'s `on_candidate` callback every 256 files (a count
threshold, not a wall-clock timer: deterministic, no extra dependency, and
cheap enough not to matter next to real I/O). Only appears during
traversal, always before `Finished` — it doesn't touch the "no group
revision after emission" invariant `ScanEvent::Finished`/`DuplicateGroup`
already have.

### `--format text|json`

`text` (default) is today's human-readable output, unchanged. `json`
emits one JSON object per line (NDJSON) to stdout — `duplicate_group`
(with an optional nested `action` object when `--action` is set),
`error`, `progress`, `finished`, and a final `action_summary` mirroring
the text mode's trailer line. NDJSON rather than one big JSON array/object:
matches the engine's streaming design (ADR-0004) instead of fighting it —
a consumer can start processing the first duplicate group's JSON line
before the scan finishes, same as text mode already allows visually.
JSON-serializable types (`JsonEvent`, `JsonAction`) live in the CLI crate,
not core — core stays output-format-agnostic (ADR-0005); paths are
rendered via `.display().to_string()` (lossy for non-UTF-8 paths, same
tradeoff every string-based JSON path representation makes).

### Progress line rendering (text mode)

A live, in-place-overwriting `scanning... N files, M bytes` line on
stderr, updated via `\r` and padded to erase any leftover characters from
a longer previous line, cleared before any other output (a duplicate
group, an error, the final summary) is printed so nothing collides
mid-line. Gated on `std::io::IsTerminal`: only shown when stderr is an
actual terminal. Piped/redirected output (logs, CI, `| jq`) gets no
progress spam — confirmed via manual smoke test showing zero `\r`
characters in that case. In `--format json`, `Progress` is instead just
another NDJSON line (no in-place-overwrite semantics apply to a
machine-readable stream).

### Confirmation prompt

Before `--apply` mutates anything (and only then — `--action` alone,
without `--apply`, is already a no-op preview), the CLI prompts on stderr
and reads a yes/no answer from stdin, bypassable with `-y`/`--yes`. This
is a **general warning naming the root and action, not a precise
preview**: the pipeline applies each duplicate group's action
incrementally as it's found (ADR-0004's streaming design), so exact
total bytes/files aren't known until the scan finishes — by which point
it's too late to ask "proceed?" before anything happened. A prompt with
real-time-accurate totals would require either buffering every group
before applying anything (abandoning the streaming architecture for this
one path) or prompting once per group (worse UX for a tree with many
groups). Neither trade felt worth it for what's explicitly a *second*
safety layer on top of the already-required `--apply` flag (ADR-0009) —
the general warning is honest about what's about to happen without
overselling precision it can't have yet.

The decision logic (`confirm(reader: impl BufRead) -> bool`, accepting
`y`/`yes` case-insensitively) is factored out from the actual
stdin/terminal I/O (`confirm_apply`), so it's unit-testable without a real
interactive session. The full path (prompt → decline → no mutation) is
covered by a CLI-level test using an unanswered (EOF) stdin, which
reliably declines; the accept path is manually smoke-tested (feeding `y`
via a pipe) since asserting on it doesn't require anything the decline
test doesn't already establish about the wiring.

## Consequences

- New dependencies: `serde` (`derive` feature) and `serde_json`, CLI-crate
  only.
- `ScanEvent` gains a fourth variant; the CLI's match on it (and any other
  exhaustive `match ScanEvent { ... }`) needed a new arm — caught at
  compile time.
- Existing tests that exercise `--apply` (`action_with_apply_actually_*`)
  now also need `yes: true` in their `Cli` construction, since the new
  confirmation gate would otherwise decline (empty/EOF stdin in the test
  harness) and silently no-op the mutation those tests are checking for.
- No change to detection or action semantics — this ADR is entirely about
  the CLI's presentation and a second opt-in confirmation gate.
- Closes the `CLI-UX` roadmap unit (`CLI-UX-001`).
