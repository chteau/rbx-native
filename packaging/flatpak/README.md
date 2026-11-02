# Flatpak

The Flatpak build of rbx-native, app id `io.github.chteau.rbx_native.Editor`.
It's laid out the way Flathub wants it: everything builds from source,
offline, on `org.freedesktop.Platform` 26.08.

The id has five parts on purpose. Flathub checks that an `io.github.*` id
points at a real repo, and it only turns `_` back into `-` in the repo part
when there's a part after it, so `io.github.chteau.rbx_native.Editor` is
what maps to `github.com/chteau/rbx-native`.

| File | What it is |
|---|---|
| `io.github.chteau.rbx_native.Editor.yml` | The manifest |
| `cargo-sources.json` | Every crate in `Cargo.lock`, for the offline cargo build |
| `cloudflared/go-sources.json`, `cloudflared/modules.txt` | cloudflared's Go modules, same idea |
| `io.github.chteau.rbx_native.Editor.metainfo.xml` | The AppStream listing (what Flathub's page shows) |
| `io.github.chteau.rbx_native.Editor.desktop` | The launcher entry |
| `io.github.chteau.rbx_native.Editor.svg` | The app icon, our logo on the dock colour |
| `io.github.chteau.rbx_native.Editor.mime.xml` | `.rbxl` / `.rbxlx` file types, so places open from the file manager |
| `screenshots/` | The listing's screenshots, linked from the metainfo by commit |

## What's in it

Three modules:

- `luau-lsp`, built from its tag with CMake. Script Analysis, autocomplete
  and hover run on it.
- `cloudflared`, built from its tag with Go. Play's game server reaches the
  editor through it.
- `rbxnative`, the editor itself, with `RBX_BUNDLE_PROGRAMS=0` so
  `rbx_studio`'s `build.rs` embeds none of the three programs it normally
  downloads. The editor picks the first two up from `/app/bin` on `PATH`,
  and slirp4netns isn't shipped at all (see below).

## Permissions

Kept small, each one is commented in the manifest:

- `--socket=wayland`, `--socket=fallback-x11`, `--share=ipc`: the window
- `--device=dri`: the viewport renders with wgpu
- `--share=network`: Open Cloud, the asset CDN, Team Create, Figma, Play's
  tunnel, and Argon / luau-lsp on localhost
- `--filesystem=xdg-run/discord-ipc-0` and
  `--filesystem=xdg-run/app/com.discordapp.Discord:create`: Discord Rich
  Presence, for a native or a Flatpak Discord

Everything else goes through portals. Places open and save through the file
chooser, the Open Cloud key goes into the keyring the Secret portal gives the
app (so a key saved by a non-Flatpak rbx-native isn't shared, you paste it
once more), links and Play's `roblox://` launch go through OpenURI, and the
eyedropper uses the Screenshot portal. The app's own settings, plugins and
cache live under `~/.var/app/io.github.chteau.rbx_native.Editor/`.

There's no `--talk-name=org.freedesktop.Flatpak`, on purpose. With it the app
could run anything on the host, which Flathub doesn't hand out to an editor
like this, and the features it would unlock all need more than that anyway
(writing into Sober's own data folder, network namespaces, `/dev/uinput`).
So these say in the app that the Flatpak build can't do them, and point at
the GitHub build:

- Play opens Sober through the portal like any `roblox://` link. It works,
  but the simulated device and the controller emulator can't reach the
  client, and the editor can't tell when Sober closes
- The network simulator (needs `unshare` and slirp4netns on the host)
- The controller emulator (needs `/dev/uinput`)
- Update planning's background timer (needs `systemctl --user`). Steps still
  run while the editor is open
- Argon's CLI badge and Loren's tool runs (the host's `argon` / `rojo`
  aren't visible). `argon serve` in a terminal still connects over localhost
- Plugins and textures from a Vinegar Studio or Sober install, since those
  live in other apps' data folders

Figma sign-in needs a client secret that only the release pipeline has, so
it can't be in a public manifest. The Figma window says sign-in isn't
available in this build.

## Building it locally

You need `flatpak` with the Flathub remote, and flatpak-builder. The Flatpak
one is easiest:

```sh
flatpak install --user flathub org.flatpak.Builder
flatpak run org.flatpak.Builder --user --install-deps-from=flathub \
  --install --force-clean build-dir \
  packaging/flatpak/io.github.chteau.rbx_native.Editor.yml
flatpak run io.github.chteau.rbx_native.Editor
```

The first build takes a while (luau-lsp and the whole workspace in release).
Add `--ccache` and a `--state-dir` outside the repo to make rebuilds quicker.

The linters Flathub runs:

```sh
flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest packaging/flatpak/io.github.chteau.rbx_native.Editor.yml
flatpak run --command=flatpak-builder-lint org.flatpak.Builder appstream packaging/flatpak/io.github.chteau.rbx_native.Editor.metainfo.xml
flatpak run --command=flatpak-builder-lint org.flatpak.Builder repo repo   # after a build with --repo=repo
```

The `flatpak` job in `.github/workflows/flatpak.yml` builds the manifest on
every PR that touches `packaging/`, `Cargo.lock` or a `build.rs`, so it
doesn't rot.

## When dependencies change

`cargo-sources.json` has to match `Cargo.lock`. After a lockfile change:

```sh
# from a checkout of github.com/flatpak/flatpak-builder-tools
python3 cargo/flatpak-cargo-generator.py /path/to/rbx-native/Cargo.lock \
  -o /path/to/rbx-native/packaging/flatpak/cargo-sources.json
```

Bumping luau-lsp or cloudflared in `rbx_studio/build.rs` means bumping the
tag and commit here too. For cloudflared, regenerate its Go modules from a
checkout of the new tag:

```sh
go install github.com/dennwc/flatpak-go-mod@latest
cd packaging/flatpak/cloudflared
flatpak-go-mod /path/to/cloudflared   # writes go.mod.yml and modules.txt
# turn go.mod.yml into go-sources.json (it's the same list, as JSON)
```

Then fix the `modules.txt` entry's path in `go-sources.json` to
`cloudflared/modules.txt`: flatpak-builder resolves it from the manifest's
folder, not the JSON's.

## Submitting to Flathub

Flathub wants a tagged release, so this happens after `v0.1.0` is tagged on
`main`.

1. Fork `flathub/flathub` and branch off `new-pr` (not `master`).
2. Copy in `io.github.chteau.rbx_native.Editor.yml`, `cargo-sources.json` and
   the `cloudflared/` folder. Leave the rest, the build takes them from the
   tag.
3. In the copied manifest, swap the `rbxnative` module's `type: dir` source
   for the release:

   ```yaml
   - type: git
     url: https://github.com/chteau/rbx-native.git
     tag: v0.1.0
     commit: <the commit v0.1.0 points at>
   ```

4. Check that the metainfo's screenshot links point at a commit that's on
   GitHub, and that its `<release>` matches the tag.
5. Run the three linters above on the result, then open the PR against
   `new-pr`, titled `Add io.github.chteau.rbx_native.Editor`.
6. A reviewer goes over the permissions. The answers are in the section
   above. Once it's merged, Flathub makes a
   `flathub/io.github.chteau.rbx_native.Editor` repo and invites me to it,
   and later releases are PRs there (bump the tag, the commit, the
   `<release>` entry and `cargo-sources.json`).
7. Verifying the app (the checkmark on the page) is done from the Flathub
   developer portal once it's live: for an `io.github` id it's a GitHub
   login as `chteau`, nothing to host.
