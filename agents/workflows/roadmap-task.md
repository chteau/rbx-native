# Workflow: picking up a roadmap item autonomously

This is the canonical, tool-agnostic procedure for an AI agent (Claude,
Codex, Gemini, or anything else) working unattended on this repository:
pick something from [`ROADMAP.md`](../../ROADMAP.md), implement it, stay in
sync with everyone else doing the same thing, and land it as a reviewable
pull request. [`agents/AGENTS.md`](../AGENTS.md) grants the authorization
this workflow exercises — read that first if you haven't. Every tool-
specific entry point (the Claude Code skill in `.claude/skills/roadmap-task/`,
a root `GEMINI.md`, `AGENTS.md`) points here for the actual procedure rather
than repeating it.

## 0. Orient

**Prerequisites**: this whole workflow assumes a working `git` and the
[GitHub CLI](https://cli.github.com/) (`gh`) are both installed and on
`PATH` — steps 2 and 6 below use `gh pr list`/`gh pr create` directly, not
just `git`. Installation is OS-dependent (see cli.github.com for the
current instructions per platform); confirm both are present
(`git --version`, `gh --version`) before starting if you're not certain,
and authenticate `gh` (`gh auth status`) if it isn't already — the rest of
this workflow assumes both just work.

Before touching anything:

1. Read the repository root [`README.md`](../../README.md) for what this
   project is and how it's built and tested.
2. Read [`GUIDELINES.md`](../../GUIDELINES.md) and
   [`SPECS.md`](../../SPECS.md) — the Rust style and architecture rules
   every change here follows.
3. Read [`agents/AGENTS.md`](../AGENTS.md) for the standing rules (asset
   licensing constraints, verification bar, token economy, what never to
   touch).

## 1. Pick one item

Read [`ROADMAP.md`](../../ROADMAP.md)'s "What's planned" section. Pick one
`📋` item — one coherent, reasonably-scoped piece of work, not several.
Prefer an item whose sub-bullets are already concrete over a vague category
header.

Don't edit `ROADMAP.md` yet to reflect this choice — wait until step 6,
once your PR for it is actually merged, then check it off yourself (see
`agents/AGENTS.md`'s "Picking up work from the roadmap" section for the
exact shape: full item shipped → move it to "What's been implemented";
only part of it shipped → flip it to `[x] 🚧` in place and say what's
still open). Roadmap direction otherwise stays the maintainer's call —
don't touch any bullet but the one you shipped.

## 2. Check for an existing branch or PR before creating one

Someone — another agent, a human, an earlier run of this same workflow —
may already be working the same item. Branch names and commit subjects
alone are an unreliable way to check this (they're often generic, e.g.
`feat/gizmos`, and don't say *which* roadmap bullet); open pull requests
are far more discoverable, since step 6 below requires every PR to quote
the exact `ROADMAP.md` bullet it addresses. Check both:

```sh
git fetch origin
gh pr list --state open --search '<keyword from the roadmap item>'
git branch -a | grep -i '<keyword from the roadmap item>'
```

- **A matching open PR or active branch exists** (recent commits, a PR
  that isn't stale): check out that branch and continue its work instead
  of starting over — `git checkout -b <branch> origin/<branch>` if you
  don't have it locally yet.
- **A matching branch exists but looks abandoned** (stale, no PR, or its PR
  was closed without merging): say so in your eventual PR description if
  you pick up the same item on a fresh branch instead; don't silently
  duplicate work without acknowledging what came before.
- **Nothing matches**: create a new branch off the current `main`, named
  after the convention in [`CONTRIBUTING.md`](../../CONTRIBUTING.md)
  (`feat/…`, `fix/…`, `docs/…`) — pick a name specific enough to the
  actual roadmap bullet that another agent's keyword search in this same
  step would find it (`feat/properties-cframe-orientation`, not
  `feat/properties`), and say which bullet you picked in your **first**
  commit message on the branch, not only in the eventual PR.

**Push that first commit to `origin` immediately** — before writing any
implementation code. The whole point of step 2 is to make your claim on
this item visible to anyone else about to start the same search; a branch
that only exists on your machine doesn't do that, and the gap between
"I decided to pick this up" and "I got around to pushing" is exactly the
window where two agents (or an agent and a human) starting on the same
item seconds apart both end up believing they're first. `git branch -a`
and `gh pr list` only ever see what's on `origin`, so an unpushed branch
is invisible to step 2's own check — don't let it stay that way for
longer than it takes to push:

```sh
git fetch origin
git checkout main
git pull --ff-only origin main
git checkout -b feat/<short-description>
git commit --allow-empty -m "Claim: <exact ROADMAP.md bullet you picked>"
git push -u origin feat/<short-description>
```

An empty, message-only commit is fine and expected here — its only job is
to give the branch something to push before any real work exists. It gets
folded into the real history by the normal commits that follow in step 3
(don't bother preserving it as its own line in the final PR).

## 3. Implement

Follow `GUIDELINES.md`/`SPECS.md` and the code rules in `agents/AGENTS.md`.
Write real unit tests for what you implement — this project holds every
change to that standard; a passing build is not a finished feature.

If the change touches rendering, lighting, or anything visual, build the
relevant binary and actually look at a screenshot before considering it
done — see `agents/AGENTS.md`'s "Verifying a change" section for why a
green test suite alone isn't proof there.

If your tooling supports it and the item is large or risky enough to
benefit, consider splitting the work across subagents and/or having a
fresh agent independently verify a hard-to-check claim before you trust
it — see `agents/AGENTS.md`'s "Orchestration and subagents" section for
when that's actually worth it versus overkill.

## 4. Stay in sync — commit, fetch, and merge `main` regularly

Working in isolation for a long stretch is how a branch drifts far enough
from `main` that merging it back becomes painful, and how two agents
working the same area end up with an unresolvable conflict instead of a
small one. Don't wait until the work is "done" to deal with this:

- **Commit in small, working increments**, not one giant commit at the
  end. Each commit should build and, ideally, pass the gate on its own —
  it's much easier to review, and much easier to bisect later, than one
  commit that changes forty files.
- **Fetch and merge `main` periodically** — after each meaningful
  increment, or at least every 30–60 minutes of active work:
  ```sh
  git fetch origin
  git merge origin/main
  ```
  Resolve conflicts immediately, don't let them accumulate. Prefer `merge`
  over `rebase` here: this is a shared, potentially long-lived branch other
  agents or the maintainer might already be looking at, and rewriting its
  history with a rebase risks orphaning anyone who already has a copy of
  it. A rebase is fine on a branch you're certain is yours alone and
  haven't yet pushed.
- **Push what you've committed regularly**, not only at the very end —
  a branch that only exists locally can't be checked by CI, can't be seen
  by another agent checking for an existing branch (step 2), and is one
  crashed process away from lost work.
- **Never force-push** over a branch's existing history unless you are
  certain you're the only one who has touched it and you understand
  exactly why you need to — see the standing git-safety rules this
  repository (and Claude Code generally) follows.

## 5. Verify

`./scripts/check.sh` from the repo root must be green (`cargo fmt --check`,
`cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`) before opening a pull request. CI
(`.github/workflows/ci.yml`) re-runs the same gate on Linux and a
build-and-test pass on Windows on every push — treat a red CI run as a real
signal to fix, not noise, especially for whichever platform you can't
personally verify.

## 6. Open a pull request

```sh
git push -u origin <branch-name>
gh pr create --title "…" --body "…"
```

Fill in `.github/PULL_REQUEST_TEMPLATE.md` for real — what changed and why,
how it was verified (test output; a screenshot or short screen recording
for anything with a visible effect, per the template's own checklist), and
what's left undone if anything. **Quote the exact `ROADMAP.md` bullet**
this PR addresses in the "Related" section, verbatim — not a paraphrase —
an exact quote is what makes the `gh pr list --search` step above actually
find this PR later, and it's the easiest way for a reviewer to check your
roadmap edit (below) against the bullet it's supposed to reflect.

**Include the `ROADMAP.md` checkoff in this same PR**, not a separate
follow-up: move the bullet to "What's been implemented" if it shipped in
full, or flip it to `[x] 🚧` in place and say what's still open if only
part of it did (see `agents/AGENTS.md`'s "Picking up work from the
roadmap" section). Bundling it means the roadmap update gets reviewed
alongside the code, never pushed unreviewed on its own — don't push a
`ROADMAP.md` edit directly to `main` outside a PR unless the maintainer
explicitly asks you to for a specific, already-merged item.

**Also add a [`CHANGELOG.md`](../../CHANGELOG.md) entry in this same PR,
crediting your GitHub username** — the identity `gh` is authenticated as
for this PR, not whatever the local `git` author identity says (they can
differ):

```sh
gh api user --jq .login
```

Append a bullet under today's date heading (`## YYYY-MM-DD` — add the
heading if today doesn't have one yet; don't touch any other date's
entries) written in the file's existing voice — a bolded short title, then
what changed and briefly why, the way every existing entry already reads —
ending the bullet with `— @<that username>`.

Never merge your own pull request. Land the work by opening it and letting
a human review it — that's the whole point of the PR step.
