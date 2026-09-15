# Contributing to rbx-native

Thanks for considering it. This project is young and moves fast, so the
short version: read [ROADMAP.md](ROADMAP.md) to see what's actually needed,
keep changes small and verified, and ask before doing something big.

## Before you start

- Skim [README.md](README.md) for what this project is and how to build it.
- Read [GUIDELINES.md](GUIDELINES.md) and [SPECS.md](SPECS.md) — the Rust
  style and architecture rules every change here follows. They're short;
  read them once rather than getting review comments about them repeatedly.
- Check [ROADMAP.md](ROADMAP.md) for what's already done, what's in
  progress, and what's deliberately out of scope. It's the single source of
  truth for project direction and is more current than any summary here
  could stay.
- For anything non-trivial (a new feature, a change to the architecture, a
  new dependency), open an issue or a discussion first. Saves everyone a
  round-trip if the direction turns out to be wrong.
- **On Windows?** The project has never been built there — see the
  "Platform support" section of [README.md](README.md). Trying a build and
  reporting exactly what breaks is genuinely one of the most valuable things
  you can contribute right now, even without fixing anything yourself.

## Setting up

```sh
git clone <this repo's URL>
cd rbx-native
cargo build --release
./scripts/check.sh
```

See [README.md](README.md#building) for platform-specific build
dependencies. `./scripts/check.sh` is the gate every change is held to —
`cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D
warnings`, and the full test suite. Run it before opening a PR; CI runs the
same script.

Some tests need real Roblox files this repository intentionally does not
ship (see [README.md](README.md#assets--fixtures)); they're `#[ignore]`d and
read a path from an environment variable, so they're skipped cleanly if you
don't have one — you don't need Roblox assets on hand to contribute.

## Making a change

1. **Every change gets its own branch off `main`** — a feature, a bug fix,
   a docs tweak, all of it. Never commit straight to `main`, even for
   something tiny. A short, descriptive name is enough:
   `fix/beam-texture-orientation`, `feat/surfacegui-text`,
   `docs/windows-build-steps`.
2. Keep the change scoped to one thing. A bug fix doesn't need a
   refactor riding along with it; a new feature doesn't need to touch
   unrelated code on the way. Small, reviewable PRs get merged faster than
   large ones.
3. Match the existing code's style and the conventions in
   [GUIDELINES.md](GUIDELINES.md) — naming, error handling, module layout,
   and especially comments: explain *why* a decision was made, not what the
   code already says it does.
4. Add or update tests for what you changed. If you're touching rendering,
   lighting, or anything visual, a passing test suite alone isn't proof of
   a correct fix — build the relevant binary and actually look at the
   result (a screenshot, a real place file) before calling it done.
5. Run `./scripts/check.sh`. It must be green.
6. Write a commit message that explains *why*, not just what changed — the
   diff already shows what changed.

## Opening a pull request

Opening one fills in a template (`.github/PULL_REQUEST_TEMPLATE.md`) — fill
it in rather than deleting it, it covers exactly what's below.

- Describe what the change does and why, and how you verified it (test
  output, a screenshot, a specific file you checked it against).
- If it touches something visual, include a before/after screenshot where
  practical — it's the fastest way for a reviewer to confirm the change
  actually does what it claims.
- Reference the relevant `ROADMAP.md` line or an open issue if there is one.
- Expect review comments. This is a young project with strong opinions on
  style and architecture (see `GUIDELINES.md`) — they're enforced
  consistently, not personally.

## Reporting a bug

Open an issue — the bug report template walks you through it: what you did,
what you expected, what happened instead, and — if it's a rendering issue —
a screenshot and, ideally, the exact camera/place setup that reproduces it.
"It looks wrong" is much harder to act on than "this part's specular
highlight is missing, here's the file and
the angle."

## A note on assets and legality

This project does not ship, embed, or commit any Roblox-owned asset (place
files, meshes, textures, icon sheets, or anything else extracted from the
Roblox client). If your change would need one, it should either fetch it
anonymously from Roblox's public CDN at runtime/test time, or read it from a
path a contributor supplies themselves via an environment variable — never
from a file added to this repository. If you're not sure whether something
you want to add is safe to ship, ask before opening the PR.

## Using an AI coding agent

If you're using Claude, Codex, or another AI coding assistant to help with
your contribution, point it at [`agents/AGENTS.md`](agents/AGENTS.md) first
— it has the same rules as this file, made explicit for an automated agent,
plus a few things specific to working in this codebase unattended. You're
still responsible for the result: review what it produces before opening a
PR, the same as you would for your own code.
