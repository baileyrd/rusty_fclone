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
