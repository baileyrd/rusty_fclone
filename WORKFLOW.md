# Repository Development Workflow

## Authority
`main` is authoritative. Every change lands through a branch, a PR against
`main`, and a merge commit on green CI — never a direct push.

## Executor detection
Detected fresh each session from environment capabilities, never from a
repository-stored flag. See the `rust-repo-lifecycle` skill's
`references/executor-modes.md` for the full detection logic: a session with
its own shell, repo checkout, and GitHub access that can itself edit,
commit, push, open PRs, and merge runs in **Claude mode** (autonomous, one
continuous session); a planner-only session with no shell of its own,
relaying instructions to a human who drives a separate Codex agent, runs in
**ChatGPT+Codex mode**.

## Roles
- Planner/reviewer: repository-aware planner, instruction author, PR
  reviewer, correction author, merge gate.
- Implementer: bounded implementer and validator — either the same session
  (Claude mode) or a separate agent a human relays to (Codex mode).
- Human (Codex mode only): coordinator who transfers prompts and
  opens/updates PRs.

## Source of truth
- Treat current `main` as authoritative once it has commits.
- Read `AGENTS.md` and `docs/PROJECT-STATUS.md` plus their routed
  authorities before planning the next unit.
- Inspect commits after the recorded checkpoint in `PROJECT-STATUS.md`.
- Report conflicts between chat/task history and repository state; never
  resolve them by trusting conversation memory over repository evidence.

## Outer loop
1. `next` — planner inspects current state (`PROJECT-STATUS.md`, roadmap,
   spec registry, traceability) and produces one complete implementation
   packet for one dependency-ready unit.
2. (Codex mode: user relays it to Codex.) Implementer implements, validates,
   commits, and reports.
3. PR opened — `PR created`.

## Inner loop
1. Reviewer inspects the actual exact head, diff, scope, authorities,
   tests, docs, threads, and CI.
2. Pass → merge the exact reviewed head.
3. Otherwise → one correction packet; implementer updates the same branch;
   `branch updated`; re-review the new exact head.

## Safeguards
- Never merge failing, pending, missing, stale, or older-head CI.
- Restart review if the head changes.
- Don't begin a competing increment while a PR is active.
- Don't let the planner implement during the `next` planning step in Codex
  mode.
- Distinguish code failures from infrastructure/account failures.
- Don't silently expand scope or resolve authority conflicts — raise them.

## ADRs
Write one per delivery cycle during active major development (the project
is currently in that phase — see `docs/decisions/` for the accumulated
log); taper to decisions-that-matter once the baseline is stable and
complete. See the `rust-repo-lifecycle` skill's `references/adr-cadence.md`.

## `next`
Verify merge state, refresh `main`, reconcile `PROJECT-STATUS.md` against
reality, select the next dependency-ready unit from
`docs/roadmap/ROADMAP.md`.
