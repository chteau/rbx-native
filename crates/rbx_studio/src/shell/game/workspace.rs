//! The window that holds a game: one [`Shell`] per place, each a complete
//! editor of its own (DOM, undo history, selection, camera, script tabs,
//! viewport), and only the active one in the element tree. Switching is
//! swapping which `Shell` the window draws, so nothing is reloaded and
//! nothing about the place left behind is lost.
//!
//! A place is loaded the first time it is shown, or when a game-wide
//! command has to write to it. Until then it is only a path: Find in Game
//! reads its file without building a render scene for it. A place left off
//! screen for [`PARK_AFTER`] lets go of its render scene (see
//! `WorkspaceView::park`) and rebuilds it from its DOM when shown again;
//! its DOM, undo history, selection and camera stay.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::component::Root;
use gpui_kit::*;

use super::super::close_place::AfterClose;
use super::super::Shell;
use super::manifest::Manifest;
use super::summary::Summary;
use crate::{EditorBoot, Place};

const PLACE_VARIABLE: &str = "RBX_STUDIO_GAME_PLACE";
const SYNC_VARIABLE: &str = "RBX_STUDIO_SYNC_SHARED";

/// Long enough that flicking between two places never rebuilds a scene.
const PARK_AFTER: Duration = Duration::from_secs(30);

/// One place of the game.
pub(crate) struct Slot {
    pub(crate) path: PathBuf,
    pub(crate) label: String,
    pub(crate) shell: Option<Entity<Shell>>,
    /// Unloads the place's scene once it has been off screen for
    /// [`PARK_AFTER`]; dropped (cancelling it) when the place is shown.
    park: Option<Task<()>>,
}

pub(crate) struct GameWorkspace {
    pub(super) manifest_path: PathBuf,
    pub(super) manifest: Manifest,
    pub(super) slots: Vec<Slot>,
    pub(super) active: usize,
    /// Places whose unsaved-changes prompt was answered during the leave in
    /// progress, so the next pass does not ask them again.
    released: HashSet<EntityId>,
    pub(super) find_window: Option<WindowHandle<Root>>,
    pub(super) sync_window: Option<WindowHandle<Root>>,
    /// Save All / Publish All's per-place report, while it is up.
    pub(super) summary: Option<Summary>,
    pub(super) window_handle: AnyWindowHandle,
}

impl GameWorkspace {
    /// The game in `manifest`, showing place `first` (already read into
    /// `place`, so a place that cannot be read stops the open before any
    /// window exists).
    pub(crate) fn new(
        manifest_path: PathBuf,
        manifest: Manifest,
        first: usize,
        place: Place,
        boot: EditorBoot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let slots = manifest
            .resolve(&manifest_path)
            .into_iter()
            .enumerate()
            .map(|(index, path)| Slot {
                label: manifest.label(index),
                path,
                shell: None,
                park: None,
            })
            .collect();
        let mut game = GameWorkspace {
            manifest_path,
            manifest,
            slots,
            active: first,
            released: HashSet::new(),
            find_window: None,
            sync_window: None,
            summary: None,
            window_handle: window.window_handle(),
        };
        let shell = game.build_shell(first, place, boot, window, cx);
        game.slots[first].shell = Some(shell);
        game.retarget(cx);
        cx.defer_in(window, |game, window, cx| game.apply_debug(window, cx));
        game
    }

    /// `RBX_STUDIO_GAME_PLACE=<index>` shows that place first,
    /// `RBX_STUDIO_FIND_IN_GAME` (see `find_window`) opens Find in Game and
    /// `RBX_STUDIO_SYNC_SHARED=1` opens Sync Shared: once, at open, for
    /// captures.
    fn apply_debug(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = std::env::var(PLACE_VARIABLE)
            .ok()
            .and_then(|value| value.parse().ok())
        {
            self.activate(index, window, cx);
        }
        if std::env::var_os(super::FIND_VARIABLE).is_some() {
            self.open_find(window, cx);
        }
        if std::env::var_os(SYNC_VARIABLE).is_some() {
            self.open_sync(window, cx);
        }
    }

    fn build_shell(
        &self,
        index: usize,
        place: Place,
        boot: EditorBoot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Shell> {
        let title = self.title_of(index);
        let game = cx.entity().downgrade();
        let EditorBoot {
            settings,
            launch,
            user,
        } = boot;
        cx.new(|cx| {
            let mut shell = Shell::new(title, place, settings, launch, user, window, cx);
            shell.game = Some(game);
            shell
        })
    }

    fn title_of(&self, index: usize) -> SharedString {
        format!(
            "{} \u{2014} {}",
            self.slots[index].label, self.manifest.name
        )
        .into()
    }

    pub(super) fn shell(&self, index: usize) -> Option<&Entity<Shell>> {
        self.slots.get(index)?.shell.as_ref()
    }

    pub(super) fn active_shell(&self) -> Option<&Entity<Shell>> {
        self.shell(self.active)
    }

    /// Every loaded place, with its index.
    pub(super) fn loaded(&self) -> impl Iterator<Item = (usize, &Entity<Shell>)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| Some((index, slot.shell.as_ref()?)))
    }

    /// The place's editor, reading its file first if it was not open yet.
    /// Blocks while the file is parsed, as opening a place always does.
    pub(super) fn load(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Entity<Shell>, String> {
        if let Some(shell) = self.shell(index) {
            return Ok(shell.clone());
        }
        let path = self.slots.get(index).ok_or("no such place")?.path.clone();
        let boot = crate::fresh_boot();
        let place = crate::load(&path, &boot.launch, boot.settings.icon_pack)
            .map_err(|err| format!("{}: {err}", self.slots[index].label))?;
        let shell = self.build_shell(index, place, boot, window, cx);
        self.slots[index].shell = Some(shell.clone());
        if index != self.active {
            self.park_later(index, cx);
        }
        // `Shell::new` points the menu bar at the editor it builds.
        self.retarget(cx);
        Ok(shell)
    }

    /// Shows place `index`, loading it first if need be. A place that cannot
    /// be read stays where it was, and says so in the active place's Output.
    pub(crate) fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index == self.active || index >= self.slots.len() {
            return;
        }
        match self.load(index, window, cx) {
            Ok(shell) => {
                self.park_later(self.active, cx);
                self.slots[index].park = None;
                shell.read(cx).viewport.read(cx).park(false);
                self.active = index;
                shell.update(cx, |shell, _| shell.retitle = true);
                self.retarget(cx);
                cx.notify();
            }
            Err(err) => self.warn(format!("Couldn\u{2019}t open {err}"), cx),
        }
    }

    fn park_later(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(shell) = self.shell(index).cloned() else {
            return;
        };
        self.slots[index].park = Some(cx.spawn(async move |_, cx| {
            cx.background_executor().timer(PARK_AFTER).await;
            shell.update(cx, |shell, cx| shell.viewport.read(cx).park(true));
        }));
    }

    /// A line in the active place's Output.
    pub(super) fn warn(&self, message: String, cx: &mut Context<Self>) {
        if let Some(shell) = self.active_shell() {
            shell.update(cx, |shell, cx| shell.warn(message, cx));
        }
    }

    /// The menu bar's commands act on the place on screen.
    fn retarget(&self, cx: &mut Context<Self>) {
        if let Some(shell) = self.active_shell() {
            crate::menu_bar::retarget(shell, cx);
        }
    }

    pub(super) fn dirty(&self, shell: &Entity<Shell>, cx: &App) -> bool {
        shell.read(cx).has_unsaved_changes()
    }

    // ------------------------------------------------------------- leaving

    /// The window manager's close: closes at once when nothing is unsaved,
    /// else asks each place with changes in turn.
    pub(crate) fn request_quit(&mut self, cx: &mut Context<Self>) -> bool {
        if self.loaded().any(|(_, shell)| self.dirty(shell, cx)) {
            self.begin_leave(AfterClose::Quit, cx);
            return false;
        }
        self.close_aux_windows(cx);
        true
    }

    /// Close Place, New, Open… or Restart from any place: every place with
    /// unsaved changes gets its own Save / Don't Save / Cancel first.
    pub(in crate::shell) fn begin_leave(&mut self, after: AfterClose, cx: &mut Context<Self>) {
        self.released.clear();
        self.continue_leave(after, cx);
    }

    /// One place answered its prompt with Save or Don't Save.
    pub(in crate::shell) fn released(
        &mut self,
        shell: EntityId,
        after: AfterClose,
        cx: &mut Context<Self>,
    ) {
        self.released.insert(shell);
        self.continue_leave(after, cx);
    }

    fn continue_leave(&mut self, after: AfterClose, cx: &mut Context<Self>) {
        let pending = self
            .loaded()
            .find(|(_, shell)| !self.released.contains(&shell.entity_id()) && self.dirty(shell, cx))
            .map(|(index, shell)| (index, shell.clone()));
        let Some((index, shell)) = pending else {
            return self.finish_leave(after, cx);
        };
        self.active = index;
        self.retarget(cx);
        shell.update(cx, |shell, cx| {
            shell.retitle = true;
            shell.after_close = after;
            shell.close_prompt = true;
            cx.notify();
        });
        cx.notify();
    }

    fn finish_leave(&mut self, after: AfterClose, cx: &mut Context<Self>) {
        self.close_aux_windows(cx);
        let handle = self.window_handle;
        let manifest = self.manifest_path.clone();
        let this = cx.weak_entity();
        // Deferred: this runs inside an update of the window's own views,
        // and a window cannot be removed from inside its own handler.
        cx.defer(move |cx| {
            let remove = |cx: &mut App| {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            };
            let open = |path: &Path, cx: &mut App| match crate::open_place(path, cx) {
                Ok(()) => remove(cx),
                Err(err) => {
                    let _ = this.update(cx, |game, cx| game.warn(format!("Open: {err}"), cx));
                }
            };
            match after {
                AfterClose::Quit => remove(cx),
                AfterClose::Home => {
                    crate::open_home_again(cx);
                    remove(cx);
                }
                AfterClose::Restart => {
                    crate::relaunch(&manifest);
                    cx.quit();
                }
                AfterClose::Open(path) => open(&path, cx),
                AfterClose::New => match super::super::open_place::create_baseplate() {
                    Ok(path) => open(&path, cx),
                    Err(err) => {
                        let _ = this.update(cx, |game, cx| game.warn(format!("New: {err}"), cx));
                    }
                },
            }
        });
    }

    fn close_aux_windows(&mut self, cx: &mut Context<Self>) {
        for handle in [self.find_window.take(), self.sync_window.take()]
            .into_iter()
            .flatten()
        {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }
}

impl Render for GameWorkspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .relative()
            .children(self.active_shell().cloned())
            .children(self.summary_overlay(cx))
    }
}

/// Opens the game whose manifest is at `path` in a window of its own,
/// showing its first place that can be read. Recorded in Recent like a
/// place, so reopening it reopens the whole game.
pub(crate) fn open_game(
    path: &Path,
    boot: EditorBoot,
    cx: &mut App,
) -> Result<(), Box<(String, EditorBoot)>> {
    // Absolute, so the places resolve the same whatever the working
    // directory, and Recent lists the game once.
    let path = &dunce::canonicalize(path).unwrap_or(path.to_path_buf());
    let manifest = match super::manifest::read(path) {
        Ok(manifest) => manifest,
        Err(message) => return Err(Box::new((message, boot))),
    };
    let places = manifest.resolve(path);
    let mut failures = Vec::new();
    let mut opened = None;
    for (index, place_path) in places.iter().enumerate() {
        match crate::load(place_path, &boot.launch, boot.settings.icon_pack) {
            Ok(place) => {
                opened = Some((index, place));
                break;
            }
            Err(err) => failures.push(err),
        }
    }
    let Some((first, place)) = opened else {
        return Err(Box::new((failures.join("\n"), boot)));
    };
    if let Err(err) = crate::home::remember(crate::home::RecentPlace {
        path: path.clone(),
        universe_id: None,
        place_id: None,
        name: Some(manifest.name.clone()),
        opened: None,
    }) {
        eprintln!("rbxstudio: could not update the Recent list: {err}");
    }
    let title = SharedString::from(manifest.name.clone());
    let options = crate::window_options(&title, cx);
    let path = path.to_path_buf();
    cx.open_window(options, |window, cx| {
        let game = cx.new(|cx| GameWorkspace::new(path, manifest, first, place, boot, window, cx));
        let closing = game.downgrade();
        window.on_window_should_close(cx, move |_, cx| {
            closing
                .update(cx, |game, cx| game.request_quit(cx))
                .unwrap_or(true)
        });
        cx.new(|cx| Root::new(game, window, cx))
    })
    .expect("failed to open the main window");
    Ok(())
}

impl Shell {
    /// The game this place belongs to and its window, for a game-wide menu
    /// command; says where those live when this place is on its own.
    pub(crate) fn game_of(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<(Entity<GameWorkspace>, AnyWindowHandle)> {
        match self.game.as_ref().and_then(WeakEntity::upgrade) {
            Some(game) => Some((game, self.window_handle)),
            None => {
                self.warn(
                    "This is a single place. Find in Game, Sync Shared, Save All and Publish All \
                     work in a game window: File \u{203a} Open Game\u{2026}"
                        .into(),
                    cx,
                );
                None
            }
        }
    }
}
