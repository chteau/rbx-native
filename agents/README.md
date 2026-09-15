# `agents/`

This directory holds the standing instructions this project expects an AI
coding agent (Claude, Codex, or anything else you're pointing at this repo)
to follow.

## Why this isn't just a root-level `AGENTS.md`

Several tools (Claude Code, Codex, and others) look for an `AGENTS.md` at
the root of a repository automatically. This project keeps its copy in a
subdirectory instead, and points to it from the bottom of the root
[`README.md`](../README.md), on purpose: it keeps repository-root real
estate for the docs a human reads first (`README.md`, `CONTRIBUTING.md`,
`ROADMAP.md`), and gives the agent-facing rules room to grow into more than
one file without cluttering the root.

If you're configuring a tool that specifically requires an `AGENTS.md` (or
equivalent) at the repository root to be picked up automatically, a stub
there that just points here is a reasonable local workaround — just don't
duplicate the actual rules in two places, since they'll drift.

## What's here

- [`AGENTS.md`](AGENTS.md) — the actual rules: code style, how to verify a
  change, asset/licensing constraints specific to this project, and how to
  work alongside other agents or contributors without stepping on them.

## What's not here

Anything that isn't specific to an automated agent lives at the repo root
instead, where a human contributor would look for it first:
[`CONTRIBUTING.md`](../CONTRIBUTING.md) for the human contribution workflow,
[`GUIDELINES.md`](../GUIDELINES.md)/[`SPECS.md`](../SPECS.md) for the Rust
style and architecture rules (agents and humans follow the same ones — no
separate agent-specific style guide), and [`ROADMAP.md`](../ROADMAP.md) for
project direction.
