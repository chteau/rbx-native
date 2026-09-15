# Standing rules for AI coding agents

Read this file before making any change in this repository, then
[`GUIDELINES.md`](../GUIDELINES.md) and [`SPECS.md`](../SPECS.md) at the repo
root for the Rust style and architecture rules. If a task brief conflicts
with something here, the task brief wins for that task only — these are the
defaults, not an override.

See [`README.md`](README.md) in this directory for why this file lives here
instead of the repo root.

## Picking up work from the roadmap

You're free to choose anything listed under "What's planned" in
[`ROADMAP.md`](../ROADMAP.md) and implement it without asking first. You're
authorized to:

- Create a new branch for it (never push to `main` directly — see
  "Committing and proposing changes" below).
- Implement the feature.
- Write real unit tests for what you implement — not optional, this
  project holds every change to that standard (see "Verifying a change"
  below); "it compiles" is not "it works."
- Open a pull request using the repository's template
  (`.github/PULL_REQUEST_TEMPLATE.md`).

**Never edit `ROADMAP.md` itself.** Roadmap direction is the maintainer's
call only — even marking your own item done isn't yours to do by editing
the file; that happens when your PR is reviewed and merged. Say what you
finished in the PR description instead.

For the full step-by-step procedure — including how to check whether a
branch for the item already exists, and how to stay synced with `main`
(commit cadence, when to merge, when never to force-push) while you work —
see [`agents/workflows/roadmap-task.md`](workflows/roadmap-task.md). Claude
Code has a skill (`.claude/skills/roadmap-task/`) that walks this
automatically; other tools should follow the same document directly.

## Code

- `cargo fmt` clean, `cargo clippy --workspace --all-targets -- -D warnings`
  clean, files under ~400 lines (split into submodules rather than growing
  one file — see `GUIDELINES.md` §6 for the layout convention this repo
  uses).
- Minimize `pub` scope (`pub(crate)`/`pub(super)` over `pub` by default).
- Comments explain **why**, not what — see `GUIDELINES.md` §2. Never
  reference a specific task, a previous change, or another agent/session in
  a comment; comments describe the code as it stands, not its history.
- No new dependency without a one-line reason. Prefer a crate already in
  `Cargo.lock` over adding an equivalent one. Check licence and dependency
  weight before pulling in anything non-trivial (this project has turned
  down at least one crate for pulling in a full physics engine as a
  transitive dependency for a feature that needed none of it).
- Prefix `cargo` commands with `ionice -c3` where you can — a full rebuild of
  this workspace is heavy, and a maintainer's machine may be doing something
  else at the same time.

## Verifying a change

- `./scripts/check.sh` from the repo root is the gate: `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, then a `TOTAL PASSED: N` line. A change isn't
  done until this is green.
- CI (`.github/workflows/ci.yml`) runs the same gate on Linux, plus a
  build-and-test job on Windows, on every branch. It's your safety net for
  the platform you're not on, not a reason to skip running the gate
  yourself on the one you are.
- When you're changing rendering, lighting, or anything visual, a passing
  test suite is necessary but not sufficient — build the relevant binary and
  actually look at a screenshot before claiming something is fixed. This
  project's history has more than one case of a change that passed every
  test and still produced a visibly wrong render (winding inverted, colours
  swapped, a shape rendered inside-out) — screenshots caught what unit tests
  structurally can't.
- Don't trust a claim about official Roblox behaviour from memory or a
  general web search — Roblox's own docs are fetchable directly:
  `gh api repos/Roblox/creator-docs/contents/content/en-us/reference/engine/classes/<Class>.yaml --jq .content | base64 -d`
  (the `gh` CLI needs to be authenticated in your environment). Where the
  docs don't cover something (Roblox doesn't publish exact tonemap curves,
  BRDF constants, or the CSG mesh format, for instance), say so plainly in
  code comments rather than presenting a guess as verified fact.

## Assets and licensing

This project is careful not to ship or embed anything proprietary to
Roblox:

- No Roblox place files, meshes, textures, icon sheets, or other extracted
  client assets are committed to this repository. Tests that need a real
  file either download one anonymously from Roblox's public CDN at test
  time, or read a path from an `RBX_..._FIXTURE`-style environment variable
  and are marked `#[ignore]` so a plain `cargo test` skips them cleanly —
  grep the codebase for `RBX_.*_FIXTURE` for the existing pattern before
  inventing a new one.
- The renderer's lighting/material model was derived by reading Roblox's own
  decompiled shader sources (credited in the root `README.md`) and public
  documentation — that's a deliberate, considered choice, not an oversight;
  don't casually add a second such derivation without the same care (reading
  the actual primary source, documenting what's verified vs. approximated).
- Roblox's proprietary CSG mesh format (`MeshData`/CSGMDL) is intentionally
  not decoded — see `ROADMAP.md` for the reasoning. Don't attempt to add a
  decoder for it as a drive-by fix.
- If you're not sure whether something you're about to add is safe to ship,
  ask rather than guess.

## Token and context economy

- Don't read a whole file to change a few lines. Locate what you need with
  `grep -n`/`rg`, then read only that range. Don't re-read a file
  immediately after editing it — you already know what you wrote.
- Make the smallest diff that fixes the thing. Don't rewrite a file to
  change a handful of lines, and don't refactor unrelated code while you're
  in the area.
- Pipe build/test output through `tail`/`grep` rather than dumping it in
  full (`./scripts/check.sh 2>&1 | tail -10` is usually enough to see
  whether it passed).
- If you're working under a token or rate-limit budget on a long,
  multi-step task (many file edits, many test/build cycles), it's fine —
  often preferable — to pause between steps rather than racing through
  everything in one uninterrupted burst. A deliberate pause between
  meaningful chunks of work costs wall-clock time, not correctness or
  quality; racing through a long task to finish faster is not itself a
  goal here.

## Working alongside other agents or contributors

If you're one of several agents (or people) working in this repository at
the same time, stay inside the files your task actually needs. A shared,
central file (a top-level `mod.rs`, a widely-used struct's definition) is
the most likely place for two independent changes to collide — if your
change to such a file is small and mechanical, that's usually fine; if it's
not, say so and coordinate rather than guessing you're the only one editing
it.

## Orchestration and subagents

If your own tooling supports spawning subagents or running multiple agent
instances (Claude Code's subagent/fork tools, Codex's or Gemini's
equivalents, or anything similar), use that deliberately rather than
doing an entire multi-part task as one long, unbroken run — patterns that
have worked well building parts of this project:

- Split a multi-part task into scoped, disjoint pieces of work — one
  subagent per bug, per feature, per independent investigation — instead
  of one agent's context accumulating everything at once. A subagent that
  only has to hold one problem in its head tends to do a more careful job
  than one juggling five.
- Independently verify a claim before trusting it, especially for
  anything hard to check mechanically: a rendering/visual fix, a
  performance claim, a concurrency/threading fix, or "I couldn't
  reproduce this." A second, fresh agent re-checking the actual evidence
  (a screenshot, a benchmark, a real repro) — ideally without the first
  agent's own reasoning in its context, so it isn't anchored on the same
  assumption — catches mistakes a single agent's self-review won't; more
  than one "fixed" claim in this project's own history turned out to need
  a second pass like this before it actually was.
- Prefer a more capable (and more expensive) model for genuinely hard,
  open-ended work — a bug with no clear lead, a cross-cutting
  architecture question, an adversarial review pass — and a cheaper/
  faster one for well-scoped, clearly-specified work. Don't spend the
  expensive model on either end of that spectrum by default.
- Run genuinely independent pieces of work concurrently (disjoint file
  scope) to save wall-clock time, but never let two agents write to the
  same file at the same time — `ROADMAP.md` in particular is a single
  shared file no more than one agent should be editing at once, and a
  shared source file two features both touch is exactly the collision
  this document's "Working alongside other agents or contributors"
  section above already warns about.
- This is a tool, not an obligation. Spinning up several subagents to fix
  a one-line typo is waste, not thoroughness — match the orchestration to
  the size and risk of the actual task, per "Token and context economy"
  above.

## Committing and proposing changes

Never push directly to a protected branch (typically `main`) or merge your
own change without review — open a pull request and let a human look at it,
even if you're confident it's correct. See
[`CONTRIBUTING.md`](../CONTRIBUTING.md) at the repo root for the actual
workflow (branch naming, commit message conventions, what the gate expects
before you open a PR).

## Report

When you finish a task, say plainly: what changed (files, not prose
summaries of intent), what you verified and how (test output, a screenshot
path if visual), what you could not verify, and anything you deliberately
left undone and why. Don't claim something is fixed based on code reading
alone if it was practical to actually build and check it.
