//! The `⋯` beside the insert picker's search field, and the insertion
//! preferences behind it: real Studio's two, and inserting parts anchored.

use gpui_kit::assets::IconName;
use gpui_kit::*;

use super::super::super::chrome;
use super::super::super::menu::{self, MenuId};
use super::Shell;

impl Shell {
    /// The `⋯` beside the search field: real Studio's two insertion
    /// preferences, in the place real Studio keeps them.
    pub(super) fn insertion_options(&self, cx: &mut Context<Self>) -> AnyElement {
        let increment = self.increment_names();
        let expand = self.expand_on_select();
        let anchored = self.insert_anchored;
        menu::dropdown(
            self,
            MenuId::InsertOptions,
            chrome::icon_trigger("insert-options", IconName::Ellipsis, "Insertion options"),
            vec![
                menu::item("Increment names for new instances")
                    .checked(increment)
                    .on_click(move |shell, cx| shell.set_increment_names(!increment, cx)),
                menu::item("Expand hierarchy when selecting")
                    .checked(expand)
                    .on_click(move |shell, cx| shell.set_expand_on_select(!expand, cx)),
                menu::item("Insert parts anchored")
                    .checked(anchored)
                    .on_click(move |shell, cx| shell.set_insert_anchored(!anchored, cx)),
            ],
            cx,
        )
        .into_any_element()
    }

    /// Whether a new instance of a class a sibling already carries the name
    /// of is numbered, see `explorer::insert::incremented_name`.
    pub(in crate::shell) fn increment_names(&self) -> bool {
        self.increment_names
    }

    pub(in crate::shell) fn set_increment_names(
        &mut self,
        increment: bool,
        cx: &mut Context<Self>,
    ) {
        self.increment_names = increment;
        self.save_settings();
        cx.notify();
    }

    /// Whether inserting, pasting or selecting expands the tree to reveal
    /// the instance, see `Shell::select`.
    pub(in crate::shell) fn expand_on_select(&self) -> bool {
        self.expand_on_select
    }

    pub(in crate::shell) fn set_expand_on_select(&mut self, expand: bool, cx: &mut Context<Self>) {
        self.expand_on_select = expand;
        self.save_settings();
        cx.notify();
    }

    /// Whether a part the editor inserts starts `Anchored`, see
    /// `Settings::insert_anchored`.
    pub(in crate::shell) fn set_insert_anchored(&mut self, anchored: bool, cx: &mut Context<Self>) {
        self.insert_anchored = anchored;
        self.save_settings();
        cx.notify();
    }
}
