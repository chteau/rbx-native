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
//! `RBX_STUDIO_UI_EDITOR=1|<width>x<height>` opens the UI Editor on its
//! canvas at startup, at that simulated resolution when one is given — the
//! same aid, for the canvas (see `shell::ui_editor`).
//! `RBX_STUDIO_GROUP=1` wraps the current selection in a new `Model` exactly
//! as `Ctrl+G` would; `RBX_STUDIO_UNGROUP=1` unwraps it back out exactly as
//! `Ctrl+Shift+G` would — the same aid, for the Explorer's Group/Ungroup
//! (see `shell::group`).
//! Ctrl+S writes the place back to the file it was opened from, in the
//! format it was opened in; `RBX_STUDIO_SAVE_AS=<path>` redirects one such
//! save to a scratch path instead (see `save`).
//! `RBX_STUDIO_ARGON_CONNECT=1|<host>:<port>` clicks Connect on the Script
//! Editor's Argon dock at startup — the same aid, for a real connection to
//! a running `argon serve` (see `shell::argon_sync`); the address form
//! overrides the dock's own address field first.
//! `RBX_STUDIO_WALLY_INSTALL=<scope>/<name>` resolves and installs that
//! Wally package at startup exactly as clicking its search result row
//! would — the same aid, for the Wally dock (see `shell::wally_sync`).
//! `RBX_STUDIO_ARGON_DIFF=1` opens the Argon review prompt's Diff window on
//! a small synthetic batch at startup — the same aid, for a window that
//! otherwise only opens once a live `argon serve` session pushes five or
//! more real changes at once (see `shell::argon_sync`).

mod align;
mod argon_client;
mod camera;
mod change_class;
mod class_icons;
mod cli;
mod command_bar;
mod display;
mod dragger;
mod explorer;
mod folder_colors;
mod history;
mod home;
mod key_store;
mod launcher;
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
mod sequence_editor;
mod sequence_window;
mod settings;
mod settle;
mod shell;
mod style_editor;
mod sun;
mod tokens;
mod transform;
mod ui_canvas;
mod wally_client;
mod workspace_view;

use std::borrow::Cow;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::highlighter::HighlightTheme;
use gpui_kit::component::{Root, Theme, ThemeConfig, ThemeMode, ThemeRegistry, ThemeSet};
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
        setup,
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

    // Everything read from the config directory beyond `settings.json`, in one
    // pass before the window exists. The icon pack goes in before `load`: the
    // Explorer's rows resolve their icons while the place is built.
    let mut user = packs::UserContent::load();
    class_icons::set_user_pack(user.icon_overlay.take());
    let theme = user.appearance.theme.clone();

    // The full Lucide catalog: the menu bar's icons are well outside the
    // default bundle the components themselves use. The Explorer's own class
    // icons are rasterized straight from `class_icons`'s embedded SVGs, not
    // painted through this asset source at all — see its own doc comment.
    let app = gpui_kit::application().with_assets(gpui_kit::assets::AllAssets);
    app.run(move |cx| {
        gpui_kit::init(cx);
        install_theme(theme.as_deref(), cx);
        scale::install(cx);
        shell::install_key_bindings(cx);
        menu_bar::install_key_bindings(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        install_dark_highlight(cx);

        cx.spawn(async move |cx| {
            // The stored key has to be in `rbx_cloud` before `load`: the
            // place's asset downloads authenticate with it.
            let has_key = key_store::restore(cx).await;
            let boot = EditorBoot {
                settings,
                launch,
                user,
            };
            cx.update(|cx| match home::route(path, has_key && !setup) {
                home::Route::Editor(path) => {
                    if let Err(failed) = open_editor(&path, boot, cx) {
                        let (message, _) = *failed;
                        eprintln!("rbxstudio: {message}");
                        std::process::exit(1);
                    }
                }
                home::Route::Wizard => launcher::open_wizard(Rc::new(RefCell::new(Some(boot))), cx),
                home::Route::Home => launcher::open_home(Rc::new(RefCell::new(Some(boot))), cx),
            });
        })
        .detach();
    });
}

/// What the editor window needs beyond its place: the resolved settings,
/// the scripted-launch aids, and the user's packs. `main` builds it; the
/// launcher holds it until a place is picked.
pub(crate) struct EditorBoot {
    settings: Settings,
    launch: Launch,
    user: packs::UserContent,
}

/// Reads `path` and opens the editor on it, recording it in Recent. Hands
/// `boot` back with the message when the place cannot be read, so the
/// launcher can offer another.
///
/// Parsing and the asset downloads both block; this runs before the editor
/// window exists, so only the launcher (if any) waits on it.
pub(crate) fn open_editor(
    path: &Path,
    boot: EditorBoot,
    cx: &mut App,
) -> Result<(), Box<(String, EditorBoot)>> {
    let EditorBoot {
        settings,
        launch,
        user,
    } = boot;
    println!("loading {}…", path.display());
    let place = match load(path, &launch, settings.icon_pack) {
        Ok(place) => place,
        Err(message) => {
            return Err(Box::new((
                message,
                EditorBoot {
                    settings,
                    launch,
                    user,
                },
            )))
        }
    };
    if let Err(err) = home::remember(home::RecentPlace {
        path: std::fs::canonicalize(path).unwrap_or(path.to_path_buf()),
        universe_id: None,
        place_id: None,
        name: None,
        opened: None,
    }) {
        eprintln!("rbxstudio: could not update the Recent list: {err}");
    }
    let title = SharedString::from(file_name(path));
    let options = window_options(&title, cx);
    cx.open_window(options, |window, cx| {
        let shell = cx.new(|cx| Shell::new(title, place, settings, launch, user, window, cx));
        cx.new(|cx| Root::new(shell, window, cx))
    })
    .expect("failed to open the main window");
    Ok(())
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
fn install_theme(user_theme: Option<&str>, cx: &mut App) {
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
    install_user_theme(user_theme, cx);
    install_fonts(cx);
}

/// GPUI Kit only swaps its syntax palette for the one a theme file's
/// `highlight` block defines; a theme without one keeps the kit's *light*
/// palette whatever its mode, which put navy keywords on the editor's near
/// black. Neither the built-in theme nor most user themes carry a block, so
/// they get the kit's own dark palette instead.
fn install_dark_highlight(cx: &mut App) {
    if Theme::global(cx).dark_theme.highlight.is_none() {
        Theme::global_mut(cx).highlight_theme = HighlightTheme::default_dark();
    }
}

/// Applies the theme `appearance.json` names, from `<config>/themes/`: the
/// first dark theme, in the file's own order, that its `ThemeSet` defines
/// under a name the registry does not already hold — the registry ignores a
/// duplicate name rather than replacing it, so a file that reused a
/// built-in's name would otherwise change nothing and say nothing. Anything
/// wrong with the file is reported on stderr and leaves the built-in theme in
/// place, with nothing from the file registered.
///
/// Only the toolkit's widgets follow it (see `packs`); the chrome this
/// editor draws itself still reads `tokens`.
fn install_user_theme(name: Option<&str>, cx: &mut App) {
    let Some(name) = name else {
        return;
    };
    let Some(json) = packs::theme_json(name) else {
        eprintln!("rbxstudio: theme {name:?} could not be read from the themes folder");
        return;
    };
    let set = match serde_json::from_str::<ThemeSet>(&json) {
        Ok(set) => set,
        Err(err) => {
            eprintln!("rbxstudio: theme {name:?} is not a valid theme file: {err}");
            return;
        }
    };
    let registry = ThemeRegistry::global(cx);
    let Some(picked) = first_new_dark(&set, |theme| registry.themes().contains_key(theme)) else {
        eprintln!(
            "rbxstudio: theme {name:?} defines no dark theme with a name of its own; \
             keeping the built-in"
        );
        return;
    };
    let picked = picked.name.clone();
    if let Err(err) = ThemeRegistry::global_mut(cx).load_themes_from_str(&json) {
        eprintln!("rbxstudio: theme {name:?} could not be loaded: {err}");
        return;
    }
    if let Some(theme) = ThemeRegistry::global(cx).themes().get(&picked).cloned() {
        Theme::global_mut(cx).dark_theme = theme;
    }
}

/// The theme [`install_user_theme`] switches to: the first dark one in `set`,
/// in the file's own order, that `known` does not already hold. Not the
/// registry's `sorted_themes()`, which orders by name and would hand a file
/// that lists `Zenith Dark` before `Aurora Dark` the wrong one.
fn first_new_dark(set: &ThemeSet, known: impl Fn(&str) -> bool) -> Option<&ThemeConfig> {
    set.themes
        .iter()
        .find(|theme| theme.mode == ThemeMode::Dark && !known(&theme.name))
}

/// The design system's own fonts, shipped with the editor (OFL, see the
/// licence files next to them) so that no machine has to have them
/// installed: `fc-query` names them `Manrope` (Regular, Medium, SemiBold,
/// Bold) and `JetBrains Mono` (Regular, Medium).
const BUNDLED_FONTS: [&[u8]; 6] = [
    include_bytes!("../../../assets/fonts/Manrope-Regular.ttf"),
    include_bytes!("../../../assets/fonts/Manrope-Medium.ttf"),
    include_bytes!("../../../assets/fonts/Manrope-SemiBold.ttf"),
    include_bytes!("../../../assets/fonts/Manrope-Bold.ttf"),
    include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../../../assets/fonts/JetBrainsMono-Medium.ttf"),
];

/// Registers the bundled fonts, then points the theme at the design
/// system's font stack for whichever of its families the text system now
/// has.
///
/// The theme takes one family name, not a CSS-style stack with fallbacks,
/// and a name that isn't installed is used as-is rather than falling
/// through — so the fallback has to happen here, by asking the text system
/// what exists before naming anything. Registration failing is reported
/// and survived: the platform UI font is a worse look, not a reason to
/// refuse to start.
fn install_fonts(cx: &mut App) {
    let fonts = BUNDLED_FONTS
        .iter()
        .map(|bytes| Cow::Borrowed(*bytes))
        .collect();
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        eprintln!(
            "rbxstudio: the bundled fonts could not be registered, using the platform font: {err}"
        );
    }
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

    /// Guards `BUNDLED_FONTS`: every embedded file is a TrueType font
    /// (`sfnt` version 1.0), so a swapped or truncated asset fails here
    /// rather than as a silent fallback to the platform font at startup.
    #[test]
    fn every_bundled_font_is_a_truetype_file() {
        for font in super::BUNDLED_FONTS {
            assert_eq!(&font[..4], &[0, 1, 0, 0]);
        }
    }

    fn set(themes: &[(&str, &str)]) -> ThemeSet {
        let entries: Vec<String> = themes
            .iter()
            .map(|(name, mode)| format!(r#"{{"name":"{name}","mode":"{mode}"}}"#))
            .collect();
        let json = format!(
            r#"{{"name":"pack","author":"me","themes":[{}]}}"#,
            entries.join(",")
        );
        serde_json::from_str(&json).expect("a minimal theme set parses")
    }

    /// The registry sorts by name; the file's own order is what a theme
    /// author wrote and what the doc comment promises.
    #[test]
    fn the_first_dark_theme_in_the_file_wins_not_the_alphabetically_first() {
        let themes = set(&[("Zenith Dark", "dark"), ("Aurora Dark", "dark")]);
        let picked = super::first_new_dark(&themes, |_| false).map(|t| t.name.to_string());
        assert_eq!(picked.as_deref(), Some("Zenith Dark"));
    }

    #[test]
    fn light_themes_and_names_the_registry_already_holds_are_skipped() {
        let themes = set(&[
            ("Dawn", "light"),
            ("rbx-native Dark", "dark"),
            ("Dusk", "dark"),
        ]);
        let picked = super::first_new_dark(&themes, |name| name == "rbx-native Dark")
            .map(|t| t.name.to_string());
        assert_eq!(picked.as_deref(), Some("Dusk"));
    }

    #[test]
    fn a_file_with_nothing_to_offer_picks_nothing() {
        let only_light = set(&[("Dawn", "light")]);
        assert!(super::first_new_dark(&only_light, |_| false).is_none());
        let only_known = set(&[("rbx-native Dark", "dark")]);
        assert!(super::first_new_dark(&only_known, |_| true).is_none());
    }

    #[test]
    fn the_window_is_titled_after_the_file() {
        assert_eq!(file_name(Path::new("/places/Demo.rbxl")), "Demo.rbxl");
        assert_eq!(file_name(Path::new("Demo.rbxl")), "Demo.rbxl");
    }
}
