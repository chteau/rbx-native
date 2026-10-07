//! Persisted Studio preferences: the graphics quality dropdown, the
//! Explorer's "show all services" checkbox, the Viewport's orthographic
//! toggle, its orientation indicator toggle, whether the selection box is
//! occluded by geometry, whether light guides are shown, and the Explorer's
//! icon pack (dark/light), so a relaunch reopens where the user left off
//! rather than always at the hardcoded defaults.
//!
//! Mirrors `rbx_assets::AssetCache`'s directory convention (`$XDG_CONFIG_HOME`,
//! falling back to `~/.config` or, on Windows, `%APPDATA%`, all under an
//! `rbx-native` folder) and its temp-file-then-rename write, but for one small
//! config document rather than many cached blobs, and `config` rather than
//! `cache` — a setting should survive `rm -rf ~/.cache`.

use std::path::{Path, PathBuf};

use rbx_viewer::QualityLevel;

use crate::class_icons::IconPack;
use crate::pacing::UnfocusedFps;
use crate::shell::{Edge, SavedEdge, SavedGroup, SavedLayout};

pub(crate) mod argon;
mod controls;
mod dragger;

pub(crate) use controls::{Controls, FEEL_SCALE_RANGE};
pub(crate) use dragger::DraggerSettings;

/// What persists across a relaunch.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Settings {
    pub(crate) quality: QualityLevel,
    pub(crate) show_all_services: bool,
    /// Which services the Explorer's default view lists, where that differs
    /// from Studio's — see `explorer::ServiceOverrides`.
    pub(crate) service_overrides: crate::explorer::ServiceOverrides,
    pub(crate) orthographic: bool,
    /// The viewport's top-right orientation indicator — see
    /// `crate::workspace_view::orientation`. Defaults on: it's meant to read
    /// as an always-there convention (the way Blender's own gizmo is), not
    /// an opt-in debug overlay.
    pub(crate) axis_indicator: bool,
    /// Whether a part standing in front of the selection hides its outline
    /// box. Defaults off, so the box draws through everything: that is what
    /// Studio does, and what keeps a `Model` selected behind a wall visible
    /// at all. Named for the behaviour being switched *on* rather than for
    /// the default, so no call site has to read `!show_through`.
    pub(crate) selection_occluded: bool,
    /// Studio's `Show Light Guides`: the lines drawn around a selected
    /// light. Defaults on — Roblox's announcement of the feature tells
    /// anyone who finds them in the way to turn them off in settings.
    pub(crate) light_guides: bool,
    pub(crate) icon_pack: IconPack,
    /// The render loop's frame rate cap while the window is unfocused — see
    /// `pacing::FocusPacing`.
    pub(crate) unfocused_fps: UnfocusedFps,
    /// The UI scale — one multiplier over every font size *and* the boxes
    /// they sit in (`tokens::font_scale`). This is how a native app meets
    /// WCAG 1.4.4's 200% resize, since there is no browser zoom to lean on;
    /// the range is Blender's Resolution Scale range, for the same reason.
    pub(crate) font_scale: f32,
    /// The renderer's tuned constants (Studio Settings › Viewport ›
    /// Advanced). The default is the tuned set; see `rbx_viewer::Calibration`.
    pub(crate) calibration: rbx_viewer::Calibration,
    /// Whether Auto-Recovery writes a background copy of the open place
    /// (see `crate::recovery`), and how many minutes apart.
    pub(crate) auto_recovery: bool,
    pub(crate) recovery_minutes: u32,
    /// The Script Editor's text size at 1x, in px, before [`Self::font_scale`]
    /// multiplies it like every other size. See [`SCRIPT_FONT_SIZE`].
    pub(crate) script_font_size: f32,
    /// The size of the text the editor draws over the 3D view, in px at 1x,
    /// before [`Self::font_scale`] multiplies it. See [`VIEWPORT_FONT_SIZE`].
    pub(crate) viewport_font_size: f32,
    /// Raises the minimum pointer target from WCAG 2.5.8's 24px floor to
    /// 2.5.5's 44px one — Blender's "editor-area padding" idea, which its
    /// own manual describes as improving usability "on pen tablets, touch
    /// screens, or for users with visual or physical accessibility issues".
    pub(crate) large_targets: bool,
    /// Whether to suppress motion. `None` follows the desktop's own
    /// setting; `Some` is an explicit choice made in the View menu, because
    /// an accessibility preference that can only be set with an environment
    /// variable is not a setting anybody has.
    pub(crate) reduce_motion: Option<bool>,
    /// The dock layout — which panel is on which edge, and how big each
    /// edge is — so it survives a relaunch, the reference doc's Stage 2
    /// item 9. Not a separate file: a layout is a preference like any
    /// other, and a second persistence path is a second thing to keep in
    /// step.
    ///
    /// Read back through `shell::Layout::restore`, which is total over
    /// whatever the file holds, so nothing here has to validate it.
    pub(crate) docks: SavedLayout,
    /// Dock layouts saved under a name from Studio Settings › Layout, to
    /// switch between (Blender's workspaces); see [`NamedLayout`].
    pub(crate) named_layouts: Vec<NamedLayout>,
    pub(crate) output_collapsed: bool,
    /// Whether Output rows print their time.
    pub(crate) output_timestamps: bool,
    /// Whether opening a script's Graph side tidies it, rather than
    /// restoring the layout saved for it.
    pub(crate) optimize_graph_on_open: bool,
    /// Real Studio's two insertion preferences, off the `⋯` beside the
    /// Explorer's insert search field (`studio/explorer.md`). Both default
    /// on, as they do there: a second `Part` called `Part` is not something
    /// anyone asks for, and an insert whose row the tree never expands to
    /// reveal reads as an insert that did nothing.
    pub(crate) increment_names: bool,
    pub(crate) expand_on_select: bool,
    /// The dragger guides' switches — see [`DraggerSettings`].
    pub(crate) dragger: DraggerSettings,
    /// The camera's feel and the snap increments — see [`Controls`].
    pub(crate) controls: Controls,
    /// The Argon dock's address field, as it stood the last time Connect
    /// actually succeeded — not every keystroke of a draft, and not an
    /// address that never connected. Empty means "nothing saved yet",
    /// which `Shell` reads as its own hardcoded `localhost:8000` default.
    pub(crate) argon_address: String,
    /// Argon's own plugin settings, per level — see [`argon::ArgonSettings`].
    pub(crate) argon: argon::ArgonSettings,
    pub(crate) discord_presence: bool,
    /// Defaults on: names are hidden unless the user opts in.
    pub(crate) discord_hide_names: bool,
}

/// A dock layout saved under a name.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NamedLayout {
    pub(crate) name: String,
    pub(crate) layout: SavedLayout,
}

/// The Script Editor's default text size: the toolkit theme's own code size
/// (`mono_font_size`, 13px), so a file without the setting looks the way the
/// editor always has at 1x.
pub(crate) const SCRIPT_FONT_SIZE: f32 = 13.;

/// What the Script Font Size field accepts. Below 8px code stops being
/// legible; above 32px a line holds too little of it, and the UI scale is
/// the setting for making everything larger.
pub(crate) const SCRIPT_FONT_SIZE_RANGE: (f32, f32) = (8., 32.);

/// Clamped rather than rejected, like the UI scale: a hand-edited `100`
/// should open the editor at the largest size, not discard the file.
pub(crate) fn clamp_script_font_size(size: f32) -> f32 {
    if size.is_finite() {
        size.clamp(SCRIPT_FONT_SIZE_RANGE.0, SCRIPT_FONT_SIZE_RANGE.1)
    } else {
        SCRIPT_FONT_SIZE
    }
}

/// The viewport's default text size: the readouts' own 12px, so a file
/// without the setting draws the 3D view's text the way it always has. Every
/// other piece of viewport text grows in proportion to it.
pub(crate) const VIEWPORT_FONT_SIZE: f32 = 12.;

/// What the Viewport Font Size field accepts: down to the script range's
/// floor, and up to 200% of the default — WCAG 1.4.4's figure on its own,
/// before the UI scale doubles it again. Larger and a guide label covers
/// the part it is measuring.
pub(crate) const VIEWPORT_FONT_SIZE_RANGE: (f32, f32) = (8., 24.);

/// Clamped rather than rejected, like the script font size.
pub(crate) fn clamp_viewport_font_size(size: f32) -> f32 {
    if size.is_finite() {
        size.clamp(VIEWPORT_FONT_SIZE_RANGE.0, VIEWPORT_FONT_SIZE_RANGE.1)
    } else {
        VIEWPORT_FONT_SIZE
    }
}

impl Default for Settings {
    /// Same defaults `Shell`/`main` used before either was configurable, so a
    /// missing settings file changes nothing about a first run.
    fn default() -> Self {
        Settings {
            quality: QualityLevel::Automatic,
            show_all_services: false,
            service_overrides: Default::default(),
            orthographic: false,
            axis_indicator: true,
            selection_occluded: false,
            light_guides: true,
            icon_pack: IconPack::Dark,
            unfocused_fps: UnfocusedFps::DEFAULT,
            font_scale: 1.,
            calibration: rbx_viewer::Calibration::default(),
            auto_recovery: true,
            recovery_minutes: crate::recovery::INTERVAL_DEFAULT,
            script_font_size: SCRIPT_FONT_SIZE,
            viewport_font_size: VIEWPORT_FONT_SIZE,
            large_targets: false,
            reduce_motion: None,
            argon_address: String::new(),
            argon: argon::ArgonSettings::default(),
            // Empty means "whatever the shell's own default is" — the
            // defaults live with the layout in `shell::layout`, and
            // duplicating them here is how the two drift apart.
            docks: SavedLayout::default(),
            named_layouts: Vec::new(),
            output_collapsed: false,
            output_timestamps: false,
            optimize_graph_on_open: false,
            increment_names: true,
            expand_on_select: true,
            dragger: DraggerSettings::default(),
            controls: Controls::default(),
            discord_presence: false,
            discord_hide_names: true,
        }
    }
}

impl Settings {
    /// Reads `$XDG_CONFIG_HOME/rbx-native/settings.json` (or
    /// `~/.config/rbx-native/settings.json`, or `%APPDATA%\rbx-native\
    /// settings.json` on Windows). Never fails: a missing file, a malformed
    /// document, an unreadable path or an undeterminable config directory all
    /// fall back to [`Settings::default`] rather than blocking or aborting
    /// startup.
    pub(crate) fn load() -> Settings {
        match default_settings_path() {
            Some(path) => load_from(&path),
            None => Settings::default(),
        }
    }

    /// Writes the same path `load` reads, creating the directory if needed
    /// and replacing the file atomically. Cheap enough to call on every
    /// change rather than debouncing.
    pub(crate) fn save(&self) -> Result<(), SettingsError> {
        let path = default_settings_path().ok_or(SettingsError::NoConfigDir)?;
        save_to(self, &path)
    }
}

/// Errors saving the settings file. Loading never errors (see
/// [`Settings::load`]); this exists only for the write side, and callers are
/// free to ignore it since a lost preference is not fatal.
#[derive(Debug)]
pub(crate) enum SettingsError {
    NoConfigDir,
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsError::NoConfigDir => {
                write!(
                    f,
                    "could not determine a config directory (no XDG_CONFIG_HOME, \
                     APPDATA, or HOME)"
                )
            }
            SettingsError::CreateDir { path, source } => {
                write!(
                    f,
                    "failed to create config directory {}: {source}",
                    path.display()
                )
            }
            SettingsError::Write { path, source } => {
                write!(
                    f,
                    "failed to write settings file {}: {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for SettingsError {}

fn load_from(path: &Path) -> Settings {
    let Ok(bytes) = std::fs::read(path) else {
        return Settings::default();
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Settings::default();
    };

    let quality = value
        .get("quality")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<QualityLevel>().ok())
        .unwrap_or(QualityLevel::Automatic);
    let show_all_services = value
        .get("show_all_services")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let orthographic = value
        .get("orthographic")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let axis_indicator = value
        .get("axis_indicator")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let icon_pack = value
        .get("icon_pack")
        .and_then(|v| v.as_str())
        .and_then(parse_icon_pack)
        .unwrap_or_default();
    let unfocused_fps = value
        .get("unfocused_fps")
        .and_then(|v| v.as_u64())
        .map(parse_unfocused_fps)
        .unwrap_or(UnfocusedFps::DEFAULT);
    // Clamped rather than rejected: a hand-edited 10.0 should open the
    // editor at 2x, not refuse to read the rest of the file.
    let font_scale = value
        .get("font_scale")
        .and_then(|v| v.as_f64())
        .map(|scale| {
            (scale as f32).clamp(
                crate::tokens::FONT_SCALE_RANGE.0,
                crate::tokens::FONT_SCALE_RANGE.1,
            )
        })
        .unwrap_or(1.);

    Settings {
        quality,
        show_all_services,
        service_overrides: value
            .get("service_overrides")
            .and_then(|v| v.as_object())
            .map(|map| {
                map.iter()
                    .filter_map(|(class, listed)| Some((class.clone(), listed.as_bool()?)))
                    .collect()
            })
            .unwrap_or_default(),
        orthographic,
        axis_indicator,
        selection_occluded: value
            .get("selection_occluded")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        light_guides: value
            .get("light_guides")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        icon_pack,
        unfocused_fps,
        font_scale,
        calibration: read_calibration(&value),
        auto_recovery: value
            .get("auto_recovery")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        recovery_minutes: value
            .get("recovery_minutes")
            .and_then(|v| v.as_u64())
            .map(crate::recovery::clamp_minutes)
            .unwrap_or(crate::recovery::INTERVAL_DEFAULT),
        script_font_size: value
            .get("script_font_size")
            .and_then(|v| v.as_f64())
            .map(|size| clamp_script_font_size(size as f32))
            .unwrap_or(SCRIPT_FONT_SIZE),
        viewport_font_size: value
            .get("viewport_font_size")
            .and_then(|v| v.as_f64())
            .map(|size| clamp_viewport_font_size(size as f32))
            .unwrap_or(VIEWPORT_FONT_SIZE),
        large_targets: value
            .get("large_targets")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        reduce_motion: value.get("reduce_motion").and_then(|v| v.as_bool()),
        docks: read_docks(&value),
        named_layouts: read_named_layouts(&value),
        output_collapsed: value
            .get("output_collapsed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        output_timestamps: value
            .get("output_timestamps")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        optimize_graph_on_open: value
            .get("optimize_graph_on_open")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        increment_names: value
            .get("increment_names")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        expand_on_select: value
            .get("expand_on_select")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        dragger: DraggerSettings::read(&value),
        controls: Controls::read(&value),
        argon_address: value
            .get("argon_address")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .unwrap_or_default(),
        argon: argon::ArgonSettings::read(&value),
        discord_presence: value
            .get("discord_presence")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        discord_hide_names: value
            .get("discord_hide_names")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
    }
}

/// The saved dock layout, or nothing at all when the file predates it or
/// has it in a shape this version cannot read.
///
/// Deliberately forgiving in one direction only: anything malformed is
/// simply left out, and `shell::Layout::restore` puts the missing panels
/// back on their own edges. That is the same contract every other field
/// here has — a hand-edited or future-version settings file must not stop
/// the editor from opening.
fn read_docks(value: &serde_json::Value) -> SavedLayout {
    value.get("docks").map(read_layout).unwrap_or_default()
}

/// One saved layout's object — `docks`, or a named layout's `layout`.
fn read_layout(docks: &serde_json::Value) -> SavedLayout {
    let names = |value: Option<&serde_json::Value>| -> Vec<String> {
        value
            .and_then(serde_json::Value::as_array)
            .map(|names| {
                names
                    .iter()
                    .filter_map(|name| name.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };

    let edges = docks
        .get("edges")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let edge: Edge = crate::shell::edge_from_key(entry.get("edge")?.as_str()?)?;
                    let groups = entry
                        .get("groups")
                        .and_then(serde_json::Value::as_array)
                        .map(|groups| {
                            groups
                                .iter()
                                .map(|group| SavedGroup {
                                    panels: names(group.get("panels")),
                                    active: group
                                        .get("active")
                                        .and_then(serde_json::Value::as_u64)
                                        .unwrap_or(0)
                                        as usize,
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    Some(SavedEdge {
                        edge,
                        groups,
                        // Zero is this file's "nothing was saved" marker,
                        // which is also what a negative, infinite or NaN
                        // size has to become: a hand-edited file must not
                        // be able to collapse a dock to nothing or stretch
                        // it past the window.
                        size: entry
                            .get("size")
                            .and_then(serde_json::Value::as_f64)
                            .map(|size| size as f32)
                            .filter(|size| size.is_finite() && *size > 0.)
                            .unwrap_or(0.),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    SavedLayout {
        edges,
        floating: names(docks.get("floating")),
        closed: names(docks.get("closed")),
    }
}

/// Any value other than exactly 25 falls back to the default preset rather
/// than erroring — a hand-edited or future-version settings file must not
/// stop the editor from opening.
fn parse_unfocused_fps(fps: u64) -> UnfocusedFps {
    if fps == u64::from(UnfocusedFps::Fps25.fps()) {
        UnfocusedFps::Fps25
    } else {
        UnfocusedFps::DEFAULT
    }
}

/// The calibration, each value falling back to its tuned default when
/// missing, not a number, or outside [`calibration_range`].
fn read_calibration(value: &serde_json::Value) -> rbx_viewer::Calibration {
    let tuned = rbx_viewer::Calibration::default();
    let read = |key: &str, default: f32| {
        value
            .get("calibration")
            .and_then(|calibration| calibration.get(key))
            .and_then(serde_json::Value::as_f64)
            .map(|number| number as f32)
            .filter(|number| {
                let (low, high) = calibration_range(default);
                number.is_finite() && (low..=high).contains(number)
            })
            .unwrap_or(default)
    };
    rbx_viewer::Calibration {
        sun_base: read("sun_base", tuned.sun_base),
        atmosphere_density_scale: read("atmosphere_density_scale", tuned.atmosphere_density_scale),
        plastic_spec_strength: read("plastic_spec_strength", tuned.plastic_spec_strength),
    }
}

/// What a calibration value may be: above zero, and within ten times its
/// tuned value either way. These are nudges for a place lit unlike the
/// captures they were tuned against; a value past that is a typo, and the
/// render it gives (a black sun, a white-out fog) is no calibration at all.
pub(crate) fn calibration_range(tuned: f32) -> (f32, f32) {
    (tuned / 10., tuned * 10.)
}

/// [`read_layout`]'s inverse.
fn layout_json(layout: &SavedLayout) -> serde_json::Value {
    serde_json::json!({
        "edges": layout
            .edges
            .iter()
            .map(|edge| {
                serde_json::json!({
                    "edge": crate::shell::edge_key(edge.edge),
                    "size": edge.size,
                    "groups": edge
                        .groups
                        .iter()
                        .map(|group| serde_json::json!({
                            "panels": group.panels,
                            "active": group.active,
                        }))
                        .collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>(),
        "floating": layout.floating,
        "closed": layout.closed,
    })
}

/// The saved named layouts. One without a name, or with a name an earlier
/// entry already has, is left out: names are how they are told apart.
fn read_named_layouts(value: &serde_json::Value) -> Vec<NamedLayout> {
    let mut layouts: Vec<NamedLayout> = Vec::new();
    for entry in value
        .get("named_layouts")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = entry
            .get("name")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        if layouts.iter().any(|named| named.name == name) {
            continue;
        }
        layouts.push(NamedLayout {
            name: name.to_owned(),
            layout: entry.get("layout").map(read_layout).unwrap_or_default(),
        });
    }
    layouts
}

fn save_to(settings: &Settings, path: &Path) -> Result<(), SettingsError> {
    let [camera, snap] = settings.controls.json();
    let value = serde_json::json!({
        "quality": format_quality(settings.quality),
        "show_all_services": settings.show_all_services,
        "service_overrides": settings.service_overrides,
        "orthographic": settings.orthographic,
        "axis_indicator": settings.axis_indicator,
        "selection_occluded": settings.selection_occluded,
        "light_guides": settings.light_guides,
        "icon_pack": format_icon_pack(settings.icon_pack),
        "unfocused_fps": settings.unfocused_fps.fps(),
        "font_scale": settings.font_scale,
        "calibration": {
            "sun_base": settings.calibration.sun_base,
            "atmosphere_density_scale": settings.calibration.atmosphere_density_scale,
            "plastic_spec_strength": settings.calibration.plastic_spec_strength,
        },
        "auto_recovery": settings.auto_recovery,
        "recovery_minutes": settings.recovery_minutes,
        "script_font_size": settings.script_font_size,
        "viewport_font_size": settings.viewport_font_size,
        "large_targets": settings.large_targets,
        "reduce_motion": settings.reduce_motion,
        "docks": layout_json(&settings.docks),
        "named_layouts": settings
            .named_layouts
            .iter()
            .map(|named| serde_json::json!({
                "name": named.name,
                "layout": layout_json(&named.layout),
            }))
            .collect::<Vec<_>>(),
        "output_collapsed": settings.output_collapsed,
        "output_timestamps": settings.output_timestamps,
        "optimize_graph_on_open": settings.optimize_graph_on_open,
        "increment_names": settings.increment_names,
        "expand_on_select": settings.expand_on_select,
        "dragger": settings.dragger.json(),
        "camera": camera,
        "snap": snap,
        "argon_address": settings.argon_address,
        "argon": settings.argon.json(),
        "discord_presence": settings.discord_presence,
        "discord_hide_names": settings.discord_hide_names,
    });
    // A fixed-shape object always serializes; nothing here can fail.
    let bytes = serde_json::to_vec_pretty(&value).expect("settings JSON always serializes");
    write_atomic(path, &bytes)
}

/// Roblox's own `Enum.QualityLevel` spelling, the same format `Shell`'s
/// dropdown labels and command line already parse back with
/// [`QualityLevel::from_str`] — `QualityLevel` has no `Display` impl of its
/// own to reuse.
fn format_quality(quality: QualityLevel) -> String {
    match quality {
        QualityLevel::Automatic => "Automatic".to_string(),
        QualityLevel::Level(level) => format!("Level{level:02}"),
    }
}

fn format_icon_pack(pack: IconPack) -> &'static str {
    match pack {
        IconPack::Dark => "Dark",
        IconPack::Light => "Light",
    }
}

fn parse_icon_pack(s: &str) -> Option<IconPack> {
    match s {
        "Dark" => Some(IconPack::Dark),
        "Light" => Some(IconPack::Light),
        _ => None,
    }
}

fn default_settings_path() -> Option<PathBuf> {
    default_config_dir().map(|dir| dir.join("settings.json"))
}

/// Returns the config directory used for all rbx-native state files
/// (`$XDG_CONFIG_HOME/rbx-native`, `%APPDATA%\rbx-native`, or `~/.config/rbx-native`).
pub(crate) fn default_config_dir() -> Option<PathBuf> {
    if let Some(xdg) = non_empty_env("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg).join("rbx-native"));
    }
    // %APPDATA% is Windows' roaming per-user data dir, the natural match for
    // a small settings file (unlike %LOCALAPPDATA%, used for the asset
    // cache — see rbx_assets::AssetCache). Checked before HOME so a
    // Windows-native launch never depends on HOME being set.
    if let Some(appdata) = non_empty_env("APPDATA") {
        return Some(PathBuf::from(appdata).join("rbx-native"));
    }
    if let Some(home) = non_empty_env("HOME") {
        return Some(PathBuf::from(home).join(".config").join("rbx-native"));
    }
    None
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// Writes via a temp file + rename so a reader never observes a partially
/// written file, and a crash mid-write can't corrupt an existing one.
/// Used for all persisted state files (settings, dock layout, etc).
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), SettingsError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|source| SettingsError::CreateDir {
        path: parent.to_path_buf(),
        source,
    })?;

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp_path = parent.join(format!("{file_name}.tmp-{}", std::process::id()));

    std::fs::write(&tmp_path, bytes).map_err(|source| SettingsError::Write {
        path: tmp_path.clone(),
        source,
    })?;
    std::fs::rename(&tmp_path, path).map_err(|source| SettingsError::Write {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    /// Guards every test below that mutates `XDG_CONFIG_HOME`/`APPDATA`: env
    /// vars are process-global, and `cargo test` runs tests on separate
    /// threads of the same process, so two such tests running concurrently
    /// would each observe the other's value.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// A settings.json path under a fresh, never-created temp directory: each
    /// test gets its own, so tests never share (or need to clean up) state,
    /// and the real `$XDG_CONFIG_HOME`/`HOME` are never touched.
    fn temp_settings_path() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir()
            .join(format!(
                "rbx_studio_settings_test_{}_{n}",
                std::process::id()
            ))
            .join("settings.json")
    }

    #[test]
    fn round_trips_through_disk() {
        let path = temp_settings_path();
        let settings = Settings {
            quality: QualityLevel::Level(7),
            show_all_services: true,
            orthographic: true,
            axis_indicator: false,
            selection_occluded: true,
            light_guides: false,
            icon_pack: IconPack::Light,
            unfocused_fps: UnfocusedFps::Fps25,
            font_scale: 1.25,
            argon_address: "argon.example.com:9000".to_owned(),
            ..Settings::default()
        };
        save_to(&settings, &path).unwrap();
        assert_eq!(load_from(&path), settings);
    }

    #[test]
    fn a_settings_file_from_before_the_argon_address_existed_reads_as_empty() {
        let path = temp_settings_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, br#"{"quality": "Automatic"}"#).unwrap();

        // Empty is what `Shell` reads as "nothing saved yet, fall back to
        // the hardcoded localhost:8000 default" — see `shell.rs`'s own
        // `argon_address` field init.
        assert_eq!(load_from(&path).argon_address, "");
    }

    #[test]
    fn a_settings_file_from_before_the_axis_indicator_toggle_existed_defaults_it_on() {
        let path = temp_settings_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            br#"{"quality": "Automatic", "show_all_services": false}"#,
        )
        .unwrap();

        assert!(load_from(&path).axis_indicator);
    }

    /// The selection box shows through geometry unless someone has asked
    /// otherwise, so a file written before the toggle existed — and one
    /// written after it, with the toggle never touched — both read as off.
    #[test]
    fn a_settings_file_with_no_selection_occlusion_recorded_defaults_it_off() {
        let path = temp_settings_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, br#"{"quality": "Automatic"}"#).unwrap();
        assert!(!load_from(&path).selection_occluded);

        // And an explicit `true` survives the round trip, or the preference
        // would be unsettable rather than merely defaulted.
        std::fs::write(&path, br#"{"selection_occluded": true}"#).unwrap();
        assert!(load_from(&path).selection_occluded);
    }

    #[test]
    fn a_settings_file_with_no_light_guides_recorded_shows_them() {
        let path = temp_settings_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, br#"{"quality": "Automatic"}"#).unwrap();
        assert!(load_from(&path).light_guides);

        std::fs::write(&path, br#"{"light_guides": false}"#).unwrap();
        assert!(!load_from(&path).light_guides);
    }

    #[test]
    fn unfocused_fps_falls_back_to_default_for_anything_but_25() {
        let path = temp_settings_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, br#"{"unfocused_fps": 30}"#).unwrap();
        assert_eq!(load_from(&path).unfocused_fps, UnfocusedFps::Fps30);
        std::fs::write(&path, br#"{"unfocused_fps": 999}"#).unwrap();
        assert_eq!(load_from(&path).unfocused_fps, UnfocusedFps::DEFAULT);
    }

    #[test]
    fn missing_file_falls_back_to_defaults() {
        let path = temp_settings_path();
        assert_eq!(load_from(&path), Settings::default());
    }

    #[test]
    fn malformed_json_falls_back_to_defaults_instead_of_crashing() {
        let path = temp_settings_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not json at all {{{").unwrap();
        assert_eq!(load_from(&path), Settings::default());
    }

    #[test]
    fn unreadable_path_falls_back_to_defaults() {
        // A directory where a file is expected can never be `fs::read`.
        let path = temp_settings_path();
        std::fs::create_dir_all(&path).unwrap();
        assert_eq!(load_from(&path), Settings::default());
    }

    #[test]
    fn save_creates_missing_parent_directories() {
        let path = temp_settings_path()
            .parent()
            .unwrap()
            .join("nested")
            .join("deep")
            .join("settings.json");
        let settings = Settings::default();
        save_to(&settings, &path).unwrap();
        assert!(path.is_file());
        assert_eq!(load_from(&path), settings);
    }

    #[test]
    fn default_dir_honors_xdg_config_home_over_the_home_fallback() {
        // SAFETY: env var mutation is process-global; guarded by ENV_LOCK so
        // this can't interleave with the other tests below that also mutate
        // XDG_CONFIG_HOME/APPDATA/HOME — scope the check to the
        // pure-computation helper rather than asserting on the process-wide
        // environment anywhere else.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_settings_path().parent().unwrap().to_path_buf();
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        let result = default_config_dir();
        std::env::remove_var("XDG_CONFIG_HOME");
        assert_eq!(result, Some(dir.join("rbx-native")));
    }

    #[test]
    fn default_dir_falls_back_to_appdata_on_windows() {
        // SAFETY: see default_dir_honors_xdg_config_home_over_the_home_fallback.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_settings_path().parent().unwrap().to_path_buf();
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::set_var("APPDATA", &dir);
        let result = default_config_dir();
        std::env::remove_var("APPDATA");
        assert_eq!(result, Some(dir.join("rbx-native")));
    }

    #[test]
    fn xdg_config_home_wins_over_appdata() {
        // SAFETY: see default_dir_honors_xdg_config_home_over_the_home_fallback.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let xdg = temp_settings_path().parent().unwrap().to_path_buf();
        let appdata = temp_settings_path().parent().unwrap().to_path_buf();
        std::env::set_var("XDG_CONFIG_HOME", &xdg);
        std::env::set_var("APPDATA", &appdata);
        let result = default_config_dir();
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::remove_var("APPDATA");
        assert_eq!(result, Some(xdg.join("rbx-native")));
    }

    #[test]
    fn quality_string_form_round_trips_every_level_and_automatic() {
        assert_eq!(
            format_quality(QualityLevel::Automatic).parse::<QualityLevel>(),
            Ok(QualityLevel::Automatic)
        );
        for level in QualityLevel::MIN..=QualityLevel::MAX {
            let mode = QualityLevel::Level(level);
            assert_eq!(format_quality(mode).parse::<QualityLevel>(), Ok(mode));
        }
    }

    #[test]
    fn icon_pack_string_form_round_trips_both_variants() {
        for pack in [IconPack::Dark, IconPack::Light] {
            assert_eq!(parse_icon_pack(format_icon_pack(pack)), Some(pack));
        }
    }

    #[test]
    fn an_unrecognized_icon_pack_string_falls_back_to_default_on_load() {
        let path = temp_settings_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, br#"{"icon_pack": "Sepia"}"#).unwrap();
        assert_eq!(load_from(&path).icon_pack, IconPack::Dark);
    }

    /// The UI scale is the app's answer to WCAG 1.4.4, so a settings file
    /// someone has hand-edited to something absurd must still open the
    /// editor — at the nearest usable scale, not at 10x and not at the
    /// default that silently discards what they asked for.
    #[test]
    fn a_font_scale_outside_the_supported_range_is_clamped_rather_than_dropped() {
        let path = temp_settings_path();
        std::fs::create_dir_all(path.parent().expect("settings path has a parent"))
            .expect("create temp dir");
        std::fs::write(&path, br#"{"font_scale": 10.0, "show_all_services": true}"#)
            .expect("write settings");

        let settings = load_from(&path);
        assert_eq!(settings.font_scale, crate::tokens::FONT_SCALE_RANGE.1);
        assert!(
            settings.show_all_services,
            "an out-of-range scale must not stop the rest of the file being read"
        );
    }

    #[test]
    fn a_font_scale_round_trips_through_save_and_load() {
        let path = temp_settings_path();
        let settings = Settings {
            font_scale: 1.5,
            ..Settings::default()
        };

        save_to(&settings, &path).expect("save settings");
        assert_eq!(load_from(&path).font_scale, 1.5);
    }

    #[test]
    fn calibration_round_trips_and_a_value_out_of_range_falls_back_alone() {
        let path = temp_settings_path();
        let calibration = rbx_viewer::Calibration {
            sun_base: 0.6,
            atmosphere_density_scale: 0.002,
            plastic_spec_strength: 0.2,
        };
        let settings = Settings {
            calibration,
            ..Settings::default()
        };
        save_to(&settings, &path).expect("save settings");
        assert_eq!(load_from(&path).calibration, calibration);

        std::fs::write(
            &path,
            br#"{"calibration": {"sun_base": 50, "plastic_spec_strength": 0.3}}"#,
        )
        .expect("write settings");
        let tuned = rbx_viewer::Calibration::default();
        let loaded = load_from(&path).calibration;
        assert_eq!(loaded.sun_base, tuned.sun_base);
        assert_eq!(
            loaded.atmosphere_density_scale,
            tuned.atmosphere_density_scale
        );
        assert_eq!(loaded.plastic_spec_strength, 0.3);
    }

    #[test]
    fn auto_recovery_round_trips_and_defaults_on_every_four_minutes() {
        let path = temp_settings_path();
        let settings = Settings {
            auto_recovery: false,
            recovery_minutes: 9,
            ..Settings::default()
        };
        save_to(&settings, &path).expect("save settings");
        let loaded = load_from(&path);
        assert!(!loaded.auto_recovery);
        assert_eq!(loaded.recovery_minutes, 9);

        std::fs::write(&path, br#"{"recovery_minutes": 60}"#).expect("write settings");
        let loaded = load_from(&path);
        assert!(loaded.auto_recovery);
        assert_eq!(loaded.recovery_minutes, 10);

        std::fs::write(&path, b"{}").expect("write settings");
        assert_eq!(
            load_from(&path).recovery_minutes,
            crate::recovery::INTERVAL_DEFAULT
        );
    }

    #[test]
    fn a_script_font_size_round_trips_and_a_missing_one_is_the_theme_size() {
        let path = temp_settings_path();
        let settings = Settings {
            script_font_size: 18.,
            ..Settings::default()
        };
        save_to(&settings, &path).expect("save settings");
        assert_eq!(load_from(&path).script_font_size, 18.);

        std::fs::write(&path, br#"{"show_all_services": true}"#).expect("write settings");
        assert_eq!(load_from(&path).script_font_size, SCRIPT_FONT_SIZE);
    }

    #[test]
    fn a_script_font_size_outside_the_range_is_clamped() {
        let path = temp_settings_path();
        std::fs::create_dir_all(path.parent().expect("settings path has a parent"))
            .expect("create temp dir");
        std::fs::write(&path, br#"{"script_font_size": 100}"#).expect("write settings");
        assert_eq!(load_from(&path).script_font_size, SCRIPT_FONT_SIZE_RANGE.1);

        assert_eq!(clamp_script_font_size(2.), SCRIPT_FONT_SIZE_RANGE.0);
        assert_eq!(clamp_script_font_size(f32::NAN), SCRIPT_FONT_SIZE);
        assert_eq!(clamp_script_font_size(16.), 16.);
    }

    #[test]
    fn a_viewport_font_size_round_trips_clamps_and_defaults_to_the_readout_size() {
        let path = temp_settings_path();
        let settings = Settings {
            viewport_font_size: 18.,
            ..Settings::default()
        };
        save_to(&settings, &path).expect("save settings");
        assert_eq!(load_from(&path).viewport_font_size, 18.);
        // Its own key, not the script editor's.
        assert_eq!(load_from(&path).script_font_size, SCRIPT_FONT_SIZE);

        std::fs::write(&path, br#"{"show_all_services": true}"#).expect("write settings");
        assert_eq!(load_from(&path).viewport_font_size, VIEWPORT_FONT_SIZE);

        std::fs::write(&path, br#"{"viewport_font_size": 100}"#).expect("write settings");
        assert_eq!(
            load_from(&path).viewport_font_size,
            VIEWPORT_FONT_SIZE_RANGE.1
        );
        assert_eq!(clamp_viewport_font_size(2.), VIEWPORT_FONT_SIZE_RANGE.0);
        assert_eq!(clamp_viewport_font_size(f32::INFINITY), VIEWPORT_FONT_SIZE);
    }

    #[test]
    fn service_overrides_round_trip_and_a_non_boolean_is_dropped() {
        let path = temp_settings_path();
        let mut overrides = crate::explorer::ServiceOverrides::new();
        overrides.insert("HttpService".into(), true);
        overrides.insert("Teams".into(), false);
        let settings = Settings {
            service_overrides: overrides.clone(),
            ..Settings::default()
        };
        save_to(&settings, &path).expect("save settings");
        assert_eq!(load_from(&path).service_overrides, overrides);

        std::fs::write(
            &path,
            br#"{"service_overrides": {"Teams": false, "Chat": "yes"}}"#,
        )
        .expect("write settings");
        let loaded = load_from(&path).service_overrides;
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.get("Teams"), Some(&false));
    }

    /// The dock layout is the one preference a user can wreck by accident
    /// — a column dragged to four pixels wide saves that way — so "Reset
    /// Layout" exists, and a saved layout has to actually come back.
    #[test]
    fn a_dock_layout_round_trips_and_a_missing_one_falls_back() {
        let path = temp_settings_path();
        let settings = Settings {
            docks: SavedLayout {
                edges: vec![
                    SavedEdge {
                        edge: Edge::Left,
                        groups: vec![SavedGroup {
                            panels: vec!["Explorer".to_owned(), "Output".to_owned()],
                            active: 1,
                        }],
                        size: 412.5,
                    },
                    SavedEdge {
                        edge: Edge::Right,
                        groups: Vec::new(),
                        size: 260.,
                    },
                ],
                floating: vec!["Properties".to_owned()],
                closed: Vec::new(),
            },
            output_collapsed: true,
            output_timestamps: true,
            large_targets: true,
            reduce_motion: Some(true),
            ..Settings::default()
        };

        save_to(&settings, &path).expect("save settings");
        let read = load_from(&path);
        assert_eq!(read.docks, settings.docks, "which panel sits where");
        assert!(read.output_collapsed);
        assert!(read.output_timestamps);
        assert!(read.large_targets);
        assert_eq!(read.reduce_motion, Some(true));

        // Nothing saved reads as no edges at all, which is the shell's cue
        // to use its own default layout rather than opening with no docks.
        let empty = temp_settings_path();
        std::fs::create_dir_all(empty.parent().expect("a parent")).expect("create dir");
        std::fs::write(&empty, b"{}").expect("write settings");
        let read = load_from(&empty);
        assert_eq!(read.docks, SavedLayout::default());
        assert_eq!(
            read.reduce_motion, None,
            "no recorded choice means follow the desktop, not 'off'"
        );
    }

    #[test]
    fn named_layouts_round_trip_and_a_nameless_or_repeated_one_is_dropped() {
        let path = temp_settings_path();
        let layout = SavedLayout {
            edges: vec![SavedEdge {
                edge: Edge::Left,
                groups: vec![SavedGroup {
                    panels: vec!["Explorer".into()],
                    active: 0,
                }],
                size: 320.,
            }],
            floating: vec!["Output".into()],
            closed: Vec::new(),
        };
        let settings = Settings {
            named_layouts: vec![NamedLayout {
                name: "Scripting".into(),
                layout: layout.clone(),
            }],
            ..Settings::default()
        };
        save_to(&settings, &path).expect("save settings");
        assert_eq!(load_from(&path).named_layouts, settings.named_layouts);

        std::fs::write(
            &path,
            br#"{"named_layouts": [
                {"name": "  ", "layout": {}},
                {"name": "Build", "layout": {}},
                {"name": "Build", "layout": {"closed": ["Output"]}},
                {"layout": {}}
            ]}"#,
        )
        .expect("write settings");
        let loaded = load_from(&path).named_layouts;
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Build");
        assert!(loaded[0].layout.closed.is_empty());
    }

    /// A hand-edited file must not be able to collapse the editor.
    #[test]
    fn a_nonsense_dock_size_falls_back_instead_of_being_used() {
        let path = temp_settings_path();
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("create dir");
        std::fs::write(
            &path,
            br#"{"docks": {"edges": [
                {"edge": "left", "groups": [{"panels": ["Properties"]}], "size": -50},
                {"edge": "right", "groups": [{"panels": ["Explorer"]}], "size": 1e39},
                {"edge": "nowhere", "groups": [], "size": 300},
                {"groups": [], "size": 300}
            ]}}"#,
        )
        .expect("write settings");

        let read = load_from(&path);
        assert_eq!(
            read.docks.edges.len(),
            2,
            "an edge with no name is not an edge"
        );
        assert_eq!(
            read.docks.edges[0].size, 0.,
            "a negative width is not a width"
        );
        // 1e39 is a fine f64 and an infinite f32, which is the only way
        // an overflow reaches this: a literal too big for f64 stops
        // `serde_json` parsing the document at all.
        assert_eq!(read.docks.edges[1].size, 0., "nor is an overflow");
    }

    #[test]
    fn discord_presence_round_trips_and_defaults_off_with_names_hidden() {
        let path = temp_settings_path();
        let settings = Settings {
            discord_presence: true,
            discord_hide_names: false,
            ..Settings::default()
        };
        save_to(&settings, &path).unwrap();
        let read = load_from(&path);
        assert!(read.discord_presence);
        assert!(!read.discord_hide_names);

        // A file from before these fields existed defaults them conservatively.
        let path = temp_settings_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, br#"{"quality": "Automatic"}"#).unwrap();
        let read = load_from(&path);
        assert!(!read.discord_presence);
        assert!(read.discord_hide_names);
    }
}
