---
name: roadmap-task
description: This skill should be used when the user asks to "pick up a roadmap item", "work on the roadmap autonomously", "implement something from ROADMAP.md", "grab a TODO from the roadmap", or wants Claude to choose and implement a planned feature from this repository's ROADMAP.md without further direction. Applies specifically to the rbx-native repository.
version: 0.1.0
---

# Roadmap task

Pick one item from `ROADMAP.md`'s "What's planned" section, implement it
end to end, and land it as a reviewable pull request — without needing
further direction once started.

## Before starting

Read, in order: the repository root `README.md`, `GUIDELINES.md`,
`SPECS.md`, and `agents/AGENTS.md`. These carry the actual rules this skill
operates under (code style, asset/licensing constraints, the verification
bar, what never to touch); this file only orchestrates the workflow.

Then read `agents/workflows/roadmap-task.md` — the full, tool-agnostic
procedure this skill is a thin entry point for. Follow it. What follows here
is the condensed version for quick reference; the referenced file has the
complete detail (branch-collision handling, exact git commands, PR
expectations).

## The loop

1. **Pick one item** from `ROADMAP.md`'s "What's planned" — one coherent,
   scoped piece of work. Don't edit `ROADMAP.md` yet to reflect this pick —
   that comes in step 6, once your own PR ships it.
2. **Check for an existing branch first.** `git fetch origin` and look for
   a branch matching the item before creating a new one — someone (another
   agent, a human) may already be on it. Continue that work instead of
   duplicating it if it's active; create a new branch off current `main`
   only if nothing matches, named per `CONTRIBUTING.md`'s convention
   (`feat/…`, `fix/…`, `docs/…`).
3. **Implement** following `GUIDELINES.md`/`SPECS.md`. Write real unit
   tests for the change — required, not optional. For anything visual
   (rendering, lighting, GUI), build the relevant binary and look at a
   screenshot before calling it done.
4. **Commit in small increments and stay synced with `main`.** Don't
   accumulate one giant commit at the end. Fetch and merge `origin/main`
   into the working branch periodically (every meaningful increment, or at
   least every 30–60 minutes of active work) — resolve any conflict
   immediately rather than letting it grow. Push regularly, not only at the
   end. Prefer `merge` over `rebase` on a branch that's already pushed or
   might be shared; never force-push over existing history.
5. **Verify.** `./scripts/check.sh` must be green (fmt, clippy `-D
   warnings`, the full test suite) before opening a PR.
6. **Open a pull request** (`gh pr create`), filling in
   `.github/PULL_REQUEST_TEMPLATE.md` for real: what changed and why, how
   it was verified, what's left undone, which roadmap item this addresses.
   Include the `ROADMAP.md` checkoff for that item in this same PR (move it
   to "What's been implemented", or flip it to `[x] 🚧` in place if only
   part of it shipped — see `agents/AGENTS.md`) so it's reviewed alongside
   the code rather than pushed on its own. Never merge it — land the work
   by opening the PR and stopping there; a human reviews and merges.

## Additional Resources

### Reference Files

- **`agents/workflows/roadmap-task.md`** (repository root) — the complete
  procedure this skill condenses, including exact git commands for the
  branch-collision check and the sync cadence, and what to do when a
  matching branch is found abandoned versus active.
- **`agents/AGENTS.md`** (repository root) — the standing rules this
  workflow operates inside: code style, asset/licensing constraints, the
  verification bar for visual changes, token economy, and the explicit
  authorization this skill exercises (pick anything from the roadmap,
  branch, implement, test, PR — without asking first).
