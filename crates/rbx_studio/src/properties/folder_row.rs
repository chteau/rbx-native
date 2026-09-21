//! The synthetic "Explorer Colour" row [`super::Properties::rows`] adds for
//! a `Folder` — split out of `properties.rs` to stay under `GUIDELINES.md`
//! §6's line budget, the same reason `properties/edit.rs` is its own file.
//! See `crate::folder_colors` for why this pseudo-property exists outside
//! `instance.properties()` at all: the same reasoning `Properties::rows`
//! already applies to its synthetic `Name` row, just for a colour tag with
//! no real Roblox `Folder` property to back it.

use crate::folder_colors::FOLDER_CLASS;

use super::{edit, EditKind, PropertyRow};

/// What the row shows before anything has been tagged: plain white, i.e. no
/// tint at all.
const NO_COLOR: (u8, u8, u8) = (255, 255, 255);

/// `Some(row)` when `class` is `Folder`, `None` for anything else —
/// `Folder`-only, per the roadmap request this ships. `category` is the
/// caller's own `self.category(class, edit::FOLDER_COLOR_PROPERTY)` lookup,
/// passed in rather than resolved here since that needs the
/// `ReflectionDatabase` this free function does not have.
pub(super) fn row(
    class: &str,
    category: String,
    folder_color: Option<(u8, u8, u8)>,
) -> Option<PropertyRow> {
    if class != FOLDER_CLASS {
        return None;
    }
    let (r, g, b) = folder_color.unwrap_or(NO_COLOR);
    Some(PropertyRow {
        name: edit::FOLDER_COLOR_PROPERTY.to_owned(),
        value: format!("({r}, {g}, {b})"),
        category,
        edit: Some(EditKind::Color { r, g, b }),
        mixed: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_row_for_a_class_other_than_folder() {
        assert_eq!(row("Part", "Other".to_owned(), Some((1, 2, 3))), None);
    }

    #[test]
    fn a_folder_row_seeds_from_the_stored_colour() {
        let row = row("Folder", "Other".to_owned(), Some((10, 20, 30))).unwrap();
        assert_eq!(row.name, edit::FOLDER_COLOR_PROPERTY);
        assert_eq!(row.value, "(10, 20, 30)");
        assert_eq!(
            row.edit,
            Some(EditKind::Color {
                r: 10,
                g: 20,
                b: 30
            })
        );
    }

    #[test]
    fn an_untagged_folder_seeds_white() {
        let row = row("Folder", "Other".to_owned(), None).unwrap();
        assert_eq!(
            row.edit,
            Some(EditKind::Color {
                r: 255,
                g: 255,
                b: 255
            })
        );
    }
}
