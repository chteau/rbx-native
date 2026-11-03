//! The Toolbox: Studio's Creator Store dock. It searches the store's
//! anonymous routes (`rbx_cloud::store_items`) by category, an empty search
//! listing the category's most popular, and inserts a pick into the place
//! the way Studio's own Toolbox does (measured in real Studio):
//!
//! - a model asset's roots go into Workspace as they are, not wrapped, so a
//!   MeshPart asset lands as the MeshPart (see `mcp::toolbox_fetch`);
//! - they land on whatever is under the centre of the view, their bounding
//!   box's bottom on the hit point; with nothing there, the box's centre
//!   sits 64 studs out along the view's centre line;
//! - an image becomes a `Decal` on the face under the centre of the view;
//! - what was inserted is selected, as one undo step;
//! - an asset holding scripts asks to have them reviewed, and Cancel takes
//!   the asset back out.
//!
//! The download needs the Open Cloud key (anonymous asset delivery refuses
//! store models); without one the card says how to add it.

mod view;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use glam::Vec3;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use rbx_cloud::StoreItem;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_viewer::pick::{self, PartSurface, Ray};

use super::{clipboard, export, Shell};

/// Results asked of one search.
const SEARCH_LIMIT: u32 = 30;
/// How far out along the view's centre line an asset lands when nothing is
/// under it, as Studio places it (the same 64 studs for a 3-stud coin and a
/// 40-stud tree).
const OPEN_AIR_DISTANCE: f32 = 64.0;
const SCRIPT_CLASSES: [&str; 3] = ["Script", "LocalScript", "ModuleScript"];

/// The kinds of asset the Toolbox lists, as Studio's Creator Store
/// filters them; plugins are installed from Manage Plugins instead.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(super) enum Category {
    #[default]
    Models,
    Meshes,
    Images,
    Audio,
    Videos,
}

impl Category {
    pub(super) const ALL: [Category; 5] = [
        Category::Models,
        Category::Meshes,
        Category::Images,
        Category::Audio,
        Category::Videos,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Category::Models => "Models",
            Category::Meshes => "Meshes",
            Category::Images => "Images",
            Category::Audio => "Audio",
            Category::Videos => "Videos",
        }
    }

    /// The store's `searchCategoryType`.
    fn store(self) -> &'static str {
        match self {
            Category::Models => "Model",
            Category::Meshes => "MeshPart",
            Category::Images => "Decal",
            Category::Audio => "Audio",
            Category::Videos => "Video",
        }
    }
}

#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub(super) enum Load {
    #[default]
    Idle,
    Loading,
    Ready,
    Failed(String),
}

/// An inserted asset that holds scripts, waiting on OK or Cancel.
pub(super) struct ScriptNotice {
    pub(super) name: String,
    pub(super) scripts: usize,
}

#[derive(Default)]
pub(crate) struct ToolboxDock {
    pub(super) category: Category,
    pub(super) query: Option<Entity<InputState>>,
    _query_entered: Option<Subscription>,
    /// What the shown results were searched for; empty lists the
    /// category's most popular.
    pub(super) text: String,
    pub(super) ids: Vec<u64>,
    pub(super) load: Load,
    generation: u64,
    /// The store's listing of every asset shown so far.
    pub(super) items: HashMap<u64, StoreItem>,
    /// `None` once a thumbnail failed, so the glyph stays.
    pub(super) icons: HashMap<u64, Option<Arc<RenderImage>>>,
    icons_asked: HashSet<u64>,
    /// The asset downloading; one at a time, as in Studio.
    pub(super) busy: Option<u64>,
    /// The last insert's failure, under the results.
    pub(super) error: Option<String>,
    pub(super) notice: Option<ScriptNotice>,
}

impl Shell {
    /// The search field, made the first time the dock shows; the first show
    /// also lists the category's most popular.
    pub(super) fn ensure_toolbox_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.toolbox.query.is_some() {
            return;
        }
        let query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search the Creator Store"));
        let entered = cx.subscribe(&query, |shell, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                shell.toolbox_search(cx);
            }
        });
        self.toolbox.query = Some(query);
        self.toolbox._query_entered = Some(entered);
        self.toolbox_search(cx);
    }

    pub(super) fn focus_toolbox_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_toolbox_query(window, cx);
        if let Some(query) = &self.toolbox.query {
            query.update(cx, |state, cx| state.focus(window, cx));
        }
    }

    pub(super) fn toolbox_set_category(&mut self, category: Category, cx: &mut Context<Self>) {
        if self.toolbox.category != category {
            self.toolbox.category = category;
            self.toolbox_search(cx);
        }
    }

    /// Searches the current category for the field's text.
    pub(super) fn toolbox_search(&mut self, cx: &mut Context<Self>) {
        let text = self
            .toolbox
            .query
            .as_ref()
            .map(|query| query.read(cx).value().trim().to_owned())
            .unwrap_or_default();
        let category = self.toolbox.category;
        let toolbox = &mut self.toolbox;
        toolbox.generation += 1;
        toolbox.text = text.clone();
        toolbox.ids.clear();
        toolbox.error = None;
        toolbox.load = Load::Loading;
        let generation = toolbox.generation;
        cx.spawn(async move |this, cx| {
            let reply = cx
                .background_spawn(async move {
                    let client = crate::shell::plugins::store_client();
                    let ids = client.search_store(category.store(), &text, SEARCH_LIMIT)?;
                    let items = client.store_items(&ids)?;
                    Ok::<_, rbx_cloud::CloudError>((ids, items))
                })
                .await;
            let _ = this.update(cx, |shell, cx| {
                if shell.toolbox.generation != generation {
                    return;
                }
                match reply {
                    Ok((ids, items)) => {
                        // Paid assets can't be inserted with a key: delivery
                        // only serves what is free or owned.
                        let free: HashSet<u64> = items
                            .iter()
                            .filter(|item| item.free)
                            .map(|item| item.id)
                            .collect();
                        let ids: Vec<u64> =
                            ids.into_iter().filter(|id| free.contains(id)).collect();
                        let toolbox = &mut shell.toolbox;
                        toolbox
                            .items
                            .extend(items.into_iter().map(|item| (item.id, item)));
                        toolbox.ids = ids.clone();
                        toolbox.load = Load::Ready;
                        shell.toolbox_icons(&ids, cx);
                    }
                    Err(err) => shell.toolbox.load = Load::Failed(err.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn toolbox_icons(&mut self, ids: &[u64], cx: &mut Context<Self>) {
        let toolbox = &mut self.toolbox;
        let wanted: Vec<u64> = ids
            .iter()
            .copied()
            .filter(|id| toolbox.icons_asked.insert(*id))
            .collect();
        if wanted.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let icons = cx
                .background_spawn(async move { crate::shell::plugins::store_icons(&wanted) })
                .await;
            let _ = this.update(cx, |shell, cx| {
                shell.toolbox.icons.extend(icons);
                cx.notify();
            });
        })
        .detach();
    }

    /// Downloads `id` off the UI thread, then inserts it.
    pub(super) fn toolbox_insert(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.toolbox.busy.is_some() || self.toolbox.notice.is_some() {
            return;
        }
        self.toolbox.busy = Some(id);
        self.toolbox.error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let fetched = cx
                .background_spawn(async move { crate::mcp::toolbox_fetch(id) })
                .await;
            let _ = this.update(cx, |shell, cx| {
                shell.toolbox.busy = None;
                // The card's own name beats the fetch's fallback when the
                // details route answers nothing.
                let placed = fetched.and_then(|(content, name)| {
                    let name = shell
                        .toolbox
                        .items
                        .get(&id)
                        .map_or(name, |item| item.name.clone());
                    shell.toolbox_place(&content, name, cx)
                });
                if let Err(err) = placed {
                    shell.toolbox.error = Some(err);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Grafts `content`'s roots in as one undo step, placed and selected.
    fn toolbox_place(
        &mut self,
        content: &WeakDom,
        name: String,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.live_shown() {
            return Err("Switch back to the edited place to insert.".to_owned());
        }
        let workspace =
            export::workspace(&self.dom).ok_or("The place has no Workspace to insert into.")?;
        // Aimed before grafting, so the ray can't find the asset itself.
        let aim = self.toolbox_aim(cx);
        let decal = content
            .root_refs()
            .iter()
            .all(|&root| content.get(root).is_some_and(|i| i.class() == "Decal"));
        let parent = match (decal, aim.part) {
            (true, Some(part)) => part,
            _ => workspace,
        };
        self.dom.take_changes();
        let before = self.dom.clone();
        let roots: Vec<Ref> = content
            .root_refs()
            .iter()
            .filter_map(|&root| clipboard::graft(&mut self.dom, content, root, parent))
            .collect();
        if roots.is_empty() {
            return Err(format!("{name} holds nothing to insert."));
        }
        if decal {
            // On the face the view looks at, as Studio does.
            if let Some(face) = aim
                .part
                .zip(aim.normal)
                .and_then(|(part, normal)| face_of(&self.dom, part, normal))
            {
                for &root in &roots {
                    let _ = self.dom.set_property(root, "Face", Variant::Enum(face));
                }
            }
        } else {
            land(&mut self.dom, &roots, &aim);
        }
        let scripts = count_scripts(&self.dom, &roots);
        self.push_history_snapshot(before);
        let changes = self.dom.take_changes();
        self.rebuild_explorer(cx);
        self.reselect(roots, cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        if scripts > 0 {
            self.toolbox.notice = Some(ScriptNotice { name, scripts });
        }
        cx.notify();
        Ok(())
    }

    /// The view's centre line and what it meets first.
    fn toolbox_aim(&self, cx: &App) -> Aim {
        let view = self.viewport.read(cx);
        let Some(pose) = view.pose() else {
            return Aim::default();
        };
        let ray = Ray::new(pose.position, pose.basis().0);
        let meshes = view.meshes().clone();
        let hit = pick::parts_along(&self.dom, &self.database, &meshes, ray)
            .into_iter()
            .find_map(|part| {
                let (distance, normal) =
                    PartSurface::read(&self.dom, &self.database, &meshes, part)?.raycast(ray)?;
                Some((part, distance, normal))
            });
        Aim {
            ray: Some(ray),
            part: hit.map(|(part, _, _)| part),
            distance: hit.map(|(_, distance, _)| distance),
            normal: hit.map(|(_, _, normal)| normal),
        }
    }

    /// OK on the scripts notice: the asset stays.
    pub(super) fn toolbox_keep(&mut self, cx: &mut Context<Self>) {
        self.toolbox.notice = None;
        cx.notify();
    }

    /// Cancel on the scripts notice: the insert is undone.
    pub(super) fn toolbox_take_back(&mut self, cx: &mut Context<Self>) {
        self.toolbox.notice = None;
        self.undo(cx);
    }
}

/// Where the view's centre line runs and what part it hits, if any.
#[derive(Default)]
struct Aim {
    ray: Option<Ray>,
    part: Option<Ref>,
    distance: Option<f32>,
    /// The hit surface's world normal.
    normal: Option<Vec3>,
}

/// Moves the parts under `roots` as one block onto `aim`: the block's
/// bottom centre on the hit point, or its centre out in the open.
fn land(dom: &mut WeakDom, roots: &[Ref], aim: &Aim) {
    let Some(ray) = aim.ray else {
        return;
    };
    // Every part, a part's own child parts included.
    let parts: Vec<Ref> = roots
        .iter()
        .flat_map(|&root| subtree(dom, root))
        .filter(|&r| pick::model_of(dom, r).is_some())
        .collect();
    let Some((min, max)) = parts
        .iter()
        .filter_map(|&part| pick::model_of(dom, part).map(bounds))
        .reduce(|(a_min, a_max), (b_min, b_max)| (a_min.min(b_min), a_max.max(b_max)))
    else {
        return;
    };
    let centre = (min + max) * 0.5;
    let offset = match aim.distance {
        Some(distance) => {
            let point = ray.origin + ray.direction * distance;
            point - Vec3::new(centre.x, min.y, centre.z)
        }
        None => ray.origin + ray.direction * OPEN_AIR_DISTANCE - centre,
    };
    // A model's stored pivot moves with its parts, or it would be left
    // behind where the asset was published.
    let pivots: Vec<Ref> = roots
        .iter()
        .flat_map(|&root| subtree(dom, root))
        .filter(|&r| {
            dom.get(r)
                .is_some_and(|i| i.properties().contains_key("WorldPivotData"))
        })
        .collect();
    for r in parts.into_iter().chain(pivots) {
        for (name, optional) in [("CFrame", false), ("WorldPivotData", true)] {
            let shifted = match dom.get(r).and_then(|i| i.properties().get(name)) {
                Some(Variant::CFrame(cframe)) if !optional => {
                    Variant::CFrame(shift(*cframe, offset))
                }
                Some(Variant::OptionalCFrame(Some(cframe))) if optional => {
                    Variant::OptionalCFrame(Some(shift(*cframe, offset)))
                }
                _ => continue,
            };
            let _ = dom.set_property(r, name, shifted);
        }
    }
}

fn shift(mut cframe: rbx_dom::CFrameData, offset: Vec3) -> rbx_dom::CFrameData {
    cframe.position.x += offset.x;
    cframe.position.y += offset.y;
    cframe.position.z += offset.z;
    cframe
}

/// The `NormalId` of `part`'s face whose outward axis is closest to the
/// world `normal`: Right, Top, Back, Left, Bottom, Front.
fn face_of(dom: &WeakDom, part: Ref, normal: Vec3) -> Option<u32> {
    let model = pick::model_of(dom, part)?;
    let axes =
        [model.x_axis, model.y_axis, model.z_axis].map(|axis| axis.truncate().normalize_or_zero());
    let (index, dot) = axes
        .iter()
        .map(|axis| axis.dot(normal))
        .enumerate()
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))?;
    Some(if dot >= 0. {
        index as u32
    } else {
        index as u32 + 3
    })
}

/// A part's world-aligned box from its scaled model matrix.
fn bounds(model: glam::Mat4) -> (Vec3, Vec3) {
    let half = (model.x_axis.truncate().abs()
        + model.y_axis.truncate().abs()
        + model.z_axis.truncate().abs())
        * 0.5;
    let centre = model.w_axis.truncate();
    (centre - half, centre + half)
}

/// `root` and everything under it.
fn subtree(dom: &WeakDom, root: Ref) -> Vec<Ref> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(r) = stack.pop() {
        if let Some(instance) = dom.get(r) {
            out.push(r);
            stack.extend(instance.children().iter().copied());
        }
    }
    out
}

fn count_scripts(dom: &WeakDom, roots: &[Ref]) -> usize {
    roots
        .iter()
        .flat_map(|&root| subtree(dom, root))
        .filter(|&r| {
            dom.get(r)
                .is_some_and(|i| SCRIPT_CLASSES.contains(&i.class()))
        })
        .count()
}

#[cfg(test)]
mod tests;
