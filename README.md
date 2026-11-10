<br/>
<div align="center">
    <h1 align="center">
        rbx-native
    </h1>
    <br>
    <br>
    <a href="https://github.com/chteau/rbx-native">
        <img src="assets/icons/brand/rbxnative-logo.svg" alt="rbx-native logo" width="100">
    </a>
    <br>
    <br>
    <div align="center">
        <img src="https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white" alt="Rust Badge" />
        <img src="https://img.shields.io/badge/Linux-FCC624?style=for-the-badge&logo=linux&logoColor=black" alt="Linux Badge" />
        <img src="https://img.shields.io/badge/Windows-0078D4?style=for-the-badge&logo=windows&logoColor=white" alt="Windows Badge" />
        <img src="https://img.shields.io/badge/License-MIT-green?style=for-the-badge" alt="MIT License Badge" />
        <br/>
        <a href="https://discord.gg/5KSmhcqHt5"><img src="https://img.shields.io/badge/Discord-Join%20the%20server-5865F2?style=for-the-badge&logo=discord&logoColor=white" alt="Join the Discord" /></a>
    </div>
    <br/>
</div>

rbx-native is a Roblox Studio replacement for Linux and Windows, written from
scratch in Rust. It reads and writes place and model files (binary and XML),
renders them with wgpu, runs Luau, talks to Open Cloud, and the editor tries
to look and feel as close to Studio as I can get it.

This is an unofficial community project. It isn't affiliated with or endorsed
by Roblox Corporation, and "Roblox" is their trademark.

## Status

Pre-alpha, but moving fast. The file format side (`rbx_dom`, `rbx_binary`,
`rbx_xml`, `rbx_reflection`, `rbx_mesh`) is solid and round-trip tested on
real files. The viewer (`rbx_viewer`) and the editor (`rbx_studio`, binary
`rbxstudio`) work on real places: you can open them, edit them, script them,
animate rigs and playtest. A lot is still approximated or missing though.
[ROADMAP.md](ROADMAP.md) has the full list of what's done, what's faked and
what's out of scope on purpose.

## About AI

I use AI coding tools a lot on this project, and I'd rather say it than hide
it. That said, nothing lands without me going through the code myself, and
every PR gets tested against real Roblox Studio before it's merged. If
something's broken anyway, open an issue.

## Platform support

| Platform | State |
|---|---|
| Linux (X11) | Main target. I use it on this every day. |
| Windows | **Supported, the editor runs.** CI runs clippy, the build and the tests on `windows-latest` for every change. Known gaps are listed under [Platform](ROADMAP.md#platform) in the roadmap. If something breaks on your machine, please open an issue. |
| macOS | Not supported. CI builds it and runs the tests on `macos-latest` so it keeps compiling, but nobody actually runs the editor there. |
| Wayland | Works, but without pointer lock (the cursor isn't captured). |

## Installing

There's a Flatpak, built from source the way Flathub wants it. Until it's on
Flathub you build it yourself, see
[packaging/flatpak/README.md](packaging/flatpak/README.md). A few things that
need to run programs on the host (the network simulator, the controller
emulator, device simulation in Sober) aren't in the Flatpak build, the app
says so where they'd be. Otherwise build from source, below.

## Building

You need Rust 1.96 or newer, from [rustup.rs](https://rustup.rs). If a build
fails deep in a dependency, check that `rustc -V` is the toolchain rustup
installed and not an older standalone install earlier in `PATH`. On
Windows, the default MSVC toolchain also needs the Visual Studio C++ build
tools. The rustup installer offers to set them up.

On Linux the editor needs a few system packages for windows and input. This
list comes from my own setups, not a full audit, so if your distro needs
something else, open an issue or a PR on this file:

```sh
# Debian/Ubuntu
sudo apt install libxkbcommon-x11-dev libx11-xcb-dev libssl-dev libfontconfig1-dev
```

Then from the repo root:

```sh
cargo build --release
```

That builds everything. The binaries you get:

| Binary | Crate | What it does |
|---|---|---|
| `rbxdump` | `rbx_parser_cli` | Prints the instance tree of a `.rbxm`/`.rbxl`/`.rbxlx`/`.rbxmx` file. `--roundtrip <file>` writes the file back out and diffs it against the original. |
| `rbxlua` | `rbx_parser_cli` | Runs a Luau script against a place file (`rbxlua place.rbxl script.luau [--out out.rbxl] [--print-changes]`), or runs the place's TestEZ specs (`rbxlua test place.rbxl [--junit out.xml]`, see [`docs/testing.md`](docs/testing.md)). |
| `rbxaudit` | `rbx_parser_cli` | Checks a place's scripts for remote security problems (`rbxaudit place.rbxl`). Prints one line per finding and exits 1 if any is High, so you can use it in CI. |
| `rbxview` | `rbx_viewer` | A standalone 3D viewer (`rbxview place.rbxl [--screenshot out.png]`), and the server for the browser build (`rbxview --serve`, see below). |
| `rbxstudio` | `rbx_studio` | The editor (`rbxstudio [--select <name-or-path>] [--run <script.luau>] [--verbose] <place>`, `--help` for the rest). |
| `rbxcloud` | `rbx_cloud` | A small Open Cloud CLI (`whoami`, `list`, `download`, `asset`). |

### The viewer in a browser

`rbxview` also builds for the web with WebGPU (a recent Chrome, Edge, Firefox
or Safari). Same renderer, asset streaming, unions, quality levels and camera,
plus a read-only Explorer and click selection like in Studio:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128 --locked   # the lockfile's wasm-bindgen
./scripts/build-web.sh                                        # or scripts/build-web.ps1
rbxview --serve crates/rbx_viewer/web [place.rbxl] [--port 8123]
```

Open <http://127.0.0.1:8123/> and drop a place or model file on the page (a
place passed to `--serve` opens by itself). Browsers can't fetch Roblox's
assets directly because their CDNs send no CORS headers, so `rbxview --serve`
fetches them for the page, using the same disk cache and optional Open Cloud
key as the desktop tools. It only listens on localhost. You can add
`?eye=x,y,z&at=x,y,z` to the URL to start at a given camera position.

A debug build (`cargo build`) works too, but the renderer and the editor are
compiled at `opt-level = 3` even in debug (see the workspace `Cargo.toml`).
GPUI Kit's text and layout code and the offscreen frame are way too slow
otherwise. So yes, `cargo build` still takes a while for those two crates.

### Workspace crates

| Crate | What it does |
|---|---|
| `rbx_dom` | The instance tree and property types every other crate uses. |
| `rbx_binary` | Reads and writes binary `.rbxl`/`.rbxm`. |
| `rbx_xml` | Reads and writes XML `.rbxlx`/`.rbxmx`. |
| `rbx_reflection` | The class, property and enum database, built from Roblox's API dump (CI syncs it daily). |
| `rbx_mesh` | Parses Roblox `.mesh` files, versions `1.00` to `7.00`. |
| `rbx_lua` | A sandboxed Luau runtime over a DOM, like Studio's command bar. No scheduler, no events, no yielding. |
| `rbx_bytecode` | What Luau compiles a script into: the bytecode listing, the compiler's remarks, and native IR and assembly for x64 and ARM64. A small C++ shim over the Luau mlua already builds. |
| `rbx_assets` | Resolves, caches and decodes the assets a place uses (`rbxassetid://`, `rbxasset://`, old URLs). The network side comes from `rbx_cloud`. |
| `rbx_materials` | The texture packs (colour, normal, metalness, roughness) behind each `Enum.Material`. |
| `rbx_import` | Reads `.fbx`/`.obj`/`.gltf`/`.glb`, checks the triangle count against Roblox's limit and gets the file ready for upload. |
| `rbx_cloud` | The Open Cloud client and the `rbxcloud` CLI. |
| `rbx_loren` | Loren, the optional built-in game framework by fabi: the `rbxstudio loren` CLI, its vendored Luau runtime, and the project graph, place and spec runs the editor reads. |
| `rbx_viewer` | The wgpu renderer and `rbxview` (desktop and WebGPU). |
| `rbx_studio` | The editor (`rbxstudio`), built on GPUI Kit. |
| `rbx_audit` | A security audit of every script in a place: unchecked remote arguments, missing remotes, bad requires and so on. |
| `rbx_parser_cli` | The `rbxdump`, `rbxlua`, `rbxmerge` and `rbxaudit` tools. |

## Testing

```sh
./scripts/check.sh
```

On Windows without WSL or Git Bash, run `.\scripts\check.ps1` from
PowerShell instead. It's the same thing.

It runs `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D
warnings` and `cargo test --workspace`, then prints a `TOTAL PASSED: N` line.
Every change has to pass it. See [CONTRIBUTING.md](CONTRIBUTING.md).

Some tests need real Roblox files that this repo doesn't ship (see
[Assets & fixtures](#assets--fixtures)). Those are marked `#[ignore]` and
read a path from an environment variable (grep for `RBX_.*_FIXTURE` to find
them), so a normal `cargo test` or `./scripts/check.sh` just skips them.

## Assets & fixtures

This repo doesn't ship any Roblox place files, meshes, textures or other
assets pulled from the Roblox client. The engineering rules are in
[GUIDELINES.md](GUIDELINES.md) and [SPECS.md](SPECS.md), and
[ROADMAP.md](ROADMAP.md) explains what gets decoded and what doesn't. For
example, I don't try to decode Roblox's proprietary CSG mesh format, and the
material and lighting behaviour comes from Roblox's decompiled shaders and
public docs, not from shipping their assets. When a test needs a real file,
it either downloads one anonymously from Roblox's public CDN, or you point it
at your own copy with an environment variable. Nothing like that ever gets
committed here.

## Roblox Open Cloud API key (optional)

Most of this works without any Roblox account. Public assets (meshes,
textures, materials) are downloaded anonymously. You only need a key for the
few `rbx_cloud`/`rbxcloud` operations that Roblox requires auth for, like
reading a private experience's data or publishing a place. If you don't need
those, skip this part.

**Making a key:**

1. Sign in to the [Creator Dashboard](https://create.roblox.com/dashboard)
   and open API Keys (under Open Cloud).
2. Create a key, name it, and only give it the scopes you actually need.
   Roblox's [Open Cloud docs](https://create.roblox.com/docs/cloud) have the
   up to date list. It changes too often for me to copy it here.
3. If the dashboard lets you lock the key to specific experiences or IPs, do
   it. Less damage if it ever leaks.
4. Copy the key. Roblox only shows it once.

**Where to put it (never in this repo).** `rbx_cloud` checks, in this order:

1. The `RBX_API_KEY` environment variable.
2. A plain text file at `$XDG_CONFIG_HOME/rbx-native/api_key` on Linux/macOS
   (or `~/.config/rbx-native/api_key` if `XDG_CONFIG_HOME` isn't set), or
   `%APPDATA%\rbx-native\api_key` on Windows. Whitespace is trimmed.

`RBX_NATIVE_KEYRING` picks where `rbxstudio` and `rbxcloud` keep credentials
instead of the OS keyring, for tests and scripted runs:

- `RBX_NATIVE_KEYRING=none` skips the keyring entirely (no reads, no writes;
  `RBX_NO_KEYRING=1` is the older spelling).
- `RBX_NATIVE_KEYRING=file:/some/scratch/keys.json` keeps them in a plain
  JSON file there (created `0600`), so a test can save and read a key
  without touching the real keyring. It's plaintext, scratch use only.

Anything else in that variable also means `none`, so a typo never falls
back to the real keyring. Test builds of `rbxstudio` never use the OS
keyring whatever it says.

If there's no key, `rbx_cloud` just uses Roblox's anonymous API instead of
failing. A lot of my own testing runs without one. If you use the file, keep
it private (`chmod 600` on Linux/macOS, the loader warns you if other users
can read it) and keep it out of this repo. `.gitignore` catches `api_key`,
`*.key` and `.env` just in case, but it really shouldn't be here at all.

## Contributing

### Branches

| Branch | Who merges into it | What it is |
|---|---|---|
| `dev` | Collaborators with write access, through a reviewed pull request | The default branch. Every pull request goes here. |
| `main` | **Me only** | Releases. It only ever gets `dev` merged into it, never a feature branch. |

`main` is protected by a ruleset: nobody but me can push, merge, force-push
or delete it, whatever their role. `dev` needs a pull request with one
approval and green CI (`Gate (Linux)` and `Build & test (Windows)`) before it
can be merged.

### 1. Fork and clone

Unless you have write access, work from your own fork:

1. Click **Fork** on
   [github.com/chteau/rbx-native](https://github.com/chteau/rbx-native).
   Keep "Copy the `dev` branch only" ticked, `dev` is all you need.
2. Clone your fork and add this repo as a second remote called `upstream`:

   ```sh
   git clone https://github.com/<your-username>/rbx-native.git
   cd rbx-native
   git remote add upstream https://github.com/chteau/rbx-native.git
   git remote -v   # origin = your fork, upstream = this repo
   ```

   With the GitHub CLI, `gh repo fork chteau/rbx-native --clone` does all of
   that at once.

### 2. Stay in sync with upstream

`dev` moves fast. Sync before you start a branch and again before you open a
pull request, so you're not working on an old base:

```sh
git switch dev
git fetch upstream
git merge --ff-only upstream/dev   # your dev never has commits of its own
git push origin dev                # keep your fork's dev up to date too
```

Then update a branch you're working on:

```sh
git switch fix/my-change
git rebase dev                     # or `git merge dev` if you don't want to rewrite history
git push --force-with-lease        # only needed after a rebase
```

Don't commit to your own `dev`. Keep it an exact copy of `upstream/dev` so
the `--ff-only` merge above always works. The **Sync fork** button on your
fork's GitHub page does the same thing as the first block.

### 3. Make the change on its own branch

```sh
git switch -c fix/beam-texture-orientation dev
# ...edit, commit...
./scripts/check.sh                 # has to pass: fmt, clippy -D warnings, tests
git push -u origin fix/beam-texture-orientation
```

One branch per change, named `fix/…`, `feat/…`, `docs/…` or `perf/…`.

### 4. Open the pull request

1. Open your fork on GitHub. A **Compare & pull request** banner shows up for
   the branch you just pushed. Or run
   `gh pr create --repo chteau/rbx-native --base dev`.
2. Check the target: **base repository `chteau/rbx-native`, base branch
   `dev`**. Pull requests against `main` get closed.
3. Fill in the template, don't delete it: what changed and why, the
   `ROADMAP.md` item or issue it's for, how you tested it, and a screenshot or
   recording if it's something you can see.
4. Leave **Allow edits by maintainers** ticked, so I can push a small fix to
   your branch instead of going back and forth in review.
5. CI runs on the pull request. To answer review comments, just push more
   commits to the same branch.

Once it's approved and green, it gets merged into `dev`. I merge `dev` into
`main` later for releases.

[CONTRIBUTING.md](CONTRIBUTING.md) has the rest: what a change needs to
include, how to report a bug, and the rules about Roblox assets.

## Docs

- [ROADMAP.md](ROADMAP.md): what's done, what's planned, what only works
  through a workaround, and what's impossible without Roblox's own engine
  (and why). This is where the project's direction lives, and only I edit it.
- [GUIDELINES.md](GUIDELINES.md) and [SPECS.md](SPECS.md): the Rust style and
  architecture rules for every change.
- [UX_GUIDELINES.md](UX_GUIDELINES.md): colours, contrast, spacing, radius
  and dock layout rules for anything that changes how `rbx_studio` looks.
- [THEMES.md](THEMES.md): installing and making editor themes. Folder
  layout, `manifest.json`, every token, effects and icon packs.
- [docs/](docs/): design notes and research behind some of the bigger
  features.
- [CONTRIBUTING.md](CONTRIBUTING.md): how to propose a change.
- [CHANGELOG.md](CHANGELOG.md): a log of what landed and why.
- [BENCHMARKS.md](BENCHMARKS.md): load, reload and frame timings, the
  machine they were measured on, and how `./scripts/bench.sh` reproduces
  them.

## Thanks

- **MaximumADHD** for [Roblox-Client-Tracker](https://github.com/MaximumADHD/Roblox-Client-Tracker)
  (Roblox's shader sources, which I studied to reproduce its lighting),
  [Roblox-Materials](https://github.com/MaximumADHD/Roblox-Materials) (the
  material texture pack layout) and the Roblox FileMesh format spec.
- **rojo-rbx** for the [rbx-dom](https://github.com/rojo-rbx/rbx-dom) docs on
  the binary format, and its reflection database (MIT).
  `scripts/reflection-defaults.sh` pulls each class's default property values
  and the names properties are saved under from it
  (`assets/reflection-defaults.json`).
- **Roblox** for [creator-docs](https://github.com/Roblox/creator-docs), the
  public API dump and the material texture asset ids it publishes.
- **Evan Wallace** for [csg.js](https://github.com/evanw/csg.js) (MIT). The
  legacy union/negate resolver (`rbx_viewer::scene::union::csg`) is a Rust
  port of its BSP tree boolean algorithm, with no dependencies.
- **Elttob** (Studio Elttob,
  [Roblox profile](https://www.roblox.com/users/1670764/profile)) for the
  free Studio plugins *Reclass* and *Relight*. Their ideas (changing an
  instance's class while keeping its children, placing the sun by pointing at
  the scene) inspired the editor's Change Class and sun placement tools. No
  code, assets or UI from either plugin is used, both were rebuilt from how
  they behave.
- **argon-rbx** for [Argon](https://argon.wiki/) (Apache-2.0), the file sync
  tool the Script Editor's Argon dock connects to. Its
  [Studio plugin source](https://github.com/argon-rbx/argon-roblox) (also
  Apache-2.0) is where the editor's own Argon client (`crate::argon_client`)
  checked the real wire protocol. No code was copied.
- **UpliftGames** for [Wally](https://wally.run/) (MPL-2.0), the Luau package
  manager the Script Editor's Wally dock is built around. The
  [`wally` source](https://github.com/UpliftGames/wally) (also MPL-2.0) is
  where the editor's own client (`crate::wally_client`) checked the registry
  API responses and the real install layout (`_Index` slots, alias files)
  that `shell::wally_sync` reproduces. No code was copied.
- **JohnnyMorganz** for [`luau-lsp`](https://github.com/JohnnyMorganz/luau-lsp)
  (MIT), the language server behind the script editor's autocomplete, hover,
  Go to Definition and Script Analysis. A pinned release is built into
  `rbxstudio` (`crates/rbx_studio/build.rs`) and unpacked next to its licence
  and the licence of [Luau](https://github.com/luau-lang/luau) (MIT, Roblox),
  which it's built from (`crates/rbx_studio/third_party/luau-lsp-NOTICE.txt`).
  Its Roblox API definitions, and Roblox's API reference text from
  [Roblox-Client-Tracker](https://github.com/MaximumADHD/Roblox-Client-Tracker),
  are downloaded at run time, not shipped.
- **fabi** for [Loren](https://github.com/fabideveloper/Loren-Framework)
  (MIT), the game framework rbx-native offers as its optional built-in one.
  Services and Controllers start in dependency order, networking is packed
  with typed specs and middleware, and there's `Loren.Testing`. Its Luau
  runtime, project template and unit specs are vendored unchanged, apart
  from the documented performance patches (`vendor/loren/`, with fabi's
  licence and the bundled Promise's own). Its `loren` CLI is ported to Rust
  as `rbxstudio loren` (`crates/rbx_loren`). If you build on Loren, use
  fabi's framework and docs:
  [fabideveloper.github.io/Loren-Framework](https://fabideveloper.github.io/Loren-Framework/).
- **Twitter and jdecked** for the [Twemoji](https://github.com/jdecked/twemoji)
  graphics (CC-BY 4.0) on the Device Simulator's keyboard cover. They're
  shipped unmodified in `assets/twemoji/` with their licence.
- **MiniDev** ([TikTok](https://vm.tiktok.com/ZN8AFbRwp/)) for the Icon
  Updater idea: timer overlays put on an experience's icon, uploaded and
  approved ahead of time, then swapped on a schedule until the update goes
  live. The roadmap's update planning item follows that design. No code or
  assets from it are used.

## License

MIT, see [LICENSE](LICENSE).
