//! Wires the `Folder`-only "Explorer Colour" pseudo-property (see
//! `crate::folder_colors` and `properties::edit::FOLDER_COLOR_PROPERTY`)
//! into `Shell`: seeding its row's current value, routing a commit to the
//! local colour store instead of the DOM — the same idea
//! `properties::edit::commit`'s `NAME_PROPERTY` branch applies to `Name`,
//! just routed here rather than there, since the store (and the place path
//! that keys it) lives on `Shell`, not on a bare `WeakDom` — and collecting
//! every tagged `Folder`'s current tint for the Explorer tree to paint.

use std::collections::HashMap;
use std::path::Path;

use gpui_kit::{Context, SharedString};
use rbx_dom::{Ref, WeakDom};

use crate::explorer;
use crate::folder_colors::{self, FolderColors, Rgb};

use super::Shell;

impl Shell {
    /// The colour tagged on `reference`, if it is a `Folder` and the store
    /// has one for its current Explorer path — what `Properties::rows` seeds
    /// the synthetic row's `EditKind::Color` widget with (see
    /// `shell::panels::properties`).
    pub(super) fn folder_color(&self, reference: Ref) -> Option<Rgb> {
        let instance = self.dom.get(reference)?;
        if instance.class() != folder_colors::FOLDER_CLASS {
            return None;
        }
        let path = folder_colors::path_of(&self.dom, reference)?;
        self.folder_colors.get(&self.path, &path)
    }

    /// Every tagged `Folder`'s current tint, keyed the way `explorer::item_id`
    /// keys a tree row — what `shell::panels::instance_tree` looks a row's
    /// tint up by (see `shell::rows::row`).
    pub(super) fn folder_tints(&self) -> HashMap<SharedString, Rgb> {
        let mut tints = HashMap::new();
        collect_tints(
            &self.dom,
            self.dom.root_refs(),
            &self.folder_colors,
            &self.path,
            &mut tints,
        );
        tints
    }

    /// Routes an "Explorer Colour" row's commit to the local colour store
    /// instead of `WeakDom::set_property` — see this module's doc comment.
    /// The DOM, and so undo history, is left untouched; only the Explorer's
    /// rows are rebuilt afterwards, to repaint the tint.
    pub(super) fn commit_folder_color(
        &mut self,
        reference: Ref,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let path = folder_colors::path_of(&self.dom, reference)
            .ok_or_else(|| "the instance no longer exists".to_string())?;
        let color = parse_rgb(text)?;
        self.folder_colors.set(&self.path, &path, color);
        // A lost write here is not fatal — the tag simply reverts to
        // whatever was last saved next launch — so this mirrors
        // `Shell::save_settings`'s own "don't interrupt the editor over it".
        let _ = self.folder_colors.save();
        self.rebuild_explorer(cx);
        Ok(())
    }
}

fn collect_tints(
    dom: &WeakDom,
    refs: &[Ref],
    colors: &FolderColors,
    place: &Path,
    out: &mut HashMap<SharedString, Rgb>,
) {
    for &reference in refs {
        let Some(instance) = dom.get(reference) else {
            continue;
        };
        if instance.class() == folder_colors::FOLDER_CLASS {
            if let Some(path) = folder_colors::path_of(dom, reference) {
                if let Some(color) = colors.get(place, &path) {
                    out.insert(explorer::item_id(reference), color);
                }
            }
        }
        collect_tints(dom, instance.children(), colors, place, out);
    }
}

/// Parses the `"r, g, b"` text an `EditKind::Color` row commits (see
/// `shell::edit::build_row_widget`) — the same shape
/// `properties::edit::parse`'s `Color3uint8` arm accepts, duplicated in
/// miniature here since that parser type-checks against a DOM `Variant` this
/// pseudo-property has none of.
fn parse_rgb(text: &str) -> Result<Rgb, String> {
    let parts: Vec<&str> = text.split(',').map(str::trim).collect();
    if parts.len() != 3 {
        return Err(format!(
            "expected 3 comma-separated numbers, got {}",
            parts.len()
        ));
    }
    let channel = |value: &str| {
        value
            .parse::<u8>()
            .map_err(|_| format!("{value:?} is not a number from 0 to 255"))
    };
    Ok((channel(parts[0])?, channel(parts[1])?, channel(parts[2])?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_comma_joined_triplet() {
        assert_eq!(parse_rgb("10, 20, 30"), Ok((10, 20, 30)));
        assert_eq!(parse_rgb("0,0,0"), Ok((0, 0, 0)));
        assert_eq!(parse_rgb("255, 255, 255"), Ok((255, 255, 255)));
    }

    #[test]
    fn rejects_the_wrong_number_of_parts() {
        assert!(parse_rgb("1, 2").is_err());
        assert!(parse_rgb("1, 2, 3, 4").is_err());
    }

    #[test]
    fn rejects_a_channel_out_of_0_to_255() {
        assert!(parse_rgb("256, 0, 0").is_err());
        assert!(parse_rgb("-1, 0, 0").is_err());
        assert!(parse_rgb("red, 0, 0").is_err());
    }
}
