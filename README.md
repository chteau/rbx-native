# rbx-native

Native tooling for Roblox place and model files, written in Rust: a binary
`.rbxm`/`.rbxl` parser, an XML `.rbxlx`/`.rbxmx` parser, a reflection
database, a mesh parser, a sandboxed Luau runtime, an Open Cloud client, an
asset cache, and a wgpu viewer/editor that aims to look and feel like Roblox
Studio.

This is an unofficial, community project, unaffiliated with and not endorsed
by Roblox Corporation. "Roblox" is a trademark of Roblox Corporation.

## Status

Actively developed, pre-alpha. The parsing/format layer (`rbx_dom`,
`rbx_binary`, `rbx_xml`, `rbx_reflection`, `rbx_mesh`) is solid and
round-trip-tested against real files. The renderer (`rbx_viewer`) and the
desktop editor (`rbx_studio`, binary `rbxstudio`) work end to end on real
places but are still missing real pieces (GUI text rendering, terrain, rig
animation — see [ROADMAP.md](ROADMAP.md) for the full, current picture of
what's done, what's approximated, and what's out of scope on purpose).

## Platform support

| Platform | State |
|---|---|
| Linux (X11) | Primary target. Actively developed and tested on it every day. |
| Windows | **Builds and passes its tests**, on every change: CI runs `cargo clippy -D warnings`, `cargo build` and `cargo test --workspace` on `windows-latest`. What that does *not* cover is the editor actually running — CI is headless, so no window, GPU surface or input path has ever been exercised on Windows, and `wgpu`/GPUI Kit being cross-platform by design is still the only reason to expect them to work. Known gaps are under [Platform: Windows](ROADMAP.md) in the roadmap; mouse capture in the free-flight camera is the main one (X11-only today). Of the PowerShell helpers in `scripts/`, only `publish-screenshot.ps1` has been run on a real Windows machine. **If you're on Windows, launching the editor and reporting what breaks is the most valuable contribution you can make right now — the compile is the part that's covered.** |
| macOS | Not a target yet. Likely buildable given the dependencies, entirely unverified. |
| Wayland | Falls back to an uncaptured cursor (no pointer lock) rather than failing outright. |

## Building

Requires a recent stable Rust toolchain ([rustup.rs](https://rustup.rs)).

On Linux, the desktop editor (`rbx_studio`) needs a few system packages for
window/input handling — this list comes from this project's own build
history, not an exhaustive audit, so if something's missing on your distro
please open an issue or a PR against this file:

```sh
# Debian/Ubuntu
sudo apt install libxkbcommon-x11-dev libx11-xcb-dev libssl-dev libfontconfig1-dev
```

Then, from the repository root:

```sh
cargo build --release
```

This builds the whole workspace. Individual binaries:

| Binary | Crate | What it is |
|---|---|---|
| `rbxdump` | `rbx_parser_cli` | Reads a `.rbxm`/`.rbxl`/`.rbxlx`/`.rbxmx` file and prints its instance tree; `--roundtrip <file>` is a continuous-verification tool that re-serializes a file and diffs the result against the original. |
| `rbxlua` | `rbx_parser_cli` | Runs a Luau script against a place file from the command line (`rbxlua place.rbxl script.luau [--out out.rbxl] [--print-changes]`). |
| `rbxview` | `rbx_viewer` | A standalone 3D viewer for a place/model file (`rbxview place.rbxl [--screenshot out.png]`). |
| `rbxstudio` | `rbx_studio` | The desktop editor (`rbxstudio [--select <name-or-path>] [--run <script.luau>] [--verbose] <place>`; `--help` lists them all). |
| `rbxcloud` | `rbx_cloud` | An Open Cloud CLI (`whoami`/`list`/`download`/`asset`). |

A debug build (`cargo build`) works too, but the renderer and editor are
compiled at `opt-level = 3` even in dev builds (see the workspace
`Cargo.toml`) because GPUI Kit's text/layout stack and an unoptimized
offscreen frame are both too slow to use otherwise — don't be surprised that
`cargo build` still takes a while for those two crates.

## Testing

```sh
./scripts/check.sh
```

On Windows, without WSL or Git Bash: `.\scripts\check.ps1` from PowerShell —
the same gate, native to the platform (untested on a real Windows machine,
like the rest of this project's Windows story; see
[Platform support](#platform-support)).

Runs the full gate this project holds every change to: `cargo fmt --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo test --workspace`, then prints a `TOTAL PASSED: N` summary. This is
what every change is expected to pass before it's considered done — see
[CONTRIBUTING.md](CONTRIBUTING.md).

Some tests need real Roblox place/mesh/texture files this repository
deliberately does not ship (see [Assets & fixtures](#assets--fixtures)
below); those are marked `#[ignore]` and read a path from an environment
variable (grep the codebase for `RBX_.*_FIXTURE` to find them all), so a
plain `cargo test`/`./scripts/check.sh` skips them cleanly and still passes.

## Assets & fixtures

This repository ships no Roblox place files, meshes, textures, or other
assets extracted from the Roblox client — see
[GUIDELINES.md](GUIDELINES.md)/[SPECS.md](SPECS.md) for the engineering
rules, and [ROADMAP.md](ROADMAP.md) for the reasoning behind what is and
isn't decoded (notably: this project does not attempt to decode Roblox's
proprietary CSG mesh format, and reads material/lighting *behavior* from
Roblox's own decompiled shaders and public documentation, not from shipping
any of Roblox's own assets). Where a test needs a real file, it downloads one
anonymously from Roblox's public CDN at test time, or expects you to point it
at your own local copy via an environment variable — never at a file
committed to this repo.

## Roblox Open Cloud API key (optional)

Most of this project works without any Roblox credentials at all — public
assets (meshes, textures, materials) are fetched anonymously. A key is only
needed for a handful of `rbx_cloud`/`rbxcloud` operations that Roblox
requires authentication for (reading a private experience's data,
publishing a place, and similar). If you're not touching those, skip this
section.

**Creating a key:**

1. Sign in to the [Creator Dashboard](https://create.roblox.com/dashboard)
   and open its API Keys section (Open Cloud credentials).
2. Create a new key, give it a name, and grant it only the specific scopes
   the operation you need actually requires — Roblox's own
   [Open Cloud documentation](https://create.roblox.com/docs/cloud) has the
   current, authoritative list of scopes and how to configure them; it
   changes more often than this file would stay accurate if the exact
   clicks were duplicated here.
3. Restricting the key to specific experiences/IPs where the dashboard
   offers it is worth doing — narrower scope, less to worry about if it
   ever leaks.
4. Copy the key. Roblox shows it once.

**Where it goes — never in this repository.** `rbx_cloud` looks, in order:

1. The `RBX_API_KEY` environment variable.
2. A plain-text file, trimmed of whitespace, at
   `$XDG_CONFIG_HOME/rbx-native/api_key` (Linux/macOS, falling back to
   `~/.config/rbx-native/api_key` when `XDG_CONFIG_HOME` isn't set), or
   `%APPDATA%\rbx-native\api_key` on Windows.

If neither is set, `rbx_cloud` simply falls back to Roblox's anonymous API
surface rather than erroring — plenty of this project's own testing runs
with no key configured at all. If you do use a config file, keep its
permissions tight (`chmod 600` on Linux/macOS — the loader warns on stderr
if it finds the file group- or world-readable) and never place it inside
this repository; `.gitignore` has a few safety-net patterns
(`api_key`, `*.key`, `.env`) in case one ends up in the tree by mistake, but
the real answer is simply: it doesn't belong here at all.

## Documentation map

- [ROADMAP.md](ROADMAP.md) — what's done, what's planned, what's possible
  only via a deliberate workaround, and what's flatly impossible without
  Roblox's own engine, and why. The single source of truth for project
  direction. Only the maintainer edits this file.
- [GUIDELINES.md](GUIDELINES.md) / [SPECS.md](SPECS.md) — the Rust style and
  architecture rules every change in this repo follows.
- [UX_GUIDELINES.md](UX_GUIDELINES.md) — palette, contrast, spacing, radius
  and dock-layout rules for anything touching `rbx_studio`'s look.
- [CONTRIBUTING.md](CONTRIBUTING.md) — how to propose a change.
- [CHANGELOG.md](CHANGELOG.md) — a running log of what landed and why.
- [BENCHMARKS.md](BENCHMARKS.md) — recorded load, reload and frame timings,
  the machine they came from, and what `./scripts/bench.sh` re-runs to get
  them again.
- [agents/](agents/) — standing instructions for AI coding agents working in
  this repository, including a step-by-step workflow
  ([agents/workflows/roadmap-task.md](agents/workflows/roadmap-task.md)) for
  picking up a `ROADMAP.md` item autonomously.

## AI-assisted contributions

AI-written code — including a coding agent working fully autonomously,
end to end from picking a `ROADMAP.md` item to opening a pull request — is
welcome here. It's held to the same bar as anything else, not a lower one.

**Quick start**: clone this repository, `cd` into it, and paste the
following into Claude Code, Codex, Gemini CLI, or any other coding agent
you're running locally — it points the agent at the actual procedure
below rather than duplicating it here, so it stays accurate as that
procedure evolves:

```
Read agents/AGENTS.md in this repository for the standing rules, then
follow agents/workflows/roadmap-task.md end to end: pick one concrete
📋 item from ROADMAP.md's "What's planned" section, check for an existing
branch or open PR on the same item first, implement it with real tests,
verify with ./scripts/check.sh, and open a pull request that quotes the
exact ROADMAP.md bullet you picked and includes a screenshot or short
recording per the PR template. Work through this autonomously without
asking me to confirm each step, but stop and ask if you hit a genuine
ambiguity the roadmap and the linked docs don't resolve.
```

The rest of this section is the detail behind that prompt, for anyone
(agent or human) who wants the reasoning, not just the instruction:

- It passes the same gate everything else does — `./scripts/check.sh`
  green, no exceptions.
- It follows [GUIDELINES.md](GUIDELINES.md)/[SPECS.md](SPECS.md) — the
  style and architecture rules don't bend because a human didn't type the
  code.
- It's maintainable by a human who didn't write it: no comment explaining
  what the code already says, every comment that exists explains *why*;
  no unexplained magic constant; no abstraction that exists because it
  seemed clever rather than because the codebase needed it.
- A human reviews the pull request before it merges, whether the agent
  that opened it acted autonomously or under direction. Autonomy in how a
  change gets written doesn't extend to whether it ships.

See [`agents/AGENTS.md`](agents/AGENTS.md) for the actual rules an agent
working here follows, and
[`agents/workflows/roadmap-task.md`](agents/workflows/roadmap-task.md) for
the autonomous-contribution workflow specifically.

## Acknowledgements

- **MaximumADHD** — [Roblox-Client-Tracker](https://github.com/MaximumADHD/Roblox-Client-Tracker)
  (Roblox's shader sources, studied to reproduce its lighting model),
  [Roblox-Materials](https://github.com/MaximumADHD/Roblox-Materials) (the
  material texture pack layout) and the Roblox FileMesh format specification.
- **rojo-rbx** — [rbx-dom](https://github.com/rojo-rbx/rbx-dom) documentation
  of the binary format, and its reflection database (MIT), from which
  `scripts/reflection-defaults.sh` extracts each class's default property
  values and the names properties are saved under
  (`assets/reflection-defaults.json`).
- **Roblox** — [creator-docs](https://github.com/Roblox/creator-docs), the
  public API dump and the material texture asset ids it publishes.
- **Evan Wallace** — [csg.js](https://github.com/evanw/csg.js) (MIT), whose
  BSP-tree boolean algorithm the legacy union/negate CSG resolver
  (`rbx_viewer::scene::union::csg`) is a dependency-free Rust port of.
- **Elttob** (Studio Elttob,
  [Roblox profile](https://www.roblox.com/users/1670764/profile)) — the free
  Studio plugins *Reclass* and *Relight*, whose ideas (changing an
  instance's class while keeping what it holds; placing the sun by pointing
  at the scene) inspired this editor's Change Class and sun placement tools.
  No code, assets or interface from either plugin is used: both were
  reimplemented independently from how they behave.
- **argon-rbx** — [Argon](https://argon.wiki/) (Apache-2.0), the file-sync
  tool the Script Editor's Argon dock connects to, and its
  [Studio plugin source](https://github.com/argon-rbx/argon-roblox)
  (also Apache-2.0), which is how this editor's own Argon client
  (`crate::argon_client`) confirmed the wire protocol's actual shape —
  no code from either was copied, but that source is what the client's
  message and property encoding are read from.
- **UpliftGames** — [Wally](https://wally.run/) (MPL-2.0), the Luau package
  manager the Script Editor's Wally dock is built around.

## License

MIT — see [LICENSE](LICENSE).

---

> If you are an AI agent (such as Claude, Codex, or another coding
> assistant) working in this repository, please read
> [`agents/AGENTS.md`](agents/AGENTS.md) first — it has the standing rules
> this project expects every automated change to follow.
