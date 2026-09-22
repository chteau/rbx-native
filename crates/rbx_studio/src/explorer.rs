//! The place's instance tree, ordered and iconed the way Roblox Studio's own
//! Explorer shows it.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::tree::TreeItem;
use gpui_kit::{RenderImage, SharedString};
use rbx_dom::{Ref, WeakDom};

use crate::class_icons::{self, IconPack};
use crate::folder_colors::{FolderColors, FOLDER_CLASS};

pub(crate) mod insert;
pub(crate) mod reparent;

/// The order Studio lists services in — neither alphabetical nor the order the
/// file stores them in. Anything else a place has at its root comes after.
const SERVICE_ORDER: [&str; 14] = [
    "Workspace",
    "Players",
    "Lighting",
    "MaterialService",
    "ReplicatedFirst",
    "ReplicatedStorage",
    "ServerScriptService",
    "ServerStorage",
    "StarterGui",
    "StarterPack",
    "StarterPlayer",
    "Teams",
    "SoundService",
    "TextChatService",
];

/// Every root "service" class Roblox itself creates in a place file, whether
/// or not Studio's default Explorer view shows it (verified against every
/// root instance dump across this repo's fixtures). A root class absent from
/// this list is some real, user-placed instance — never one of the
/// deliberately-noisy internal ones — so the default filter below must never
/// hide it.
const KNOWN_SERVICES: [&str; 55] = [
    "AssetService",
    "Chat",
    "CollectionService",
    "ContextActionService",
    "CookiesService",
    "CSGDictionaryService",
    "DataStoreService",
    "Debris",
    "DevPackages",
    "GamePassService",
    "GuidRegistryService",
    "HttpService",
    "InsertService",
    "Instance",
    "Lighting",
    "LocalizationService",
    "LodDataService",
    "LuaWebService",
    "MaterialService",
    "NonReplicatedCSGDictionaryService",
    "Packages",
    "PermissionsService",
    "PhysicsService",
    "PlayerEmulatorService",
    "Players",
    "ProcessInstancePhysicsService",
    "ProximityPromptService",
    "ReplicatedFirst",
    "ReplicatedStorage",
    "ScriptService",
    "Selection",
    "SerializationService",
    "ServerPackages",
    "ServerScriptService",
    "ServerStorage",
    "ServiceVisibilityService",
    "SoundService",
    "StarterGui",
    "StarterPack",
    "StarterPlayer",
    "StudioData",
    "Teams",
    "TeleportService",
    "TestService",
    "TextChatService",
    "TimerService",
    "TouchInputService",
    "TweenService",
    "UGCAvatarService",
    "VideoCaptureService",
    "VideoService",
    "VirtualInputManager",
    "VoiceChatService",
    "VRService",
    "Workspace",
];

/// The classes a UI tree hangs from: the `LayerCollector`s a place holds.
const UI_ROOT_CLASSES: [&str; 3] = ["ScreenGui", "BillboardGui", "SurfaceGui"];

/// One instance, as the explorer needs it: the DOM's own borrows cannot outlive
/// the load, and the tree keeps its items for the life of the window.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    id: u32,
    class: String,
    name: String,
    /// This instance's own `crate::folder_colors::path_of` — computed once
    /// here rather than re-walked per row, since only `items` (building the
    /// tinted icon for a tagged `Folder`) needs it and it no longer has a
    /// live `&WeakDom` to walk by the time it runs.
    path: String,
    children: Vec<Node>,
}

/// A row's icon: this project's own icon, rasterized from the kit (see
/// `class_icons`), when its class is covered; a Lucide stand-in otherwise.
#[derive(Clone)]
pub(crate) enum ClassIcon {
    Sprite(Arc<RenderImage>),
    Lucide(IconName),
}

/// A whole place, ready to hand to a `TreeState`.
pub(crate) struct Explorer {
    /// Studio's own default Explorer set: `SERVICE_ORDER` plus anything not a
    /// recognized service at all.
    default_items: Vec<TreeItem>,
    /// Every root the file has, for the "show all services" toggle.
    all_items: Vec<TreeItem>,
    icons: HashMap<SharedString, ClassIcon>,
    /// Every row's class, kept alongside `icons` so [`Explorer::set_icon_pack`]
    /// can re-resolve icons for the other pack without re-walking `dom` — the
    /// tree's own `TreeItem`s (and their expansion state) never need to
    /// change for that, only which sprite each row's icon points at.
    classes: HashMap<SharedString, String>,
    /// Every row's own `crate::folder_colors::path_of`, kept alongside
    /// `classes` so [`Explorer::set_icon_pack`] can re-tint a tagged
    /// `Folder`'s icon for the other pack without re-walking `dom` either.
    paths: HashMap<SharedString, String>,
}

impl Explorer {
    /// `folder_colors`/`place` are only consulted for a `Folder` node, to
    /// bake a tagged one's tinted icon (see `class_icons::tint`) into
    /// `icons` right here — computed once per rebuild rather than once per
    /// render, and shared (via `tinted`) across every folder that happens to
    /// carry the same tag colour.
    pub(crate) fn from_dom(
        dom: &WeakDom,
        pack: IconPack,
        folder_colors: &FolderColors,
        place: &Path,
    ) -> Self {
        let mut icons = HashMap::new();
        let mut classes = HashMap::new();
        let mut paths = HashMap::new();
        // Every instance of a class shares one icon; resolving it once per
        // class rather than once per instance keeps a place with thousands of
        // parts from repeating the same lookup thousands of times.
        let mut per_class = HashMap::new();
        let mut tinted = HashMap::new();
        let roots = roots(dom);
        let all_items = items(
            &roots,
            &mut per_class,
            &mut icons,
            &mut classes,
            &mut paths,
            pack,
            folder_colors,
            place,
            &mut tinted,
        );
        let default_items = roots
            .iter()
            .zip(all_items.iter())
            .filter(|(node, _)| is_default_visible(&node.class))
            .map(|(_, item)| item.clone())
            .collect();

        Explorer {
            default_items,
            all_items,
            icons,
            classes,
            paths,
        }
    }

    /// Re-resolves every row's icon for `pack`, keeping the same `TreeItem`s
    /// (so expansion/selection state, which lives on them, survives) and the
    /// same `classes`/`paths` maps this was built from. Re-applies a tagged
    /// `Folder`'s tint on top of the freshly resolved pack icon — `tint`
    /// overwrites every non-transparent pixel with the tag colour regardless
    /// of which pack it started from, but the untinted base icon still has
    /// to come from the pack just switched to.
    pub(crate) fn set_icon_pack(
        &self,
        pack: IconPack,
        folder_colors: &FolderColors,
        place: &Path,
    ) -> Explorer {
        let mut per_class = HashMap::new();
        let mut tinted = HashMap::new();
        let icons = self
            .classes
            .iter()
            .map(|(id, class)| {
                let icon = per_class
                    .entry(class.clone())
                    .or_insert_with(|| resolve_icon(class, pack))
                    .clone();
                let icon = if class == FOLDER_CLASS {
                    self.paths
                        .get(id)
                        .and_then(|path| folder_colors.get(place, path))
                        .map(|color| {
                            tinted
                                .entry(color)
                                .or_insert_with(|| tint_icon(&icon, color))
                                .clone()
                        })
                        .unwrap_or(icon)
                } else {
                    icon
                };
                (id.clone(), icon)
            })
            .collect();

        Explorer {
            default_items: self.default_items.clone(),
            all_items: self.all_items.clone(),
            icons,
            classes: self.classes.clone(),
            paths: self.paths.clone(),
        }
    }

    /// The root items to show. Cloning a `TreeItem` shares its expansion
    /// state, so the tree widget and this list stay in agreement.
    pub(crate) fn items(&self, show_all: bool) -> Vec<TreeItem> {
        if show_all {
            self.all_items.clone()
        } else {
            self.default_items.clone()
        }
    }

    /// The UI editor's view of the place: every `ScreenGui`, `BillboardGui`
    /// and `SurfaceGui` as a root of its own, wherever it sits, with
    /// everything beneath it — and nothing that is not beneath one. The rows
    /// are the full tree's own, so expanding one here expands it there.
    pub(crate) fn ui_items(&self) -> Vec<TreeItem> {
        fn collect(
            items: &[TreeItem],
            classes: &HashMap<SharedString, String>,
            into: &mut Vec<TreeItem>,
        ) {
            for item in items {
                match classes.get(&item.id) {
                    Some(class) if UI_ROOT_CLASSES.contains(&class.as_str()) => {
                        into.push(item.clone());
                    }
                    _ => collect(&item.children, classes, into),
                }
            }
        }
        let mut roots = Vec::new();
        collect(&self.all_items, &self.classes, &mut roots);
        roots
    }

    pub(crate) fn icon(&self, id: &SharedString) -> ClassIcon {
        self.icons
            .get(id)
            .cloned()
            .unwrap_or(ClassIcon::Lucide(IconName::CircleDot))
    }

    /// The tree row standing for `reference`, whatever depth it sits at.
    /// Searched among every root, not just the visible set, so a selection can
    /// be revealed even under a service the default filter hides.
    pub(crate) fn item(&self, reference: Ref) -> Option<TreeItem> {
        let id = item_id(reference);
        fn find(items: &[TreeItem], id: &SharedString) -> Option<TreeItem> {
            items.iter().find_map(|item| {
                (item.id == *id)
                    .then(|| item.clone())
                    .or_else(|| find(&item.children, id))
            })
        }
        find(&self.all_items, &id)
    }
}

/// A row's id is its referent, so a selected row leads straight back to the
/// DOM without a lookup table.
pub(crate) fn item_id(reference: Ref) -> SharedString {
    SharedString::from(reference.value().to_string())
}

/// The referent a row's id was minted from (see [`item_id`]).
pub(crate) fn item_ref(id: &SharedString) -> Option<Ref> {
    id.parse().ok().map(Ref::new)
}

/// The instance a scripted launch's `--select` target names: the one at that
/// Explorer path when the path resolves, otherwise the first instance simply
/// called that.
///
/// Trying the path first and falling back, rather than branching on whether
/// `target` contains a `.`, is what keeps an instance whose own name has a dot
/// in it (`Config.v2`) reachable — it fails as a path, then matches as a name.
pub(crate) fn resolve(dom: &WeakDom, target: &str) -> Option<Ref> {
    find_by_path(dom, target).or_else(|| find_by_name(dom, target))
}

/// The instance at `path`, each segment naming a child of the one before it
/// and the first naming a root — the spelling [`crate::folder_colors::path_of`]
/// produces (`Workspace.Nested.MyFolder`). `None` unless every segment
/// matches: this is an exact lookup, not [`find_by_name`]'s search.
fn find_by_path(dom: &WeakDom, path: &str) -> Option<Ref> {
    let mut segments = path.split('.');
    let mut current = child_named(dom, dom.root_refs(), segments.next()?)?;
    for segment in segments {
        current = child_named(dom, dom.get(current)?.children(), segment)?;
    }
    Some(current)
}

fn child_named(dom: &WeakDom, references: &[Ref], name: &str) -> Option<Ref> {
    references
        .iter()
        .copied()
        .find(|&reference| dom.get(reference).is_some_and(|child| child.name() == name))
}

/// The first instance called `name`, depth-first in file order. A debugging
/// aid for scripted launches, not a search: names are not unique.
pub(crate) fn find_by_name(dom: &WeakDom, name: &str) -> Option<Ref> {
    fn visit(dom: &WeakDom, references: &[Ref], name: &str) -> Option<Ref> {
        references.iter().find_map(|&reference| {
            let instance = dom.get(reference)?;
            (instance.name() == name)
                .then_some(reference)
                .or_else(|| visit(dom, instance.children(), name))
        })
    }
    visit(dom, dom.root_refs(), name)
}

fn roots(dom: &WeakDom) -> Vec<Node> {
    let mut roots: Vec<Node> = dom
        .root_refs()
        .iter()
        .filter_map(|reference| node(dom, *reference, ""))
        .collect();
    sort_roots(&mut roots);
    roots
}

/// `parent_path` is the already-built `crate::folder_colors::path_of` of
/// `reference`'s parent (empty at a root), so the whole tree's paths cost one
/// walk down rather than one walk up per node.
fn node(dom: &WeakDom, reference: Ref, parent_path: &str) -> Option<Node> {
    let instance = dom.get(reference)?;
    let name = instance.name().to_string();
    let path = if parent_path.is_empty() {
        name.clone()
    } else {
        format!("{parent_path}.{name}")
    };

    Some(Node {
        id: reference.value(),
        class: instance.class().to_string(),
        name,
        // Children keep the file's order, which is the order Studio shows them
        // in for the services that matter (Camera and Terrain before the parts).
        children: instance
            .children()
            .iter()
            .filter_map(|child| node(dom, *child, &path))
            .collect(),
        path,
    })
}

/// Services first, in Studio's fixed order; everything else alphabetically after.
fn sort_roots(roots: &mut [Node]) {
    roots.sort_by(
        |left, right| match (rank(&left.class), rank(&right.class)) {
            (Some(left), Some(right)) => left.cmp(&right),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            // Case-insensitive, like Studio's own listing: a lowercase name
            // must not sort after every uppercase one.
            (None, None) => left
                .name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.name.cmp(&right.name)),
        },
    );
}

fn rank(class: &str) -> Option<usize> {
    SERVICE_ORDER.iter().position(|service| *service == class)
}

/// Whether a root of this class is one of the services this Explorer knows,
/// shown or not. Asked beside the dump's own `Service` tag because a few of
/// them (`Packages`, `SerializationService`) are not in the dump at all.
pub(crate) fn is_known_service(class: &str) -> bool {
    rank(class).is_some() || KNOWN_SERVICES.contains(&class)
}

/// Whether Studio's default Explorer view shows a root of this class without
/// the "show all services" toggle: its 14 fixed services, or anything that is
/// not one of Roblox's own well-known service classes at all.
fn is_default_visible(class: &str) -> bool {
    rank(class).is_some() || !KNOWN_SERVICES.contains(&class)
}

/// `tinted` caches one recolored icon per tag colour actually in use this
/// rebuild, so two folders sharing a tag share one rasterized bitmap instead
/// of each paying `class_icons::tint`'s own byte-buffer pass.
#[allow(clippy::too_many_arguments)]
fn items(
    nodes: &[Node],
    per_class: &mut HashMap<String, ClassIcon>,
    icons: &mut HashMap<SharedString, ClassIcon>,
    classes: &mut HashMap<SharedString, String>,
    paths: &mut HashMap<SharedString, String>,
    pack: IconPack,
    folder_colors: &FolderColors,
    place: &Path,
    tinted: &mut HashMap<(u8, u8, u8), ClassIcon>,
) -> Vec<TreeItem> {
    nodes
        .iter()
        .map(|node| {
            let id = item_id(Ref::new(node.id));
            let class_icon = per_class
                .entry(node.class.clone())
                .or_insert_with(|| resolve_icon(&node.class, pack))
                .clone();
            let class_icon = if node.class == FOLDER_CLASS {
                folder_colors
                    .get(place, &node.path)
                    .map(|color| {
                        tinted
                            .entry(color)
                            .or_insert_with(|| tint_icon(&class_icon, color))
                            .clone()
                    })
                    .unwrap_or(class_icon)
            } else {
                class_icon
            };
            icons.insert(id.clone(), class_icon);
            classes.insert(id.clone(), node.class.clone());
            paths.insert(id.clone(), node.path.clone());

            TreeItem::new(id, node.name.clone()).children(items(
                &node.children,
                per_class,
                icons,
                classes,
                paths,
                pack,
                folder_colors,
                place,
                tinted,
            ))
        })
        .collect()
}

/// A tagged `Folder`'s own icon, recolored to its tag — see
/// `class_icons::tint`. Falls back to `icon` itself (untinted) for the
/// Lucide stand-in, which is not something this project rasterizes and so
/// has no bitmap to recolor; unreached in practice today, since the icon kit
/// always covers `Folder` (`class_icons::CLASS_ICON_SLUGS`), but a class
/// whose only icon is a glyph should degrade to that glyph rather than panic
/// if the kit ever stops covering it.
fn tint_icon(icon: &ClassIcon, color: (u8, u8, u8)) -> ClassIcon {
    match icon {
        ClassIcon::Sprite(image) => class_icons::tint(image, color)
            .map(ClassIcon::Sprite)
            .unwrap_or_else(|| icon.clone()),
        ClassIcon::Lucide(_) => icon.clone(),
    }
}

/// This project's own icon for `class` from `pack` (see `class_icons`) when
/// the kit covers it, the Lucide stand-in otherwise.
///
/// `pub(crate)`: also the insert picker's entry point
/// (`shell::explorer_edit::picker`), so a class listed there and an instance
/// of it in the tree cannot end up drawn from two different lookups.
/// `class_icons::icon_tile` memoizes, so calling this per visible row per
/// frame costs a hash lookup rather than a rasterization.
pub(crate) fn resolve_icon(class: &str, pack: IconPack) -> ClassIcon {
    match class_icons::icon_tile(class, pack) {
        Some(image) => ClassIcon::Sprite(image),
        None => ClassIcon::Lucide(icon(class)),
    }
}

/// A Lucide stand-in per class family, for a class the mirrored
/// `ExplorerImageIndex` metadata does not cover.
fn icon(class: &str) -> IconName {
    match class {
        "Workspace" => IconName::Globe,
        "Lighting" => IconName::Lightbulb,
        "Camera" => IconName::Camera,
        "Terrain" => IconName::Mountain,
        "Model" => IconName::Package,
        "Folder" => IconName::Folder,
        _ if class.ends_with("Script") => IconName::FileCode,
        _ if class.ends_with("Part") || class == "UnionOperation" => IconName::Box,
        _ if class.ends_with("Service") || class.ends_with("Storage") => IconName::Server,
        _ if rank(class).is_some() => IconName::Server,
        _ => IconName::CircleDot,
    }
}

#[cfg(test)]
#[path = "explorer/tests.rs"]
mod tests;
