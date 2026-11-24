//! Who made the third-party fonts, icons, models, data and ideas RbxNative
//! uses: `assets/credits.toml`, the one list Settings › About shows and
//! `CREDITS.md` is generated from (the tests hold the two together).

use std::sync::LazyLock;

use serde::Deserialize;

#[cfg(test)]
mod tests;

const SOURCE: &str = include_str!("../../../assets/credits.toml");

/// One heading's worth of credits: Fonts, Icons and graphics, ...
#[derive(Deserialize)]
pub(crate) struct Group {
    pub(crate) title: String,
    #[serde(rename = "entry")]
    pub(crate) entries: Vec<Credit>,
}

#[derive(Deserialize)]
pub(crate) struct Credit {
    pub(crate) name: String,
    pub(crate) owner: String,
    pub(crate) url: String,
    pub(crate) license: String,
    #[serde(rename = "use")]
    pub(crate) used_for: String,
    /// The licence or terms require the credit, rather than only welcome it.
    /// For CREDITS.md, like `notice`, which the packaging script reads.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) required: bool,
    /// `<repo path> as <file name>`: a licence text every package carries
    /// in its `licenses/` folder.
    #[cfg_attr(not(test), allow(dead_code))]
    #[serde(default)]
    pub(crate) notice: Option<String>,
}

#[derive(Deserialize)]
struct File {
    group: Vec<Group>,
}

/// Every group, in the file's order. Empty only if the file stops parsing,
/// which the tests rule out.
pub(crate) fn groups() -> &'static [Group] {
    static GROUPS: LazyLock<Vec<Group>> = LazyLock::new(|| {
        toml::from_str::<File>(SOURCE)
            .map(|file| file.group)
            .unwrap_or_default()
    });
    &GROUPS
}
