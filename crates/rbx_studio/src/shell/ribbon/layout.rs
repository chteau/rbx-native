//! Which commands each ribbon tab shows, grouped and ordered as Roblox
//! Studio 0.748's own ribbon (measured in Studio, tab by tab), with this
//! editor's own commands beside their nearest Studio neighbour.
//!
//! Studio commands that live only in Studio's menus (Cut/Copy/Paste in
//! Edit, Experience Settings in File, Input Action Manager, Localization
//! Tools and MicroProfiler in Window) are only in this editor's menus too.
//! Studio's own repeats are kept: the transform tools and their snap
//! increments head Home, Avatar and Model, as in Studio, and Part, Character,
//! GUI and the part-property tiles show on two tabs each.
//!
//! Data only, so the tests can hold the layout to that: every command has a
//! home, nothing repeats that Studio doesn't repeat, and a beta command is
//! gone with its beta off.

use super::Tab;
use crate::shell::layout::Panel;

/// One ribbon control. Most are one tile; `Tools`, `Pivot`,
/// `SolidModeling`, `Sun` and `Test` are the clusters and stacks those
/// commands already draw as one piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::shell) enum Item {
    /// Select, Move, Scale, Rotate, Transform and the Local toggle.
    Tools,
    /// Studio's Geometric options and its two increments: the snap stack.
    Snap,
    Part,
    Terrain,
    Character,
    Gui,
    Script,
    Import,
    Material,
    Color,
    Group,
    Ungroup,
    Lock,
    Anchor,
    /// A dock's show/hide toggle.
    Dock(Panel),
    DataStores,
    UpdatePlanning,
    AvatarSettings,
    Setup,
    Accessory,
    UgcValidation,
    UploadItem,
    FacePreview,
    ClipEditor,
    StyleEditor,
    PseudoLoc,
    Format,
    Find,
    CommandBar,
    Pivot,
    Effect,
    Align,
    SolidModeling,
    Sun,
    /// Play, Stop and the rest of the test controls, `test_tile_order`'s.
    Test,
    ViewportSettings,
    ShowUi,
    Fullbright,
    /// Every plugin toolbar, a group each.
    PluginToolbars,
    /// Popular, Manage, Folder and Reload.
    PluginTools,
}

/// The beta switches that add commands to the ribbon.
#[derive(Debug, Clone, Copy, Default)]
pub(in crate::shell) struct Gates {
    pub(in crate::shell) update_planning: bool,
    pub(in crate::shell) pseudo_loc: bool,
    pub(in crate::shell) local_run: bool,
}

impl Gates {
    pub(in crate::shell) fn current() -> Self {
        Gates {
            update_planning: crate::beta::enabled(crate::update_planning::BETA_ID),
            pseudo_loc: crate::beta::enabled(crate::shell::PSEUDO_LOC_BETA_ID),
            local_run: crate::beta::enabled(crate::shell::LOCAL_RUN_BETA_ID),
        }
    }
}

/// Local Run's own debugging docks, only with its beta on.
const LOCAL_RUN_DOCKS: [Panel; 3] = [Panel::RemoteInspector, Panel::LeakFinder, Panel::LoadTest];

/// `tab`'s groups, left to right, a separator between each.
pub(in crate::shell) fn groups(tab: Tab, gates: Gates) -> Vec<Vec<Item>> {
    use Item::*;
    let groups = match tab {
        Tab::Home => vec![
            vec![Tools, Snap],
            vec![Part, Terrain, Character, Gui, Script, Import],
            vec![Material, Color, Group, Ungroup, Lock, Anchor],
            vec![
                Dock(Panel::Explorer),
                Dock(Panel::Properties),
                Dock(Panel::Toolbox),
                Dock(Panel::Viewport),
                Dock(Panel::Layers),
                Dock(Panel::Loren),
            ],
            vec![
                DataStores,
                UpdatePlanning,
                Dock(Panel::PrePublish),
                Dock(Panel::SecurityAudit),
            ],
        ],
        Tab::Avatar => vec![
            vec![Tools, Snap],
            vec![AvatarSettings],
            vec![
                Character,
                Setup,
                Accessory,
                UgcValidation,
                UploadItem,
                FacePreview,
            ],
            vec![ClipEditor],
        ],
        Tab::Ui => vec![vec![Gui], vec![StyleEditor, PseudoLoc]],
        Tab::Script => vec![
            vec![Script, Format, Find],
            vec![CommandBar, Dock(Panel::Output)],
            vec![Dock(Panel::CallStack), Dock(Panel::Watch)],
            vec![
                Dock(Panel::ScriptAnalysis),
                Dock(Panel::Bytecode),
                Dock(Panel::Tests),
            ],
            vec![Dock(Panel::Argon), Dock(Panel::Wally)],
        ],
        Tab::Model => vec![
            vec![Tools, Snap],
            vec![Pivot],
            vec![Part, Effect],
            vec![Material, Color, Group, Ungroup, Lock, Anchor],
            vec![Align],
            vec![SolidModeling],
            vec![Sun],
        ],
        Tab::Test => vec![
            vec![Test],
            vec![
                Dock(Panel::Playtests),
                Dock(Panel::RemoteInspector),
                Dock(Panel::LeakFinder),
                Dock(Panel::LoadTest),
            ],
            vec![ViewportSettings, ShowUi, Fullbright],
        ],
        Tab::Plugins => vec![vec![PluginToolbars], vec![PluginTools]],
    };
    groups
        .into_iter()
        .map(|group| {
            group
                .into_iter()
                .filter(|item| match item {
                    UpdatePlanning => gates.update_planning,
                    PseudoLoc => gates.pseudo_loc,
                    Dock(panel) if LOCAL_RUN_DOCKS.contains(panel) => gates.local_run,
                    _ => true,
                })
                .collect::<Vec<_>>()
        })
        .filter(|group| !group.is_empty())
        .collect()
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
