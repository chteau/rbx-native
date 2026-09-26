# Themes

A theme is a folder that restyles the whole editor: its palette, its size and
type scale, the toolkit's own widgets, the class icons, and a few effects —
a transparent or blurred window, a glow on hover, a background image. The
editor's own look is itself a theme, **Default** (by @chteau), built in and
impossible to uninstall. Every other theme is layered over it, so a theme
only has to name what it changes.

## Installing one

Themes live in the config directory, one folder each:

```text
<config>/themes/<id>/            ~/.config/rbx-native on Linux, %APPDATA%\rbx-native on Windows
<config>/appearance.json         {"theme": "<id>"}
```

Put a theme's folder in `themes/` and name it in `appearance.json` (`"default"`
goes back to Default). The editor picks the change up within a second — no
restart — and re-applies the active theme whenever one of its files is
saved, which is also how to preview a theme while writing it. A theme that
fails to load is named in the Output dock with what is wrong, and the
current one stays on screen.

The editor can also install a theme straight from a GitHub repository link;
that arrives with the settings screen.

## Writing one

```text
manifest.json    required
preview.png      required (PNG, JPEG or WebP; any name, see "preview")
theme.json       optional: colours, sizes, effects
widgets.json     optional: the toolkit's widgets (a GPUI Kit ThemeSet)
icons/           optional: an icon pack, one SVG per class
```

Anything optional that is left out comes from Default: no `icons/` draws the
built-in icons, no `widgets.json` keeps Default's widgets.

### `manifest.json`

```json
{
  "name": "Neon Dusk",
  "author": "@you",
  "description": "Translucent chrome over a gradient, with a pink accent.",
  "version": "1.0.0",
  "preview": "preview.png"
}
```

Every field is required and non-empty. `preview` is a path inside the theme
folder.

### `theme.json`

[`assets/themes/default/theme.json`](assets/themes/default/theme.json) is the
complete list of tokens with Default's values — copy it as a starting point,
then delete what you don't change.

```json
{
  "colors": {
    "dock": "#141024C8",
    "check_on": "#FF5FA2",
    "selection": "@check_on/0.45"
  },
  "sizes": { "radius": 8, "text_md": 13 },
  "window": "transparent",
  "background": { "image": "bg.png", "opacity": 0.8, "fit": "cover", "layer": "behind" },
  "hover": { "glow": "@check_on/0.7", "glow_radius": 10 }
}
```

- **Colours** are `#RRGGBB`, `#RRGGBBAA`, or a reference to another colour
  token: `@dock`, or `@check_on/0.12` for the same colour at another alpha.
  References resolve after your theme is layered over Default, so changing
  `dock` also changes `tile` and `chrome`, which Default defines as `@dock`.
- **Sizes** are pixels before the UI scale (Ctrl+= / Ctrl+-), between 0 and
  2000. Minimum click-target sizes are an accessibility floor and are not
  themeable.
- **`window`**: `opaque` (default), `transparent` or `blurred`. The desktop
  only shows through surfaces you also gave some transparency (`black` is
  the ground behind everything, `dock` the panels). Blur needs a compositor
  that offers it and falls back to plain transparency.
- **`background`**: an image (PNG, JPEG, WebP or GIF, under 16 MiB) inside
  the theme. `opacity` 0–1, `fit` is `cover`, `contain` or `fill`, and
  `layer` is `behind` (shows through translucent surfaces) or `over` (on top
  of everything, never taking a click).
- **`hover`**: a `glow` colour and `glow_radius` (0–64) added to every hover
  state the editor draws.

A token name the editor doesn't know is skipped with a warning, so a theme
written for a newer version still loads; a value that doesn't parse stops
the theme loading, with the token named.

### `icons/`

One SVG per class, named after the class (`Part.svg`) or after the built-in
kit's tile it replaces (`humanoid-description.svg`, which reaches every class
sharing that tile). Classes the theme leaves out keep the built-in icon. An
icon pack chosen in the Explorer's menu is drawn over the theme's icons.

### `widgets.json`

Inputs, popovers, scrollbars and the focus ring are GPUI Kit's own widgets
and read a GPUI Kit `ThemeSet`; the first dark theme in the file is used.
[`assets/themes/default/widgets.json`](assets/themes/default/widgets.json)
is Default's.
