## What this does

<!-- What changed and why. The diff already shows what — explain the why. -->

## Related

<!-- Quote the exact ROADMAP.md bullet this addresses (verbatim, not a
     paraphrase — the file itself is never edited to link back, so this is
     what makes the PR findable by a future `gh pr list --search`), an
     issue, or "none". -->

## How I verified it

<!--
`./scripts/check.sh` must be green before this is ready for review.
If this touches rendering, lighting, or anything visual, a passing test
suite alone isn't proof — include a before/after screenshot, ideally next
to a real Roblox Studio screenshot of the same view.
-->

- [ ] `./scripts/check.sh` passes (fmt, clippy `-D warnings`, full test suite)
- [ ] Added/updated tests for the actual change
- [ ] Screenshot(s) or a short screen recording attached below, for any
      change with a visible effect (a new feature, a UI change, a
      rendering fix) — see below if this one genuinely has none

## Screenshots / recording

<!--
Required for anything with a visible effect. A before/after screenshot, a
real Studio reference screenshot next to it if you have one, or a short
screen recording (a GIF or a linked video) for anything an interaction —
a dragged gizmo, a dock being rearranged, a dialog flow — needs motion to
actually show. If this change genuinely has no visible surface (a parser
fix, an internal refactor, a format-only change), say so explicitly here
instead of leaving the section empty.

To attach one: `./scripts/publish-screenshot.sh path/to.png` (or `.ps1` on
Windows) is the supported way — see its header comment for why a plain
`gh gist create` doesn't work directly on images. It publishes to a
secret (unlisted) gist and prints Markdown you can paste straight in
below, so only run it when you actually have permission to share data
outside the repo this way. If you don't have that permission, state the
local file path and describe what it shows instead, and say plainly that
the image still needs attaching by whoever can.
-->

## Anything left undone or worth flagging for review

<!-- A known limitation, an approximation where Roblox doesn't publish the
     real formula, a case you didn't get to test — say so here rather than
     leaving a reviewer to find it. -->
