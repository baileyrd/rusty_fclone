# Docs audit — 2026-08-25

Scope: whole tracked `*.md` surface (31 docs), prompted by "README doesn't
address the UI piece." Doc-comments (`///`/`//!`) not audited (not
requested). Harness mode: interactive (unset) — nothing below is applied
without sign-off.

Ground truth built from: `Cargo.toml`/`Cargo.lock`, `cargo test --workspace`
(76/76, confirmed), `git ls-files`, `crates/rusty_fclone-core/src/traversal.rs`,
`scripts/inventory_docs.sh` (drift ranking), `scripts/check_references.py --all`
(152 references checked: 2 broken, 19 unresolved-and-reviewed — see below),
and the spec/ADR files themselves.

## Findings

| Doc | Where | Claim | Classification | Ground truth | Fix | Size |
| --- | --- | --- | --- | --- | --- | --- |
| README.md | whole file | — (no mention of UI/GUI anywhere) | missing | `CLI-UX-001.md` Non-goals: "A GUI, TUI, or anything beyond a plain terminal/pipe-friendly CLI"; no GUI/TUI dependency or code anywhere in the repo (`grep` for egui/iced/tauri/ratatui/etc. — zero hits) | Add a short line stating this is CLI-only by design, linking to `CLI-UX-001.md`'s Non-goals — this is the row that prompted the run | S |
| AGENTS.md | `Change rules`, L49 | "see `docs/decisions/adr-cadence` guidance in the `rust-repo-lifecycle` skill" | stale | No `docs/decisions/adr-cadence` path exists in this repo — `check_references.py` flags it `broken`; the sentence means a reference *inside* the skill, not a repo file, but reads as a repo-relative path | Reword so it's unambiguous the path is internal to the skill, not this repo | S |
| AGENTS.md | `Project shape`, L9 | "an action layer (delete/hardlink)" | stale | `action.rs`'s `ActionKind` has a third variant, `Reflink` (ADR-0014, shipped); CLI `--action reflink` exists | Add reflink to the list | S |
| AGENTS.md | `Architectural boundaries`, L26–28 | "No dependency that requires a C toolchain... If a change needs one, that's an ADR-worthy decision" | stale | `rusqlite`'s `bundled` feature (added by ADR-0017) vendors and compiles SQLite's C source via `libsqlite3-sys` — the rule's own escape hatch was already exercised, but the rule reads as still-absolute | Note the ADR-0017 precedent so a reader doesn't take the rule as literally unbroken | S |
| WORKFLOW.md | `Authority`, L4–5 | "`main` is authoritative once this baseline is merged. Until then, this branch (`claude/custom-fclone-detection-bufv7b`) is the working state." | orphaned | `main` has 19+ merged PRs and has been the sole target of every branch→PR→merge cycle this session; the named branch doesn't exist in current history | Cut or rewrite — this describes a bootstrap-phase state that's long over | S |
| WORKFLOW.md | `ADRs`, L59 | "six ADRs exist already for the v1 baseline" | stale | `git ls-files docs/decisions/` → ADR-0001 through ADR-0019 (19 ADRs) | Update the count or drop the specific number | S |
| WORKFLOW.md | `ADRs`, L61 | "See the `rust-repo-lifecycle` skill's `references/adr-cadence.md`" | accurate | Same skill-internal reference as AGENTS.md L49, but this one is phrased unambiguously (not styled as a repo path) — no fix needed | — | — |
| SYSTEM-ARCHITECTURE.md | `Purpose`, L8 | "reflink deferred, see ADR-0009" | stale | Reflink shipped via ADR-0014 (`ACTION-REFLINK`, done); ADR-0009 is the original action-layer ADR, predates the reflink decision | Update to reflect reflink is shipped, cite ADR-0014 | S |
| SYSTEM-ARCHITECTURE.md | `Product boundary` → Non-goals, L18 | "Non-goals for v1: reflink support, ..." | stale | Same — reflink is implemented, not a non-goal | Drop "reflink support" from the non-goals list | S |
| SYSTEM-ARCHITECTURE.md | `Detection pipeline` diagram, L21–57 | (diagram has no cache/import stage before "full hash") | missing | `pipeline.rs`'s full-hash stage checks `--cache` (ADR-0016) then `--import-fclones-cache` (ADR-0019) before any real read | Add a short cache-check box ahead of "full hash" in the diagram, or a caption note | M |
| SYSTEM-ARCHITECTURE.md | `Data flow / ownership`, L75 | "`traversal::traverse` produces `Vec<Candidate>`" | orphaned | `traversal.rs`'s `traverse` signature takes `on_candidate: impl FnMut(Candidate)` and returns `()` — changed by ADR-0012 (`DETECTION-TRAVERSAL-COLLAPSE-FUSION`), never updated here | Rewrite to describe the callback, not a returned `Vec` | S |
| SYSTEM-ARCHITECTURE.md | `Where to look next`, L119 | "Decisions: `docs/decisions/ADR-0001` through `ADR-0009`." | stale + broken-path | `check_references.py`: `broken` (path missing the descriptive filename suffix each ADR file actually has); range is also stale — 19 ADRs exist, not 9 | Fix the path form and the range (or just say "see `docs/decisions/`") | S |
| docs/specifications/SPEC-REGISTRY.md | versions column | `FCLONE-DETECTION-001` 0.1.9, `FCLONE-ACTION-001` 0.2.0, `CLI-UX-001` 0.2.0 | accurate | Matches each spec file's own `- Version:` header exactly | None | — |
| docs/PROJECT-STATUS.md | `Validation` | "`cargo test --workspace`: pass, 76/76" | accurate | Re-ran: 15 (cli) + 61 (core) = 76, all passing | None | — |
| docs/roadmap/ROADMAP.md, ADR log (ADR-0001–0019) | — | — | accurate (spot-checked) | Content matches shipped behavior for the units checked in this run; append-only per Rules, not re-litigated further | None | — |

## Reviewed, not findings

`check_references.py --all` flagged 19 more `unresolved` inline-paths beyond
the two `broken` ones above. All read and confirmed non-issues, not silently
dropped:
- `benches/detection.rs` used as informal shorthand in `FCLONES-COMPARISON.md`,
  `ADR-0006`, `SPEC-REGISTRY.md`, `FCLONE-DETECTION-001.md` — each occurrence
  is inline prose in a paragraph that already names the crate; the full path
  (`crates/rusty_fclone-core/benches/detection.rs`) appears correctly
  elsewhere (e.g. `ROADMAP.md`). Not worth a row.
- `src/main.rs` in `CLI-UX-001.md` — same pattern, sentence already names
  `rusty_fclone-cli`.
- `queue/` in `ADR-0013` — a Linux sysfs path fragment
  (`/sys/dev/block/.../queue/rotational`), not a repo-relative claim.
- `hasher.rs`/`file.rs`/`group.rs` in `ADR-0019`, `actions/checkout` /
  `softprops/action-gh-release` in `ADR-0018`/`ROADMAP.md` — these correctly
  name files in *fclones' own upstream repo* or GitHub Action identifiers,
  not paths in this tree.

## Counts

| Classification | Count |
| --- | --- |
| missing | 1 |
| stale | 8 |
| orphaned | 2 |
| aspirational | 0 |
| unverifiable | 0 |
| accurate | 4 (+ the 19 reviewed-non-issues above) |

## Auto-eligible under `LOOP_HARNESS_MODE=auto` (not set — all rows wait for pick either way)

Would qualify as auto-eligible (transcription from a verifiable source, no
judgment call): the two `broken`-path rows (AGENTS.md L49, SYSTEM-ARCHITECTURE.md
L119), the WORKFLOW.md ADR-count row, and AGENTS.md's missing-reflink row.
Everything else — the README UI addition, WORKFLOW.md's Authority section,
the pipeline-diagram update, the data-flow rewrite, and the C-toolchain-rule
note — involves picking *what to say*, not just correcting a fact, so all of
it would pause for sign-off in auto mode too.

## No code-is-the-suspect-party findings

Everything above is a doc lagging real, intentional, already-ADR'd changes.
Nothing here suggests the code is behaving wrong.

## Resolution (step 5 verify) — 2026-08-25

All 12 actionable rows approved by the user ("All 12 findings") were fixed
and merged, one PR per doc file:

| Doc | PR | Rows fixed |
| --- | --- | --- |
| README.md | [#20](https://github.com/baileyrd/rusty_fclone/pull/20) | the `missing` UI/GUI row |
| AGENTS.md | [#21](https://github.com/baileyrd/rusty_fclone/pull/21) | the `adr-cadence` path, reflink-in-action-layer, C-toolchain-precedent rows |
| WORKFLOW.md | [#22](https://github.com/baileyrd/rusty_fclone/pull/22) | the `orphaned` Authority section, the ADR-count row |
| SYSTEM-ARCHITECTURE.md | [#23](https://github.com/baileyrd/rusty_fclone/pull/23) | reflink Purpose/Non-goals, the pipeline-diagram cache note, the `traverse` callback rewrite, the `ADR-0001`–`ADR-0009` path+range |

`scripts/check_references.py --all` re-run against the merged result: **0
`broken` references** (down from 2 — both the `AGENTS.md` `adr-cadence` path
and the `SYSTEM-ARCHITECTURE.md` `ADR-0001` path resolve or are now
unambiguous). The same 19 `unresolved` hits from "Reviewed, not findings"
above are still present and still non-issues (unchanged code, re-confirmed
by spot check, not re-litigated row by row).

Counts after: `missing` 0, `stale` 0, `orphaned` 0 — every row above that
carried one of those three classifications is now `accurate`. `accurate` and
the reviewed-non-issue rows are left as-is per the Persistence rule, so a
future run starts from these verdicts instead of re-checking them.

No new documented commands were introduced by any of the four fixes (all
were prose/path/diagram-caption edits), so step 5's "execute the read-only
commands" check doesn't apply to this batch's new content;
`cargo test --workspace` was already re-confirmed passing (76/76) as part of
step 1's ground truth and nothing in this batch touches code.

---

# Docs audit — 2026-08-27 (re-run)

Scope: whole tracked `*.md` surface (45 docs — up from 31 at the last run;
23+ PRs landed in between, closing out
`docs/roadmap/DEDUP-GAP-IMPLEMENTATION-PLAN.md` in its entirety). Prompted
by a `/docs-loop` invocation, no specific doc named. Doc-comments not
audited (not requested). Harness mode: interactive (`LOOP_HARNESS_MODE`
unset) — nothing below is applied without sign-off.

Ground truth built from: `Cargo.toml`/each crate's `Cargo.toml`,
`crates/rusty_fclone-core/src/action.rs`'s `ActionKind` enum, a real
`cargo run -p rusty_fclone-cli -- --help` capture diffed against README's
Usage section, `git ls-files`, `scripts/inventory_docs.sh` (drift ranking),
`scripts/check_references.py --all` (451 references: 1 `broken`, 73
`unresolved`, 16 `historical-inline-path` — see below), and the spec/ADR/
roadmap files themselves. The prior run's "Resolution" section above (PRs
#20-#24) was re-verified as still holding, not re-litigated.

## check_references.py: the one `broken` hit

`docs/PROJECT-STATUS.md:127` names `scripts/check_references.py` as a
repo-relative path. It resolves against *this skill's* script (outside the
repo), inside prose narrating a past docs-loop run — the same structural
false-positive class the skill's own Limitations section calls out (a doc
correctly describing a different component's layout). Reviewed, not a
finding.

## Findings

| Doc | Where | Claim | Classification | Ground truth | Fix | Size |
| --- | --- | --- | --- | --- | --- | --- |
| `AGENTS.md` | `Project shape`, L9 | "an action layer (delete/trash/hardlink/reflink)" | stale | `action.rs`'s `ActionKind` also has `Move`/`Copy` (`ACTION-MOVE-COPY`, ADR-0026, shipped); CLI `--action move`/`copy` and GUI archive-folder field both exist | Add move/copy to the list | S |
| `AGENTS.md` | `Change rules`, L69-70 | "Update the relevant spec (`FCLONE-DETECTION-001.md`, `FCLONE-ACTION-001.md`, or **a future one**)" | stale | Two more specs already exist and are actively maintained: `docs/specifications/cli-ux/CLI-UX-001.md`, `docs/specifications/gui-ux/GUI-UX-001.md` — "a future one" undersells what's already shipped | Name both existing specs explicitly | S |
| `docs/architecture/SYSTEM-ARCHITECTURE.md` | `Where to look next` → Specs, L172-174 | Lists `FCLONE-DETECTION-001.md`, `FCLONE-ACTION-001.md`, `GUI-UX-001.md` | missing | `docs/specifications/cli-ux/CLI-UX-001.md` exists (0.3.6, actively maintained) and isn't listed | Add it to the list | S |
| `docs/roadmap/DEDUP-GAP-IMPLEMENTATION-PLAN.md` | Header, L9-15 | "Status of this document: a proposal, not a commitment... fold the chosen rows into `ROADMAP.md`... this plan can be trimmed to a changelog note or removed" | orphaned | Every unit in this plan (all 3 phases, 10 units) is `Done` and already folded into `ROADMAP.md`, confirmed by grepping every unit name there — the doc's own stated exit condition has been reached | Update the status note to reflect completion (or trim per its own instruction — user's call, see below) | S |
| `docs/roadmap/ROADMAP.md` | `GUI-MEDIA-PREVIEW`/`SCAN-PROFILES`/`DETECTION-PERCEPTUAL-IMAGES`/`DASHBOARD-CHART-UPGRADE` rows, Evidence column | Each: "not yet manually verified through the rendered UI (no Xvfb/`xdotool` pass this session)" | stale | `GUI-UX-001.md`'s own Verification plan (0.4.1 entry) documents a real Xvfb+`xdotool` pass, merged this session (PR #47), that verified: `GUI-MEDIA-PREVIEW`'s photo path (not audio), `SCAN-PROFILES`'s card rendering (not save/load/delete interaction), `DETECTION-PERCEPTUAL-IMAGES` end-to-end, `DASHBOARD-CHART-UPGRADE` end-to-end (chart/tooltip/legend) | Update each row's Evidence text to match what `GUI-UX-001.md` and `PROJECT-STATUS.md` already record as verified vs. still-open | M |
| `docs/traceability/TRACEABILITY.md` | `FR-026`/`FR-027`/`FR-028`/`FR-029` rows, Status column | Each: "no manual Xvfb/`xdotool` verification through the rendered UI yet" | stale | Same ground truth as the `ROADMAP.md` row above | Same correction, mirrored into the Status column | M |
| `docs/PROJECT-STATUS.md` | Header, L2, L5-6, L23-25; `## In progress`, L774-776 | "Last verified main commit: `4e34dae`"; branch `gui-scan-layout-fix` described as "not yet merged" / "implemented, validated, not yet merged" | stale | `main` is at `c67f63c` (PR #47 merged this session, branch deleted); `AGENTS.md`'s own "Definition of done" requires `PROJECT-STATUS.md` updated after every merge — this update didn't happen yet | Rewrite the header and `## In progress` to reflect the merge | S |

## Reviewed, not findings

- `docs/benchmarks/FCLONES-COMPARISON.md` ranked highest on the drift
  scanner (36 code commits since last touched) but nothing since has
  changed the benchmarked hot path (traversal/hashing) — every unit in
  between was additive (filters, actions, GUI, perceptual images, a
  separate opt-in pass). Spot-checked the header claims; still accurate.
  Not re-benchmarked (no code change to justify it).
- `WORKFLOW.md` — the two rows the last run fixed (orphaned Authority
  section, hardcoded ADR count) stayed fixed; no new drift found on a
  fresh read.
- The 73 `unresolved` + 16 `historical-inline-path` hits from
  `check_references.py --all`: spot-checked a sample (`ui/app.js`-style
  paths in `GUI-UX-001.md`/`TRACEABILITY.md` are inline shorthand in
  sentences that already name `crates/rusty_fclone-gui`; `docs-audit.md`'s
  own `historical-inline-path` hits are the prior run's resolution record,
  correctly left alone). Same pattern as the last run's 19 reviewed hits;
  not worth a row each.
- README.md's Usage section vs. a real `cargo run -- --help` capture:
  every flag name, default value, and possible-value set matches. The
  prose is a hand-formatted paraphrase (wrapped lines, `<BYTES>` instead of
  clap's derived `<SMALL_FILE_THRESHOLD>`), not a verbatim dump — a
  stylistic choice, not drift. No finding.
- `README.md`, `SYSTEM-ARCHITECTURE.md` (body), `docs/specifications/
  SPEC-REGISTRY.md`, `docs/specifications/gui-ux/GUI-UX-001.md`: all
  freshly authored or revised this session (0-2 commits since last
  touched per the drift scanner) and already cross-checked against the
  code during that work. Spot-checked, no new drift found.

## Counts

| Classification | Count |
| --- | --- |
| missing | 1 |
| stale | 5 |
| orphaned | 1 |
| aspirational | 0 |
| unverifiable | 0 |
| accurate (reviewed this run) | 5 doc groups (+ the 73/16 reference-check hits, sampled) |

## Auto-eligible under `LOOP_HARNESS_MODE=auto` (not set — all rows wait for pick either way)

The `AGENTS.md` move/copy row, the `SYSTEM-ARCHITECTURE.md` missing-spec
row, and the `PROJECT-STATUS.md` merge-sync row are transcription from a
verifiable source (an enum variant, a file's existence, `git log`) — would
qualify. The `AGENTS.md` "a future one" row, the `ROADMAP.md`/
`TRACEABILITY.md` verification-status rows, and the `DEDUP-GAP-
IMPLEMENTATION-PLAN.md` status note all involve picking *how much detail to
say*, not just correcting one fact — would pause for sign-off even in auto
mode.

## No code-is-the-suspect-party findings

Everything above is a doc lagging real, already-shipped, already-tested
changes (mostly this session's own). Nothing here suggests the code is
behaving wrong.
