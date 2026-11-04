# What the DevForum asks for in Studio

A pass over the DevForum's **Feature Requests › Studio Features** category
(<https://devforum.roblox.com/c/feature-requests/studio-features>), sorted
by likes, all time and the last two years, read on 2026-11-03 through the
forum's public `top.json` listing. Like counts are the topic's own count on
that day. The point was to find what developers have been asking Roblox
for, check it against rbx-native, and build the ones that fit.

Left out on purpose: requests about Roblox's installer, launcher, website,
moderation, monetisation or Marketplace rules (nothing an editor can
change), Luau language proposals (luau-lang's RFC process owns those) and
engine-runtime classes (new `GuiObject`s, physics, audio APIs).

Status, as of `dev` on 2026-11-03:

- **shipped**: rbx-native already does it; where it lives in `ROADMAP.md`.
- **tonight**: built on its own branch during this pass.
- **planned**: feasible, added to `ROADMAP.md` › What's planned.
- **impossible**: needs Roblox's engine, servers or client; why.

| Request | Likes | Status in rbx-native |
| --- | ---: | --- |
| [Integrate Rojo into Roblox Studio officially](https://devforum.roblox.com/t/integrate-rojo-into-roblox-studio-officially/657577) | 2499 | **shipped**: native Argon live sync, plus a Rojo-format `sourcemap.json` (Editor › Argon) |
| [It should be possible to use Roblox Studio offline](https://devforum.roblox.com/t/it-should-be-possible-to-use-roblox-studio-offline/153244) | 1964 | **shipped**: local files open, edit and save with no sign-in; the `rbxasset://` fallback keeps a place readable offline (Platform) |
| [Colored folders!](https://devforum.roblox.com/t/colored-folders/20739) | 933 | **shipped**: colour-coded Explorer folders |
| [Add a dark theme for studio](https://devforum.roblox.com/t/add-a-dark-theme-for-studio/25932) | 725 | **shipped**: whole-editor themes, Dark, Light and High Contrast built in |
| [Colors in the output window](https://devforum.roblox.com/t/colors-in-the-output-window/51196) | 633 | **planned**: opt-in rich-text colour for Output lines (a printed string is the game's, so it stays off by default) |
| [Expanded UI Elements for Roblox (Dropdowns, Sliders, etc)](https://devforum.roblox.com/t/expanded-ui-elements-for-roblox-dropdowns-sliders-etc/2538016) | 618 | **impossible**: new runtime `GuiObject` classes only exist if the engine has them |
| [FBX Exporting Option](https://devforum.roblox.com/t/fbx-exporting-option/231566) | 483 | **planned**: OBJ and glTF export are shipped; FBX would be a third writer |
| [Allow Studio plugins to copy things to clipboard](https://devforum.roblox.com/t/allow-studio-plugins-to-copy-things-to-clipboard/638682) | 400 | **planned**: an rbx-native `plugin:SetClipboard` addition, like `PromptColor3` |
| [Allow plugins to use color picker](https://devforum.roblox.com/t/allow-plugins-to-use-color-picker/63257) | 334 | **shipped**: `plugin:PromptColor3` / `PromptBrickColor` (Plugins) |
| [Custom Intellisense Documentation Comments](https://devforum.roblox.com/t/custom-intellisense-documentation-comments/110378) | 288 | **shipped**: luau-lsp hover reads `---` doc comments (Script Analysis) |
| [Have parts anchored by default when inserting in Studio](https://devforum.roblox.com/t/have-parts-anchored-by-default-when-inserting-in-studio/347169) (and [another](https://devforum.roblox.com/t/can-we-make-the-default-part-anchored/85180), 126) | 265 | **planned**: a Studio Settings switch for inserted parts |
| [Custom Studio Interface (Themes and Icons)](https://devforum.roblox.com/t/custom-studio-interface-themes-and-icons/1535715) | 256 | **shipped**: installable themes and icon packs |
| [Add ability to browse / view instances that are parented to `nil`](https://devforum.roblox.com/t/add-ability-to-browse-view-instances-that-are-parented-to-nil/1664441) | 237 | **impossible**: nil-parented instances only exist in a running game's memory, and Play runs in Roblox's own client |
| [Create an orthographic view](https://devforum.roblox.com/t/create-an-orthographic-view/1300021) | 191 | **shipped**: orthographic camera toggle |
| [Ctrl+Shift+H to find and replace in all scripts](https://devforum.roblox.com/t/ctrlshifth-to-find-and-replace-in-all-scripts/35255) | 178 | **shipped** for open scripts (Find All / Replace All); **planned** for every script in the place |
| [Provide an exploit investigation environment](https://devforum.roblox.com/t/provide-an-exploit-investigation-environment/3083740) | 163 | **impossible**: needs Roblox's client and server |
| [Customizable folder icons for studio explorer](https://devforum.roblox.com/t/customizable-folder-icons-for-studio-explorer/256538) | 157 | **shipped** as colour tags; per-folder icon art is not planned |
| [Allow developers to save places as studio templates](https://devforum.roblox.com/t/allow-developers-to-save-places-as-studio-templates/241595) | 156 | **tonight**: `feat/place-templates` |
| [Ability to collapse all folds in explorer](https://devforum.roblox.com/t/ability-to-collapse-all-folds-in-explorer/69318) | 150 | **tonight**: `feat/explorer-hierarchy` (real Studio has it now, under Hierarchy) |
| ["Magic Wand" Selection Tool for Studio](https://devforum.roblox.com/t/magic-wand-selection-tool-for-studio/1489595) | 147 | **shipped** as query selection (`Part where Color == ...`) |
| [Copy Properties From Object](https://devforum.roblox.com/t/copy-properties-from-object/224274) | 119 | **tonight**: `feat/copy-paste-properties` |
| [Studio layout saving](https://devforum.roblox.com/t/studio-layout-saving/10639) | 113 | **shipped**: dock layout persists, with a reset |
| [Script Editor should save which folds are collapsed](https://devforum.roblox.com/t/script-editor-should-save-which-folds-are-collapsed-and-expa/31297) | 110 | **planned**: the script editor has no folding yet, which comes first |
| [Automatically make a history waypoint after the command bar is ran](https://devforum.roblox.com/t/automatically-make-a-history-waypoint-after-the-command-bar-is-ran/56619) | 108 | **shipped**: a Command Bar run is one undo step |
| [Please add a search bar for the plugin management window](https://devforum.roblox.com/t/please-add-a-search-bar-for-the-plugin-management-window/2016337) (and [2025's](https://devforum.roblox.com/t/add-a-search-bar-to-plugin-management-and-overwrite-an-existing-asset/4137618), 65) | 103 | **planned**: the Plugins window searches the store, not the installed list |
| [Collapse/Expand All on Selected Explorer Objects](https://devforum.roblox.com/t/collapseexpand-all-on-selected-explorer-objects/78905) | 102 | **tonight**: `feat/explorer-hierarchy` |
| [Allow selecting locked parts by holding a key combination](https://devforum.roblox.com/t/allow-selecting-locked-parts-by-holding-a-key-combination/977989) | 99 | **planned** |
| [Multiple Lines in the Command Bar](https://devforum.roblox.com/t/multiple-lines-in-the-command-bar/233343) (and [2024's](https://devforum.roblox.com/t/it-is-too-hard-to-edit-multiline-code-in-the-command-bar/3013101), 47) | 97 | **planned** |
| [Ability to right click, copy path of an instance](https://devforum.roblox.com/t/ability-to-right-click-copy-path-of-an-instance/2839062) | 84 | **tonight**: `feat/explorer-hierarchy` (real Studio has it now, under Copy As) |
| [Filter by Attributes in Explorer](https://devforum.roblox.com/t/filter-by-attributes-in-explorer/2828954) | 50 | **shipped**: query selection, `has attr Name` / `attr Name == ...` |
| [Easier way of getting an image's resolution](https://devforum.roblox.com/t/easier-way-of-getting-an-images-resolution/4225605) | 48 | **planned** |
| [Build stylua into Roblox Studio](https://devforum.roblox.com/t/build-stylua-into-roblox-studio/3645518) | 44 | **planned** |
| [Version History needs naming, search, and folders](https://devforum.roblox.com/t/version-history-needs-naming-search-and-folders/4382404) | 41 | **planned**: browse and restore are shipped; search is not. Naming a version needs Roblox's servers to store the name, so only a local label is possible |
| [Warn when two instances share the same name and are accessed by scripts](https://devforum.roblox.com/t/warn-when-two-instances-share-the-same-name-and-are-accessed-by-scripts/2698911) | 36 | **shipped** |
| ["Fullbright" Option in Studio](https://devforum.roblox.com/t/fullbright-option-in-studio/4622596) | 35 | **planned** |
| [AudioPlayer should have Play button in Properties Pane](https://devforum.roblox.com/t/audioplayer-should-have-play-button-in-properties-pane/3790735) | 32 | **planned** |
| [Add server/client context option for ModuleScripts](https://devforum.roblox.com/t/add-server-client-context-option-for-modulescripts/4192753) | 65 | **impossible**: where a module runs is the engine's call |
| [Allow Developers to Test Teleports to Other Places in Studio](https://devforum.roblox.com/t/allow-developers-to-test-teleports-to-other-places-in-studio-without-a-second-device/4336630) | 34 | **impossible**: teleports go through Roblox's game servers |

## What got built, and why those

Three made the cut: they are editor work with no engine in the way, the
community has asked for them for years, and each could be finished and
checked on a real build in one night.

- **Explorer Copy Path, Expand All, Collapse All, Select Children**
  (`feat/explorer-hierarchy`). Roblox shipped these in real Studio since
  the threads were opened: right-clicking an instance there shows
  **Copy As › Copy Path** and **Hierarchy › Select Children / Expand All /
  Collapse All**, checked in Studio through Vinegar on 2026-11-03. So this
  one is parity. What Studio does, measured: Copy Path writes
  `game.Workspace.Baseplate`, a service by its name
  (`game.ReplicatedStorage`, not `GetService`), and a name that is not an
  identifier in brackets (`game.Workspace["my part"]`). It writes a Luau
  keyword name as `game.Workspace.end`, which does not parse; ours
  brackets keywords too. Expand All opens the instance and everything
  under it; Collapse All closes the instance and everything under it, so
  reopening it later shows its children closed.
- **Copy Properties / Paste Properties** (`feat/copy-paste-properties`).
  Not in real Studio. The thread's use case is copying a whole set of
  values, such as Lighting's, from one instance or place to another.
- **Save as Template** (`feat/place-templates`). Not in real Studio. The
  thread asks for a "Saved Templates" row on New, filled from your own
  places, for a lighting setup or folder layout you start every project
  with.

## Skipped tonight, and why

- **Colors in the output window** (633): a game prints into Output through
  Roblox's own client during Play, and Studio shows those strings
  literally. Turning tags into colour changes what a printed string means,
  so it has to be an opt-in setting, and doing that properly touches every
  Output source (Command Bar, plugins, Play). Too big to finish well
  tonight.
- **Parts anchored by default** (265): small, but it changes what every
  insert path writes (ribbon Part tiles, Insert Object, the UI Editor), so
  it wants its own settings row and tests rather than a rushed one.
- **Multi-line Command Bar** (97): our Command Bar is a single-line field;
  a real multi-line editor there means reusing the script editor kit
  inside the Output dock, a bigger UI change than one night.
- The rest of the planned rows are smaller-voted or need work first
  (folding before saved folds, an installed-plugins list before its
  search).
