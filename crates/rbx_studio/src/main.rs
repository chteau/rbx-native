//! Desktop shell of the editor: the wgpu viewer of `rbx_viewer` drawn inside a
//! GPUI Kit window, next to the Explorer of the place, mutable through the
//! Command Bar (see `command_bar`).
//!
//! One place per process: `rbxstudio [--quality <auto|1..21>]
//! <file.rbxl|.rbxm|.rbxlx|.rbxmx>`. `RBX_STUDIO_SELECT=<name>[,<name>...]`
//! pre-selects the first instance of that name at startup, then adds each
//! further comma-separated name to the selection exactly as a
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
//! Ctrl+S writes the place back to the file it was opened from, in the
//! format it was opened in; `RBX_STUDIO_SAVE_AS=<path>` redirects one such
//! save to a scratch path instead (see `save`).

mod camera;
mod class_icons;
mod command_bar;
mod display;
mod explorer;
mod history;
mod menu_bar;
mod pacing;
mod pointer_lock;
mod properties;
mod render_image;
mod save;
mod script_editor;
mod settings;
mod settle;
mod shell;
mod style_editor;
mod transform;
mod workspace_view;

use std::path::{Path, PathBuf};

use gpui_kit::component::{Root, Theme, ThemeMode};
use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::{Headless, QualityLevel};

use camera::PlaceCamera;
use explorer::Explorer;
use properties::Properties;
use save::Format;
use settings::Settings;
use shell::Shell;

// `pub(crate)`: `shell` re-reads it — as the full comma list, not just the
// one name resolved above — both up front and again after `RBX_STUDIO_RUN`,
// to select whatever the script just created (see
// `Shell::apply_debug_select`).
pub(crate) const SELECT_VARIABLE: &str = "RBX_STUDIO_SELECT";

const USAGE: &str = "usage: rbxstudio [--quality <auto|1..21>] <file.rbxl|.rbxm|.rbxlx|.rbxmx>";
const WINDOW_SIZE: (f32, f32) = (1600.0, 900.0);

fn main() {
    // Loaded once, up front: the CLI's `--quality` still wins when given (see
    // `parse`), and the Explorer's initial visibility set comes straight from
    // here since nothing on the command line can express it.
    let settings = Settings::load();
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Arguments { path, quality } = match parse(&arguments, settings.quality) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("{message}\n{USAGE}");
            std::process::exit(2);
        }
    };

    // Parsing and the asset downloads both block; running them before the
    // window exists keeps the UI thread from ever stalling on the network.
    println!("loading {}…", path.display());
    let select = std::env::var(SELECT_VARIABLE).ok();
    let place = match load(&path, select.as_deref()) {
        Ok(place) => place,
        Err(message) => {
            eprintln!("rbxstudio: {message}");
            std::process::exit(1);
        }
    };
    let title = SharedString::from(file_name(&path));
    let show_all_services = settings.show_all_services;
    let orthographic = settings.orthographic;
    let axis_indicator = settings.axis_indicator;

    // The full Lucide catalog: the menu bar's icons are well outside the
    // default bundle the components themselves use. The Explorer's own class
    // icons are rasterized straight from `class_icons`'s embedded SVGs, not
    // painted through this asset source at all — see its own doc comment.
    let app = gpui_kit::application().with_assets(gpui_kit::assets::AllAssets);
    app.run(move |cx| {
        gpui_kit::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);

        cx.spawn(async move |cx| {
            let options = cx.update(|cx| window_options(&title, cx));
            cx.open_window(options, |window, cx| {
                let shell = cx.new(|cx| {
                    Shell::new(
                        title,
                        place,
                        quality,
                        show_all_services,
                        orthographic,
                        axis_indicator,
                        window,
                        cx,
                    )
                });
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
    /// The instance `RBX_STUDIO_SELECT` named, when it exists in the file.
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
}

fn load(path: &Path, select: Option<&str>) -> Result<Place, String> {
    let bytes = std::fs::read(path).map_err(|err| format!("failed to read {path:?}: {err}"))?;
    let format = Format::sniff(&bytes);
    let dom = rbx_viewer::read_place(path)?;
    let database = ReflectionDatabase::embedded();

    Ok(Place {
        explorer: Explorer::from_dom(&dom),
        selected: select.and_then(|name| explorer::find_by_name(&dom, name)),
        camera: PlaceCamera::from_dom(&dom),
        viewer: Headless::load(path, true)?,
        properties: Properties::new(database.clone()),
        dom,
        database,
        path: path.to_path_buf(),
        format,
    })
}

fn window_options(title: &SharedString, cx: &App) -> WindowOptions {
    let bounds = Bounds::centered(None, size(px(WINDOW_SIZE.0), px(WINDOW_SIZE.1)), cx);

    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(title.clone()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// A parsed command line: the place to open, and the graphics quality to open it
/// at.
struct Arguments {
    path: PathBuf,
    quality: QualityLevel,
}

/// Reads the command line: one place file, and `--quality` spelled exactly as
/// `rbxview` spells it (the same [`QualityLevel`] parser reads the value).
///
/// `default_quality` is what a bare invocation opens at when `--quality` is
/// absent — the persisted [`Settings`], so the last pick from the dropdown
/// survives a relaunch; an explicit flag still overrides it.
fn parse(arguments: &[String], default_quality: QualityLevel) -> Result<Arguments, String> {
    let mut path = None;
    let mut quality = default_quality;

    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--quality" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--quality needs a level".to_string())?;
                quality = value.parse()?;
            }
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}")),
            file if path.is_none() => path = Some(PathBuf::from(file)),
            _ => return Err("only one place file can be opened".to_string()),
        }
    }

    Ok(Arguments {
        path: path.ok_or_else(|| "no place file given".to_string())?,
        quality,
    })
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::{file_name, parse};
    use rbx_viewer::QualityLevel;
    use std::path::{Path, PathBuf};

    fn arguments(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn parse_with_default(values: &[&str]) -> Result<super::Arguments, String> {
        parse(&arguments(values), QualityLevel::Automatic)
    }

    #[test]
    fn a_single_file_is_the_place_to_open() {
        let parsed = parse_with_default(&["place.rbxl"]).expect("a path");
        assert_eq!(parsed.path, PathBuf::from("place.rbxl"));
    }

    #[test]
    fn no_argument_is_a_usage_error() {
        assert!(parse_with_default(&[]).is_err());
    }

    #[test]
    fn a_second_file_is_a_usage_error() {
        assert!(parse_with_default(&["one.rbxl", "two.rbxl"]).is_err());
    }

    #[test]
    fn flags_are_rejected_rather_than_opened_as_files() {
        assert!(parse_with_default(&["--orbit"]).is_err());
        assert!(parse_with_default(&["--size", "800x600"]).is_err());
    }

    // The editor has a frame clock, so unlike `rbxview` it manages the level
    // itself unless told otherwise.
    #[test]
    fn the_quality_is_automatic_until_a_level_is_given() {
        let parsed = parse_with_default(&["place.rbxl"]).expect("a path");
        assert_eq!(parsed.quality, QualityLevel::Automatic);
    }

    // The persisted setting is what a bare invocation should reopen at, not
    // always `Automatic`.
    #[test]
    fn a_missing_flag_falls_back_to_the_given_default_rather_than_always_automatic() {
        let parsed = parse(&arguments(&["place.rbxl"]), QualityLevel::Level(12)).expect("a path");
        assert_eq!(parsed.quality, QualityLevel::Level(12));
    }

    #[test]
    fn an_explicit_flag_overrides_the_given_default() {
        let parsed = parse(
            &arguments(&["--quality", "Level03", "place.rbxl"]),
            QualityLevel::Level(12),
        )
        .expect("a path");
        assert_eq!(parsed.quality, QualityLevel::Level(3));
    }

    #[test]
    fn a_quality_level_is_read_the_way_rbxview_reads_it() {
        let quality = |values: &[&str]| parse_with_default(values).map(|parsed| parsed.quality);

        assert_eq!(
            quality(&["--quality", "Level07", "place.rbxl"]),
            Ok(QualityLevel::Level(7))
        );
        assert_eq!(
            quality(&["place.rbxl", "--quality", "auto"]),
            Ok(QualityLevel::Automatic)
        );
        assert!(quality(&["--quality", "22", "place.rbxl"]).is_err());
        assert!(quality(&["place.rbxl", "--quality"]).is_err());
    }

    #[test]
    fn the_window_is_titled_after_the_file() {
        assert_eq!(file_name(Path::new("/places/Demo.rbxl")), "Demo.rbxl");
        assert_eq!(file_name(Path::new("Demo.rbxl")), "Demo.rbxl");
    }
}
