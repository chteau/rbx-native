//! Beta features: real Studio's File › Beta Features. A half-finished
//! feature ships switched off and asks [`enabled`] before it shows itself;
//! the user switches it on in Settings › Beta features.
//!
//! A switch applies after a restart, as in Studio: [`enabled`] answers from
//! the choices as they stood at launch ([`init`]), while the Settings page
//! edits the stored choices (`<config>/beta.json`,
//! `{"enabled": ["<id>", …]}`) and counts what a restart would change.
//!
//! Adding one: a [`Feature`] in [`FEATURES`], then `beta::enabled("<id>")`
//! at the place the feature appears. Remove both once it graduates.

use crate::settings::default_config_dir;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static LAUNCH: OnceLock<Choices> = OnceLock::new();
static STORED: Mutex<Option<Choices>> = Mutex::new(None);

fn stored_path() -> Option<PathBuf> {
    default_config_dir().map(|dir| dir.join("beta.json"))
}

fn with_stored<T>(f: impl FnOnce(&mut Choices) -> T) -> T {
    let mut guard = STORED.lock().unwrap_or_else(|e| e.into_inner());
    let choices = guard.get_or_insert_with(|| match stored_path() {
        Some(path) => Choices::load(&path),
        None => Choices::default(),
    });
    f(choices)
}

/// One switchable feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Feature {
    /// The stable name `beta.json` stores.
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    /// One or two sentences: what it does and what is unfinished.
    pub(crate) description: &'static str,
}

/// What ships behind a switch today.
pub(crate) const FEATURES: &[Feature] = &[
    Feature {
        id: crate::shell::COEDIT_BETA_ID,
        name: "Live co-editing",
        description: "Shares edits live with others in the place's Team Presence room (hosted relay only). Remote edits are undo steps of their own; concurrent script edits are merged, with conflicts kept side by side.",
    },
    Feature {
        id: crate::update_planning::BETA_ID,
        name: "Update planning",
        description: "Countdown icons on autopilot: timers over the current icon, uploaded for review early, swapped in at their time by the editor or a background helper the OS runs while the PC is on. Not yet tried against Roblox: the icon swap needs the key's legacy-universe:manage permission.",
    },
    Feature {
        id: crate::shell::PSEUDO_LOC_BETA_ID,
        name: "Pseudo-localization",
        description: "Previews every GUI text longer and accented (and optionally right to left) in the 3D view and the UI Editor, lists what no longer fits, and writes the place's user-facing strings into a LocalizationTable. The preview never touches the place; overflow is measured, not yet suggested fixes.",
    },
    Feature {
        id: crate::shell::PERF_BETA_ID,
        name: "Performance heatmap: totals and models",
        description: "Adds Total and Physics to the Viewport dock's Performance view and ranks the costliest models with a breakdown of what each costs: triangles, texture memory, instances, physics, shadows and overdraw. The weights are an estimate, not Roblox's own costs.",
    },
    Feature {
        id: LUAU_EXPORTS_BETA_ID,
        name: "Luau Exports and Cyclic Requires",
        description: "export local, export const and export function in a ModuleScript, like Studio's beta of the same name: the module returns a table of what it exports, and two modules that export can require each other. Mixing export with a top-level return is a compile error there too.",
    },
    Feature {
        id: crate::camera::ORTHOGRAPHIC_BETA_ID,
        name: "Orthographic Camera",
        description: "The 3D view follows the place camera's ProjectionType, OrthographicSize and OrthographicMode, like Studio's beta of the same name: set workspace.CurrentCamera.ProjectionType to Orthographic and the view goes flat at that size, and zooming writes the size back.",
    },
    Feature {
        id: crate::workspace_view::camera_row::BETA_ID,
        name: "New Studio Camera Controls",
        description: "The edit camera flies like Studio's beta of the same name: Adaptive, FPS and Orbit modes under the View Selector, fly speed that follows the focus distance, wheel zoom towards the cursor, Alt + right drag to dolly, and Focus that glides in and frames the selection at 70% of the view.",
    },
    Feature {
        id: crate::transform::NEXT_GEN_DRAGGERS_BETA_ID,
        name: "Next-Gen Draggers",
        description: "Move and Rotate handles in the look of Studio's beta of the same name: its colours, smaller and thinner handles, positive arrows only with a square to drag along each plane, and rings drawn on their near half inside a white view ring, with a trackball inside them for a free turn.",
    },
    Feature {
        id: crate::explorer::insert::UI_PARTICLES_BETA_ID,
        name: "UI Particles",
        description: "Lists ParticleEmitter2D in the Explorer's insert picker and the UI Editor's +, like Studio's beta of the same name. Particles draw on screen, on the UI Editor canvas and on BillboardGuis and SurfaceGuis whether this is on or not, and scripts can Emit and Clear them.",
    },
    Feature {
        id: crate::shell::UGC_VISUAL_BETA_ID,
        name: "Visualizing UGC Validation",
        description: "Click a finding in Check Avatar Item and the 3D view draws where it failed on the item, in Studio's colours: open edges of a mesh that isn't watertight, and the vertices of a cage whose UVs moved off the template. Other checks have no spot to point at and draw nothing, as in Studio.",
    },
    Feature {
        id: crate::properties::MATERIAL_VARIANT_ALPHA_MODE_BETA_ID,
        name: "MaterialVariant Alpha Mode",
        description: "Lists AlphaMode on a MaterialVariant and draws each mode the way Studio's beta of the same name does: Overlay shows the part colour where the colour map is clear, TintMask tints only where it is opaque, and Transparency cuts the surface out below half alpha. Off, the alpha is ignored, as in Studio, but the property still saves.",
    },
    Feature {
        id: crate::shell::VIDEO_UPLOADS_BETA_ID,
        name: "Video Uploads",
        description: "Import takes .mp4 and .mov, like Studio's beta of the same name: Roblox's limits (5 minutes, 4096x2160, 3.75 GB) are checked first, the 2,000 Robux price is asked before anything goes up, and the video lands as a VideoFrame. Roblox takes videos only from 13+ ID-verified accounts.",
    },
    Feature {
        id: crate::shell::AUTO_REIMPORT_BETA_ID,
        name: "Auto-Reimport",
        description: "Reimport for imported 3D models, like Studio's beta of the same name: the Explorer's menu can reimport a model from its file, pick the file, and turn on Auto-Reimport, which reimports whenever the file is saved, for this session. Each reimport uploads the file again, so the meshes get new asset ids.",
    },
    Feature {
        id: SOLID_MESHES_BETA_ID,
        name: "Solid Modeling On Meshes",
        description: "Union, Intersect and Negate take MeshParts, like Studio's beta of the same name: a mesh carves as its own triangles, and one that isn't closed is refused in Studio's words. Studio makes a MeshPart that saves only as a box and can't be separated; this makes a union, so the shape saves and Separate gives the MeshPart back. Off, a MeshPart carves as its box.",
    },
    Feature {
        id: crate::shell::REQUIRE_GRAPH_BETA_ID,
        name: "Module require graph",
        description: "View › Require Graph draws every script and ModuleScript with an edge for each require it can resolve statically, and lists cycles, client scripts requiring server-only modules, requires it can't follow, and modules nothing requires.",
    },
    Feature {
        id: crate::shell::LOCAL_RUN_BETA_ID,
        name: "Local Run",
        description: "Our own clean-room Roblox runtime, no Roblox client needed. Test › Play (local), Run (local) and Server & Clients play the place right in the editor, with physics, humanoids, your avatar, GUI, sound and remotes, plus the Remote Inspector, Leak Finder, MicroProfiler, Load test and integration specs. Works offline. Close to the real thing, not exact, so real Play stays the reference.",
    },
    Feature {
        id: IF_LOCAL_BETA_ID,
        name: "If Local/Const Bindings",
        description: "if local, if const and while local/while const, like Studio's beta of the same name: a binding scoped to the block that runs only when its value is truthy. One name and one value for now, as in Studio. Script Analysis knows the syntax too.",
    },
];

/// Studio's "If Local/Const Bindings" beta, see `rbx_lua::set_if_local`.
pub(crate) const IF_LOCAL_BETA_ID: &str = "if-local";

/// `RBX_STUDIO_BETA_SAMPLE=1` lists this one too, so the page can be
/// exercised (and captured) while [`FEATURES`] is empty. It gates nothing.
pub(crate) const SAMPLE: Feature = Feature {
    id: "sample",
    name: "Sample feature",
    description: "Gates nothing. Listed only when RBX_STUDIO_BETA_SAMPLE is set, to try the page.",
};

/// The stored choices. Pure; the app's copies are behind the functions
/// below.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Choices {
    pub(crate) enabled: BTreeSet<String>,
}

impl Choices {
    /// Malformed text is no choices.
    pub(crate) fn from_json(text: &str) -> Choices {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
            return Choices::default();
        };
        let enabled = value
            .get("enabled")
            .and_then(serde_json::Value::as_array)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| id.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        Choices { enabled }
    }

    pub(crate) fn to_json(&self) -> String {
        let ids: Vec<&str> = self.enabled.iter().map(String::as_str).collect();
        serde_json::json!({ "enabled": ids }).to_string()
    }

    /// A missing or unreadable file is no choices.
    pub(crate) fn load(path: &Path) -> Choices {
        std::fs::read_to_string(path)
            .map(|text| Choices::from_json(&text))
            .unwrap_or_default()
    }

    /// Temp file then rename, creating the directory.
    pub(crate) fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, self.to_json())?;
        std::fs::rename(&tmp, path)
    }

    pub(crate) fn is_on(&self, id: &str) -> bool {
        self.enabled.contains(id)
    }

    pub(crate) fn set(&mut self, id: &str, on: bool) {
        if on {
            self.enabled.insert(id.to_owned());
        } else {
            self.enabled.remove(id);
        }
    }

    /// Every listed feature back to its default, which is off. Ids stored
    /// for features that are no longer listed are left alone.
    pub(crate) fn reset(&mut self, listed: &[Feature]) {
        for feature in listed {
            self.set(feature.id, false);
        }
    }

    /// Ids of `listed` whose state differs between `self` and `other`.
    pub(crate) fn differences(&self, other: &Choices, listed: &[Feature]) -> Vec<&'static str> {
        listed
            .iter()
            .filter(|feature| self.is_on(feature.id) != other.is_on(feature.id))
            .map(|feature| feature.id)
            .collect()
    }
}

/// [`FEATURES`], plus [`SAMPLE`] when `RBX_STUDIO_BETA_SAMPLE` is set.
pub(crate) fn features() -> Vec<Feature> {
    let mut all = FEATURES.to_vec();
    if std::env::var("RBX_STUDIO_BETA_SAMPLE").is_ok_and(|value| !value.is_empty()) {
        all.push(SAMPLE);
    }
    all
}

/// Studio's "Luau Exports and Cyclic Requires" beta, see
/// `rbx_lua::set_luau_exports`.
pub(crate) const LUAU_EXPORTS_BETA_ID: &str = "luau-exports";
/// Studio's "Solid Modeling On Meshes" beta, see
/// `rbx_lua::csg::set_mesh_source`.
pub(crate) const SOLID_MESHES_BETA_ID: &str = "solid-modeling-on-meshes";

/// Reads `beta.json` once, at launch; later calls do nothing. Before it
/// runs, every feature is off.
pub(crate) fn init() {
    let choices = stored_path()
        .map(|path| Choices::load(&path))
        .unwrap_or_default();
    rbx_lua::set_if_local(choices.is_on(IF_LOCAL_BETA_ID));
    rbx_lua::set_luau_exports(choices.is_on(LUAU_EXPORTS_BETA_ID));
    // A mesh in view is already in the disk cache, so the read is local.
    rbx_lua::csg::set_mesh_source(
        choices.is_on(SOLID_MESHES_BETA_ID).then(|| {
            Box::new(|uri: &str| rbx_viewer::asset_bytes(uri)) as rbx_lua::csg::MeshSource
        }),
    );
    rbx_lua::set_orthographic_camera(choices.is_on(crate::camera::ORTHOGRAPHIC_BETA_ID));
    rbx_viewer::set_variant_alpha_modes(
        choices.is_on(crate::properties::MATERIAL_VARIANT_ALPHA_MODE_BETA_ID),
    );
    let _ = LAUNCH.set(choices);
}

/// Whether `id` is on for this run (the choice at launch).
pub(crate) fn enabled(id: &str) -> bool {
    LAUNCH.get().is_some_and(|choices| choices.is_on(id))
}

/// The stored choice, which may differ from [`enabled`] until a restart.
pub(crate) fn chosen(id: &str) -> bool {
    with_stored(|choices| choices.is_on(id))
}

/// Stores a choice; it applies at the next launch.
pub(crate) fn choose(id: &str, on: bool) -> std::io::Result<()> {
    let path =
        stored_path().ok_or_else(|| std::io::Error::other("no config directory for beta.json"))?;
    with_stored(|choices| {
        choices.set(id, on);
        choices.save(&path)
    })
}

/// Switches every listed feature back off, saved the way [`choose`] saves.
pub(crate) fn reset_all() -> std::io::Result<()> {
    let path =
        stored_path().ok_or_else(|| std::io::Error::other("no config directory for beta.json"))?;
    with_stored(|choices| {
        choices.reset(&features());
        choices.save(&path)
    })
}

/// How many listed features a restart would switch.
pub(crate) fn pending() -> usize {
    let launch = LAUNCH.get().cloned().unwrap_or_default();
    let stored = with_stored(|choices| choices.clone());
    launch.differences(&stored, &features()).len()
}

#[cfg(test)]
mod tests;
