//! Desktop shell of the editor: the wgpu viewer of `rbx_viewer` drawn inside a
//! GPUI Kit window, next to the Explorer of the place, mutable through the
//! Command Bar (see `command_bar`).
//!
//! One place per process; the command line it takes lives in [`cli`], and
//! `--help` prints it. `--select` and `--run` are the supported spelling of
//! the first two variables below — same behaviour, the flag winning when both
//! are given. The rest stay variables: they are aids for scripted
//! screenshots, not a surface worth committing to.
//!
//! `RBX_STUDIO_SELECT=<target>[,<target>...]` pre-selects the first instance
//! that target names — an Explorer path (`Workspace.Model.Part`) or a bare
//! name, see `explorer::resolve` — at startup, then adds each
//! further comma-separated target to the selection exactly as a
//! `Shift`/`Ctrl`/`Cmd`-click would — a debugging aid for scripted
//! screenshots of the Properties panel and the viewport's multi-selection
//! outline/gizmo, since nothing else can click the tree or the viewport on
//! the editor's behalf; re-applied after `RBX_STUDIO_RUN` too, so a
//! screenshot can show what a script just created without a click.
//! `RBX_STUDIO_RUN=<source>` runs one Luau chunk against the place right
//! after the window opens, exactly as typing it into the Command Bar and
//! pressing Enter would — the same aid, for the Command Bar itself.
//! `RBX_STUDIO_TOOL=move[,local][,nosnap]` picks a transform tool (and its
//! world/local orientation, and whether the move/scale snap starts switched
//! on) at startup, the same aid for the transform toolbar and its viewport
//! draggers (see `shell::toolbar`). `RBX_STUDIO_DRAG=<dx>,<dy>,<dz>` moves the
//! selection by that offset through the same path a real gizmo or cursor drag
//! ends with, the same aid for a group drag; `RBX_STUDIO_RESIZE=<dx>,<dy>,<dz>`
//! grows the selected part by that much along its own axes through the path
//! a Scale drag ends with, the same aid for the Scale tool (see
//! `shell::drag::debug` for both). `RBX_STUDIO_SCROLL=<x>,<y>,<dx>,<dy>`
//! rolls the wheel once over viewport pixel `(x, y)` by that many notches
//! once the first frame is up, the same aid for scrolling a `ScrollingFrame`
//! drawn in the viewport (see `workspace_view::scroll`).
//! `RBX_STUDIO_OPEN_SCRIPT=<name>[,<name>...]` opens each named
//! `Script`/`LocalScript`/`ModuleScript` in the Script Editor panel exactly as
//! double-clicking its Explorer row would — the same aid, for the script
//! editor and its Luau highlighting (see `shell::scripts`).
//! `RBX_STUDIO_GROUP=1` wraps the current selection in a new `Model` exactly
//! as `Ctrl+G` would; `RBX_STUDIO_UNGROUP=1` unwraps it back out exactly as
//! `Ctrl+Shift+G` would — the same aid, for the Explorer's Group/Ungroup
//! (see `shell::group`).
//! Ctrl+S writes the place back to the file it was opened from, in the
//! format it was opened in; `RBX_STUDIO_SAVE_AS=<path>` redirects one such
//! save to a scratch path instead (see `save`).

mod align;
mod camera;
mod class_icons;
mod cli;
mod command_bar;
mod display;
mod explorer;
mod folder_colors;
mod history;
mod menu_bar;
mod pacing;
mod packs;
mod pointer_lock;
mod properties;
mod render_image;
mod save;
mod scale;
mod script_editor;
mod script_templates;
mod settings;
mod settle;
mod shell;
mod style_editor;
mod tokens;
mod transform;
mod workspace_view;

use std::path::{Path, PathBuf};

use gpui_kit::component::{Root, Theme, ThemeMode, ThemeRegistry};
use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::Headless;

use camera::PlaceCamera;
use class_icons::IconPack;
use cli::Launch;
use explorer::Explorer;
use folder_colors::FolderColors;
use properties::Properties;
use save::Format;
use settings::Settings;
use shell::Shell;

/// The fallback for `--select`, resolved into [`Launch`] below and read
/// nowhere else — `shell` takes the whole comma list from there rather than
/// reaching back into the environment for it.
const SELECT_VARIABLE: &str = "RBX_STUDIO_SELECT";

const WINDOW_SIZE: (f32, f32) = (1600.0, 900.0);

fn main() {
    // Loaded once, up front: the CLI's `--quality` still wins when given (see
    // `parse`), and the Explorer's initial visibility set comes straight from
    // here since nothing on the command line can express it.
    let settings = Settings::load();
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let cli::Arguments {
        path,
        quality,
        select,
        run,
        verbose,
    } = match cli::parse(&arguments, settings.quality) {
        Ok(cli::Parsed::Open(parsed)) => parsed,
        Ok(cli::Parsed::Usage) => {
            println!("{}", cli::USAGE);
            return;
        }
        Err(message) => {
            eprintln!("{message}\n{}", cli::USAGE);
            std::process::exit(2);
        }
    };
    // The CLI's own `--quality` wins over the persisted one from here on;
    // `Shell::new` takes the whole struct rather than one parameter per
    // field, so this is the one place that resolved value has to land.
    let settings = Settings {
        quality,
        ..settings
    };

    // A flag wins over the variable it supersedes: a wrapper script that
    // exports one and then passes the other meant the one it spelled out on
    // the line. `--run`'s file is read here, before anything expensive, so a
    // typo in its path is an exit code rather than a line in the Output dock
    // on the first frame.
    let launch = Launch {
        select: select.or_else(|| std::env::var(SELECT_VARIABLE).ok()),
        run: match run {
            Some(script) => Some(read_script(&script)),
            None => std::env::var(command_bar::RUN_VARIABLE).ok(),
        },
        verbose,
    };

    // Before `load`: the Explorer's rows resolve their icons while the place
    // is built, so an installed pack has to be in place by then.
    if let Some(pack) = packs::Appearance::load().icon_pack {
        class_icons::set_user_pack(packs::IconOverlay::load(&pack));
    }

    // Parsing and the asset downloads both block; running them before the
    // window exists keeps the UI thread from ever stalling on the network.
    println!("loading {}…", path.display());
    let place = match load(&path, &launch, settings.icon_pack) {
        Ok(place) => place,
        Err(message) => {
            eprintln!("rbxstudio: {message}");
            std::process::exit(1);
        }
    };
    let title = SharedString::from(file_name(&path));

    // The full Lucide catalog: the menu bar's icons are well outside the
    // default bundle the components themselves use. The Explorer's own class
    // icons are rasterized straight from `class_icons`'s embedded SVGs, not
    // painted through this asset source at all — see its own doc comment.
    let app = gpui_kit::application().with_assets(gpui_kit::assets::AllAssets);
    app.run(move |cx| {
        gpui_kit::init(cx);
        install_theme(cx);
        scale::install(cx);
        shell::install_key_bindings(cx);
        menu_bar::install_key_bindings(cx);
        Theme::change(ThemeMode::Dark, None, cx);

        cx.spawn(async move |cx| {
            let options = cx.update(|cx| window_options(&title, cx));
            cx.open_window(options, |window, cx| {
                let shell = cx.new(|cx| Shell::new(title, place, settings, launch, window, cx));
                cx.new(|cx| Root::new(shell, window, cx))
            })
            .expect("failed to open the main window");
        })
        .detach();
    });
}

/// A place file, read before the window exists: the tree the Explorer lists,
/// the properties behind it, the viewpoint it was saved at, and the viewer that
/// draws it.
struct Place {
    explorer: Explorer,
    properties: Properties,
    /// The instance `--select` named, when it exists in the file.
    selected: Option<Ref>,
    camera: Option<PlaceCamera>,
    viewer: Headless,
    /// The canonical, mutable tree: what the Command Bar's `rbx_lua::Runtime`
    /// takes ownership of and hands back, what the Explorer and viewport are
    /// rebuilt from afterwards, and what `properties` reads on every render.
    dom: WeakDom,
    /// Cloned once per run into a fresh `rbx_lua::Runtime`, which takes
    /// ownership of its copy.
    database: ReflectionDatabase,
    /// Where Ctrl+S writes back to, and in which format — see `save`.
    /// `rbx_viewer::read_place` sniffs the same bytes internally but does not
    /// expose its choice, so `load` sniffs them a second time here rather
    /// than widening that crate's public surface for this.
    path: PathBuf,
    format: Format,
    /// This place's `Folder` colour tags — see `folder_colors`. Loaded here
    /// (and pruned of any entry whose folder no longer resolves) rather than
    /// lazily on first use, since the Explorer's own tints are baked in by
    /// `shell::folder_color::folder_tints` from the moment the window opens.
    folder_colors: FolderColors,
}

/// Swaps GPUI Kit's stock near-black dark theme for this editor's own
/// lower-contrast palette (`assets/themes/dark-soft.json`), before
/// [`Theme::change`] below activates it. The file follows GPUI Kit's own
/// `ThemeSet`/`ThemeConfig` JSON format (any key this leaves unset falls
/// back to the stock dark theme). A user's own theme file in the same shape
/// (see [`install_user_theme`]) replaces it afterwards.
fn install_theme(cx: &mut App) {
    const THEME: &str = include_str!("../../../assets/themes/dark-soft.json");
    ThemeRegistry::global_mut(cx)
        .load_themes_from_str(THEME)
        .expect("assets/themes/dark-soft.json is valid ThemeSet JSON");
    if let Some(theme) = ThemeRegistry::global(cx)
        .themes()
        .get("rbx-native Dark")
        .cloned()
    {
        Theme::global_mut(cx).dark_theme = theme;
    }
    install_user_theme(cx);
    install_fonts(cx);
}

/// Applies the theme `appearance.json` names, from `<config>/themes/`: the
/// first dark theme its `ThemeSet` file defines under a name the registry
/// does not already hold — the registry ignores a duplicate name rather than
/// replacing it, so a file that reuses a built-in's name would otherwise
/// change nothing and say nothing. Anything wrong with the file is reported
/// on stderr and leaves the built-in theme in place.
///
/// Only the toolkit's widgets follow it (see `packs`); the chrome this
/// editor draws itself still reads `tokens`.
fn install_user_theme(cx: &mut App) {
    let Some(name) = packs::Appearance::load().theme else {
        return;
    };
    let Some(json) = packs::theme_json(&name) else {
        eprintln!("rbxstudio: theme {name:?} could not be read from the themes folder");
        return;
    };
    let known: std::collections::HashSet<SharedString> =
        ThemeRegistry::global(cx).themes().keys().cloned().collect();
    if let Err(err) = ThemeRegistry::global_mut(cx).load_themes_from_str(&json) {
        eprintln!("rbxstudio: theme {name:?} is not a valid theme file: {err}");
        return;
    }
    let picked = ThemeRegistry::global(cx)
        .sorted_themes()
        .into_iter()
        .find(|theme| !known.contains(&theme.name) && theme.mode == ThemeMode::Dark)
        .cloned();
    match picked {
        Some(theme) => Theme::global_mut(cx).dark_theme = theme,
        None => eprintln!(
            "rbxstudio: theme {name:?} defines no dark theme with a name of its own; \
             keeping the built-in"
        ),
    }
}

/// Points the theme at the design system's own font stack, for whichever of
/// its families this machine actually has.
///
/// The theme takes one family name, not a CSS-style stack with fallbacks,
/// and a name that isn't installed is used as-is rather than falling
/// through — so the fallback has to happen here, by asking the text system
/// what exists before naming anything. A machine with neither family keeps
/// the platform UI font, which is the right answer and not a failure.
fn install_fonts(cx: &mut App) {
    let installed = cx.text_system().all_font_names();
    let has = |family: &str| installed.iter().any(|name| name == family);

    let theme = Theme::global_mut(cx);
    if has(tokens::FONT_FAMILY_UI) {
        theme.font_family = tokens::FONT_FAMILY_UI.into();
    }
    if has(tokens::FONT_FAMILY_MONO) {
        theme.mono_font_family = tokens::FONT_FAMILY_MONO.into();
    }
}

fn load(path: &Path, launch: &Launch, icon_pack: IconPack) -> Result<Place, String> {
    let bytes = std::fs::read(path).map_err(|err| format!("failed to read {path:?}: {err}"))?;
    let format = Format::sniff(&bytes);
    launch.say(format!(
        "read {} ({} bytes, {} place)",
        path.display(),
        bytes.len(),
        match format {
            Format::Binary => "binary",
            Format::Xml => "XML",
        }
    ));
    let dom = rbx_viewer::read_place(path)?;
    let database = ReflectionDatabase::embedded();

    let mut folder_colors = FolderColors::load();
    if folder_colors.prune(path, &dom) {
        let _ = folder_colors.save();
    }

    Ok(Place {
        explorer: Explorer::from_dom(&dom, icon_pack, &folder_colors, path),
        selected: launch
            .select
            .as_deref()
            .and_then(|target| explorer::resolve(&dom, target)),
        camera: PlaceCamera::from_dom(&dom),
        viewer: Headless::load(path, true)?,
        properties: Properties::new(database.clone()),
        dom,
        database,
        path: path.to_path_buf(),
        format,
        folder_colors,
    })
}

fn window_options(title: &SharedString, cx: &App) -> WindowOptions {
    let bounds = Bounds::centered(None, size(px(WINDOW_SIZE.0), px(WINDOW_SIZE.1)), cx);

    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        // The editor draws its own title bar (see `shell::chrome::topbar`),
        // so the window manager is asked not to. The title itself still
        // goes through `TitlebarOptions`: that is what names the window in
        // a taskbar, an alt-tab switcher and a screenshot tool, none of
        // which can read the row this app paints for itself.
        titlebar: Some(TitlebarOptions {
            title: Some(title.clone()),
            appears_transparent: true,
            ..Default::default()
        }),
        window_decorations: Some(WindowDecorations::Client),
        ..Default::default()
    }
}

/// `--run`'s script, read before the window and the GPU exist so that a path
/// typo stops the process outright instead of surfacing as an Output-dock
/// error on the first frame.
fn read_script(path: &Path) -> String {
    match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(err) => {
            eprintln!("rbxstudio: failed to read {}: {err}", path.display());
            std::process::exit(1);
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::file_name;
    use gpui_kit::component::{ThemeMode, ThemeSet};
    use std::path::Path;

    /// Guards `install_theme`'s `include_str!` + `.expect(...)`: a change to
    /// `assets/themes/dark-soft.json` that breaks its `ThemeSet` shape (a
    /// typo in a key, invalid JSON) would otherwise only surface as a panic
    /// the first time `rbxstudio` actually starts.
    #[test]
    fn the_bundled_theme_file_parses_as_a_dark_theme_named_for_this_project() {
        const THEME: &str = include_str!("../../../assets/themes/dark-soft.json");
        let theme_set: ThemeSet =
            serde_json::from_str(THEME).expect("assets/themes/dark-soft.json is valid JSON");
        let theme = theme_set
            .themes
            .iter()
            .find(|theme| theme.name == "rbx-native Dark")
            .expect("a theme named \"rbx-native Dark\"");
        assert_eq!(theme.mode, ThemeMode::Dark);
    }

    #[test]
    fn the_window_is_titled_after_the_file() {
        assert_eq!(file_name(Path::new("/places/Demo.rbxl")), "Demo.rbxl");
        assert_eq!(file_name(Path::new("Demo.rbxl")), "Demo.rbxl");
    }
}
