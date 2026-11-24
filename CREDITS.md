# Credits

RbxNative is built on other people's work. These are the third-party fonts, icons, 3D models, data and programs it ships or fetches, and the projects it took ideas from: who made them, under what licence, and what RbxNative uses them for. The same list is in the app under Help › About RbxNative.

This file is generated from `assets/credits.toml`. Edit that, then run `RBX_WRITE_CREDITS=1 cargo test -p rbx_studio credits`.

Every package carries the licence texts that have to travel with the files in its `licenses/` folder, next to `THIRD-PARTY-LICENSES.html` (the Rust crates compiled in) and `THIRD-PARTY-EMBEDDED.txt` (the programs carried inside the editor).

## Fonts

| What | Owner | Licence | Credit required | Used for |
|---|---|---|---|---|
| [Manrope](https://github.com/googlefonts/manrope) | Mikhail Sharanda and the Manrope Project Authors | SIL Open Font License 1.1 | yes | The editor's interface typeface, built into the program. |
| [JetBrains Mono](https://github.com/JetBrains/JetBrainsMono) | JetBrains and the JetBrains Mono Project Authors | SIL Open Font License 1.1 | yes | The script editor's and every code field's monospace font, built into the program. |

## Icons and graphics

| What | Owner | Licence | Credit required | Used for |
|---|---|---|---|---|
| [Lucide](https://lucide.dev) | Lucide Icons and Contributors (Feather icons by Cole Bemis) | ISC (the Feather-derived icons MIT) | yes | The toolbar, menu and dock icons, from the catalogue longbridge's gpui-kit-assets packs. |
| [Twemoji](https://github.com/jdecked/twemoji) | Twitter, Inc., jdecked and other contributors | CC BY 4.0 | yes | The emoji page of the Device Simulator's keyboard, unmodified. |

## Free 3D models

| What | Owner | Licence | Credit required | Used for |
|---|---|---|---|---|
| [Poly Haven](https://polyhaven.com) | Poly Haven and its artists | CC0 1.0 | yes | The Toolbox's Free kits, browsed live. Each model's card names its artists, and an imported model keeps Author, Source and License attributes. The models need no credit; the Poly Haven API's terms ask for Poly Haven to be named where its content shows. |
| [Kenney](https://kenney.nl) | Kenney | CC0 1.0 | no | 3D kit packs you download from kenney.nl and add to the Toolbox's Free kits. Nothing is downloaded from kenney.nl by RbxNative itself. |

## Code and data built in

| What | Owner | Licence | Credit required | Used for |
|---|---|---|---|---|
| [Loren](https://github.com/fabideveloper/Loren-Framework) | fabi | MIT | yes | The optional built-in game framework: its Luau runtime, project template and specs, and its CLI ported as rbxstudio loren. |
| [roblox-lua-promise](https://github.com/evaera/roblox-lua-promise) | Eryn L. K. | MIT | yes | The Promise library Loren's runtime bundles. |
| [csg.js](https://github.com/evanw/csg.js) | Evan Wallace | MIT | yes | The BSP-tree boolean algorithm behind Union, Negate and Intersect, ported to Rust. |
| [rbx-dom reflection database](https://github.com/rojo-rbx/rbx-dom) | The Rojo Developers | MIT | yes | Each class's default property values and serialized property names (assets/reflection-defaults.json), and the binary format docs. |
| [Roblox API dump](https://github.com/MaximumADHD/Roblox-Client-Tracker) | Roblox Corporation, mirrored by MaximumADHD's Roblox-Client-Tracker | No licence stated | no | Every class, property and enum the editor knows, built in (assets/API-Dump.json). |
| [luau-lsp](https://github.com/JohnnyMorganz/luau-lsp) | JohnnyMorganz | MIT | yes | Autocomplete, hover, Go to Definition and Script Analysis. Carried inside the program; its licence and Luau's are in THIRD-PARTY-EMBEDDED.txt. |
| [Luau](https://github.com/luau-lang/luau) | Roblox Corporation and Lua.org, PUC-Rio | MIT | yes | The scripting language: luau-lsp is built from it, and the plugin host runs it. |
| [cloudflared](https://github.com/cloudflare/cloudflared) | Cloudflare | Apache-2.0 | yes | Play's tunnel. Carried inside the program; licence in THIRD-PARTY-EMBEDDED.txt. |
| [slirp4netns](https://github.com/rootless-containers/slirp4netns) | The slirp4netns authors (rootless-containers) | GPL-2.0-or-later | yes | Play's network simulator on Linux. Carried inside the program, unmodified; licence and source offer in THIRD-PARTY-EMBEDDED.txt. |

## Fetched while you work

| What | Owner | Licence | Credit required | Used for |
|---|---|---|---|---|
| [Roblox](https://www.roblox.com) | Roblox Corporation | Roblox Terms of Use | no | Studio's content packages (the default sky, material textures), assets, thumbnails and the Creator Store, downloaded when a place needs them. The setup wizard's three Creator Dashboard screenshots show Roblox's own pages. |
| [Roblox Creator Documentation](https://github.com/Roblox/creator-docs) | Roblox Corporation | CC BY 4.0 (code samples MIT) | yes | The documentation pages the MCP docs tool reads for an AI assistant, fetched from their source. |
| [Roblox-Client-Tracker](https://github.com/MaximumADHD/Roblox-Client-Tracker) | MaximumADHD | No licence stated | no | Roblox's API reference text for hover docs, downloaded at run time. Its shader sources are what the renderer's lighting was studied from. |
| [luau-lsp type definitions](https://github.com/JohnnyMorganz/luau-lsp) | JohnnyMorganz | MIT | yes | Roblox's API types for autocomplete, downloaded at run time. |

## Ideas and references

| What | Owner | Licence | Credit required | Used for |
|---|---|---|---|---|
| [Reclass and Relight](https://www.roblox.com/users/1670764/profile) | Elttob | Idea only | no | Inspired Change Class and sun placement. Rebuilt from how the plugins behave; no code, assets or UI used. |
| [Icon Updater](https://vm.tiktok.com/ZN8AFbRwp/) | MiniDev | Idea only | no | Update planning's countdown icons follow MiniDev's design. No code or assets used. |
| [Community redesign concept](https://www.roblox.com/users/1324764019/profile) | u/1324764019 on the Roblox DevForum | Idea only | no | The calmer dock and panel look the editor's theme takes its direction from. |
| [Argon](https://argon.wiki) | argon-rbx | Apache-2.0 | no | The file sync tool the Argon dock connects to; its plugin source is where the wire protocol was checked. No code copied. |
| [Wally](https://wally.run) | UpliftGames | MPL-2.0 | no | The package manager the Wally dock is built around; its source is where the registry API and install layout were checked. No code copied. |
| [Roblox-Materials and the FileMesh spec](https://github.com/MaximumADHD/Roblox-Materials) | MaximumADHD | Reference only | no | The material texture pack layout and the mesh format the renderer reads. |
| [Bytecode viewer request](https://devforum.roblox.com/t/bytecodenative-code-viewer/4964517) | A Roblox DevForum feature request | Idea only | no | The Bytecode dock. |
