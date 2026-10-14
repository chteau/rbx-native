# Launching the real Roblox client into a placeId, and embedding it

**TL;DR**
- Launching into a place needs no auth-ticket dance from our side: `roblox://placeId=<id>` handed to the OS protocol handler (Windows) or to Sober (Linux) uses the client's own login. The ticket/`placelauncherurl` form is what the website builds internally.
- `roblox://placeId=…&launchData=…` is documented, but the page is marked deprecated. `launchData` (≤200 bytes) arrives via `Player:GetJoinData()`, which suits passing a session token to the probe.
- Sober is a closed-source Flatpak that runs Roblox's Android x86-64 build. Its CLI and URI handling is undocumented in what I could fetch, so URI acceptance must be tested live.
- Embedding: X11 reparenting is feasible in principle but untested with Sober's SDL/Vulkan surface. Wayland has no cross-process embedding. Windows `SetParent` on `RobloxPlayerBeta.exe` is risky under Hyperion. Keep the client in its own window.

## Possible

URI formats
- [Verified] The documented deep link is `roblox://placeId=<id>[&launchData=<urlencoded>]`. Other parameters: `userId`, `accessCode`, `linkCode`, `gameInstanceId`. The page is marked "deprecated" in favour of share links. https://create.roblox.com/docs/production/promotion/deeplinks
- [Verified] `launchData` is limited to 200 bytes, is URL-encoded and is read with `Player:GetJoinData()`. Same page.
- [Verified, local clone] `roblox://placeId=…` is the form used throughout the creator-docs, e.g. `experiences.md` and `production/promotion/deeplinks.md`.
- [Reported] The older internal form is `roblox-player:1+launchmode:play+gameinfo:<authticket>+launchtime:<unix>+placelauncherurl:<PlaceLauncher.ashx?request=RequestGame&placeId=…>+browsertrackerid:<id>+robloxLocale:en_us+gameLocale:en_us+channel:`. https://github.com/NoTwistedHere/Roblox-Apis/blob/main/Other/roblox-player.md
- [Reported] The equivalent `RobloxPlayerBeta.exe` command line is `--play -a https://www.roblox.com/Login/Negotiate.ashx -t <gameinfo> -j <placelauncherurl> -b <browsertrackerid> --launchtime=<unix> --rloc en_us --gloc en_us`. Same source.
- [Inferred] `gameinfo` is a one-shot authentication ticket minted by the website. Building that URL ourselves would mean handling the user's cookie or ticket, which this project must not do. Do not use it.
- [Inferred] `roblox://experiences/start?placeId=…` is the newer web-to-app form that the website emits. I found no official doc for it. It is probably equivalent to `roblox://placeId=…`, but test it before depending on it.

Windows
- [Inferred] Launch with `ShellExecute`/`cmd /c start "" "roblox://placeId=<id>&launchData=<token>"`. The registered `roblox:` handler (the Roblox launcher) starts `RobloxPlayerBeta.exe` using the logged-in user's saved session. No ticket is needed, but the user must already be logged in. If the launcher is not logged in, it shows the login or error UI.
- [Reported] Third-party launchers and the `DisableBetaApp` proxy intercept the `roblox-player:` handler, so the handler is the stable integration point rather than the exe's arguments. https://github.com/jrelvas-ipc/DisableBetaApp

Sober (Linux)
- [Verified, local] `flatpak info --show-metadata org.vinegarhq.Sober` shows `command=sober`, runtime `org.gnome.Platform//51`, `tags=proprietary`, and `[Context]`: `shared=ipc;network;`, `sockets=fallback-x11;pulseaudio;wayland;`, `devices=dri;`.
- [Verified] `fallback-x11` means Sober gets X11 only when no Wayland socket is available. It also gets a `wayland` socket. On a Wayland session it therefore defaults to Wayland, and on a pure X11 session it uses X11. `fallback-x11` is documented in flatpak-metadata. Whether the Roblox client itself honours an X11/Wayland override is not documented.
- [Verified] The sandbox has no `--filesystem`, `--talk-name` or `--socket=session-bus` permission. It has `shared=ipc`, so X11 SHM/MIT-SHM works.
- [Verified] Sober is closed source, experimental and unsupported by Roblox. It runs a specialised runtime for Roblox's Android code, not Wine. https://sober.vinegarhq.org/
- [Verified] On first run Sober prompts to install Roblox (automatic or manual APK). It requires Vulkan 1.1 (recommended) or GLES 3.0. https://vinegarhq.org/Sober/Installation.html
- [Reported] Sober's site says "Open your favorite Roblox experience / Sober will prompt you for installation". This implies it registers as the `roblox:` and `roblox-player:` handler via its exported `.desktop` file, so a browser click launches it. https://sober.vinegarhq.org/
- [Inferred] Launch from `rbxstudio` with `flatpak run org.vinegarhq.Sober "roblox://placeId=<id>"` (the URI as argv[1]). The alternative is `xdg-open roblox://…` if the handler is registered. I could not read the `.desktop` file (outside the allowed dirs), so I could not confirm its `Exec=`/`MimeType=` lines. Check `Exec=… @@u %u @@` and `MimeType=x-scheme-handler/roblox;x-scheme-handler/roblox-player;`.

Auth
- [Inferred] The sandbox flow needs only the client's existing login. Roblox clients log in once and persist the session. Our app never sees credentials, which satisfies the "no cookies" rule.
- [Inferred] The sandbox place being private means the user's account must be the owner, or a collaborator in the test universe. Otherwise the client shows a "not authorized" error. This is also true of an unpublished or private place.

## Not possible

- [Inferred] The sandbox cannot be headless or windowless on Linux. Sober has no documented headless mode.
- [Inferred] We cannot pass an auth ticket to Sober ourselves without reading the user's session. Don't try.
- [Verified] Sober cannot be inspected or patched, since it is closed source. Any Sober-specific CLI flag would have to come from `sober --help`, which I did not run.
- [Inferred] On Wayland, one client cannot embed another process's `wl_surface`. There is no XReparentWindow equivalent in core Wayland. `xdg-foreign` only supports parent/child *relationship* between top-levels (positioning and stacking), not embedding.
- [Inferred] Embedding Sober as a Wayland client therefore needs a nested compositor, e.g. a `cage`/`gamescope`/`weston` instance running Sober and rendered into a gpui texture. That means a dmabuf/shm capture path and forwarding input back. It is a large project and out of scope for the first iteration.

## Embedding

X11 (`XReparentWindow`)
- [Inferred] On an X11 session (or with Sober forced to X11), find Sober's top-level window through `_NET_WM_PID` or `WM_CLASS`, then `XReparentWindow(child, our_window, x, y)`, with `XSelectInput` and `XMoveResizeWindow` on resize. Flatpak sandboxing does not interfere with X11 reparenting: it is an X server operation and both processes are clients of the same display (`sockets=fallback-x11`/`shared=ipc`).
- [Inferred] Caveats:
  - The window must be mapped and un-decorated first. Remove decorations through `_MOTIF_WM_HINTS` before reparenting.
  - The window manager may re-manage it.
  - A Vulkan/GL surface on X11 uses a drawable bound to the X window. Reparenting a live swapchain window generally works (browsers and PiP do it), but the Android-runtime SDL path is untested.
  - Input focus needs `XSetInputFocus` or XEmbed `_XEMBED` messages. SDL clients do not speak XEmbed, so focus has to be forced by hand. Pointer lock (used by the game camera) may break across reparenting.
  - gpui renders with wgpu on its own surface. Child X windows would overlay it as native subwindows with no compositing, so overlays such as dropdowns cannot draw above the embedded client.
- [Inferred] It only works when the user is on X11. The maintainer's machine is X11, which suits a prototype.
- [Reported] Sober on Wayland and XWayland: community guides say it works on both. https://axeetech.com/sober-roblox-linux-guide/

Windows (`SetParent`)
- [Reported] Hyperion (Byfron) is the anti-tamper layer in the Windows client. It blocks injection, debuggers and memory tampering. https://roblox.fandom.com/wiki/Hyperion and https://devforum.roblox.com/t/exploit-prevention-update/2663101?page=57
- [Inferred] `SetParent` on another process's top-level window is a USER32 call on a window we don't own. It is not injection or memory access, so it is unlikely to be flagged. It does change styles (`WS_CHILD`), may break DPI handling, input focus and the fullscreen/DXGI swapchain path, and Roblox may react to the window-style change. Roblox has no published guidance. Treat as unverified and risky. There is no ban-risk statement either way.
- [Inferred] Safer alternative on Windows: keep the client as a separate window and position it next to the editor (`SetWindowPos`), or use DWM thumbnails (`DwmRegisterThumbnail`) for display-only preview.
- [Reported] Hyperion has also been reported to crash with security software that hooks windows (Kaspersky). https://devforum.roblox.com/t/hyperion-client-crash-hyptmp-with-kaspersky-in-multi-profile-windows-setup/4926131

## Unknown / needs a live test

- Does `flatpak run org.vinegarhq.Sober roblox://placeId=…` join directly, or only through `xdg-open`? What does the installed `.desktop` `Exec=` contain?
- If Sober is already running (the user is playing), does a second launch start a second instance, reuse the first, or kick the running session? See the separate note on running a second client without disconnecting the live session.
- Is `roblox://experiences/start?placeId=…` accepted by Sober and the Windows launcher?
- Does a private (unpublished/owner-only) place join from the URI with the client's own login, and what is the error when the logged-in account is not the owner?
- Does `launchData` reach `Player:GetJoinData()` through Sober?
- Which window class/title does Sober's X11 window use, and does it survive `XReparentWindow`: Vulkan swapchain, pointer lock, keyboard focus?
- On Wayland, whether Sober can be forced to XWayland (e.g. `SDL_VIDEODRIVER=x11` via `flatpak run --env=…`) so X11 embedding works in a Wayland session.
- Windows: whether `SetParent` on `RobloxPlayerBeta.exe` triggers Hyperion or merely misbehaves.

## Impact on the roadmap design

- Step 2: The launch step is simple: call the protocol handler with `roblox://placeId=<sandbox>&launchData=<session token>` (Windows: shell open; Linux: `flatpak run org.vinegarhq.Sober <uri>` with an `xdg-open` fallback). Remove any plan to mint auth tickets or read cookies. Document that the user must be logged in on the client, and must own the sandbox place.
- Step 2 (embedding): Keep it optional. A prototype on X11 using `XReparentWindow` is plausible; Wayland needs a nested compositor and is out of scope; on Windows prefer window positioning over `SetParent` because of Hyperion. Do not make any other step depend on it.
- Step 4: `launchData` (200 bytes) can carry the tunnel session token or the endpoint id, so the probe needs no hard-coded URL. It is visible to the player and to the server, so treat it as a low-value token only.
- Step 3: Add a note that a plain page-level `GetJoinData` read is how the probe picks up its config.

## Sources

- https://create.roblox.com/docs/production/promotion/deeplinks
- https://github.com/NoTwistedHere/Roblox-Apis/blob/main/Other/roblox-player.md
- https://github.com/jrelvas-ipc/DisableBetaApp
- https://sober.vinegarhq.org/
- https://vinegarhq.org/Sober/Installation.html
- https://github.com/vinegarhq/sober
- https://axeetech.com/sober-roblox-linux-guide/
- https://roblox.fandom.com/wiki/Hyperion
- https://devforum.roblox.com/t/exploit-prevention-update/2663101?page=57
- https://devforum.roblox.com/t/hyperion-client-crash-hyptmp-with-kaspersky-in-multi-profile-windows-setup/4926131
