# Instructions for Gemini

This project's standing instructions for AI coding agents live in
[`agents/AGENTS.md`](agents/AGENTS.md) — [`agents/README.md`](agents/README.md)
explains why they're in a subdirectory rather than at the repository root.
Read it before making any change here.

If you're picking up a task autonomously from [`ROADMAP.md`](ROADMAP.md) —
choosing something to implement, branching, writing it, and opening a pull
request without further direction — follow the full procedure in
[`agents/workflows/roadmap-task.md`](agents/workflows/roadmap-task.md).

The short version: pick one `📋` item from `ROADMAP.md`'s "What's planned",
check whether a branch for it already exists before creating a new one,
implement it per [`GUIDELINES.md`](GUIDELINES.md)/[`SPECS.md`](SPECS.md)
with real unit tests, commit in small increments while periodically
merging `origin/main` back into the working branch to avoid drifting into
conflict, verify with `./scripts/check.sh`, and open a pull request using
`.github/PULL_REQUEST_TEMPLATE.md` — never merge it yourself, and never
edit `ROADMAP.md`.
