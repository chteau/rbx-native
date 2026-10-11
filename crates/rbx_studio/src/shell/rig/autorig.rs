//! The auto-rigger in the editor: an unrigged character mesh becomes an R15
//! rig in the place (`rbx_autorig` fits the skeleton and cuts the 15 body
//! parts). The parts are written into the local asset cache as `version
//! 2.00` meshes and the rig is built from them through the Rig Builder, so
//! the 3D view and the Animation Editor work on it at once, before anything
//! is uploaded. Moving a joint, turning the front or mirroring cuts the
//! parts again and rebuilds the rig in place. Publish uploads the 15 parts
//! as one model, through the same Open Cloud import as Model › Import 3D
//! Model…, and points each part at its real mesh.

use std::path::{Path, PathBuf};

use glam::Vec3;
use gpui_kit::{AppContext as _, Context, PathPromptOptions};
use rbx_autorig::{Body, Fit, FitOptions, Joint, Mesh, Orientation, Part, Side};
use rbx_dom::{Ref, Variant, WeakDom};

use crate::command_bar::Feedback;
use crate::explorer;
use crate::shell::Shell;

use super::bundle::{Family, Piece, Pieces};
use super::insert::spawn_top;
use super::{build_rig, BodyScale, BodyShape, JointStyle, RigOptions, RigType};

const SOURCE: &str = "Auto-rig";
/// Where the cut parts live in the local asset cache, under a folder per
/// cut named by its content, so a re-cut never shows a stale mesh.
const CACHE_FOLDER: &str = "rbx-native-local/autorig";
/// How tall the rig stands: the Classic body's height, which the Animation
/// Editor's example clips are keyed for.
pub(crate) const HEIGHT: f32 = 5.2;

/// A mesh being rigged, and the rig made of it so far.
pub(crate) struct AutoRig {
    pub(crate) source: PathBuf,
    mesh: Mesh,
    pub(crate) fit: Fit,
    /// Mirror every joint edit to the other side.
    pub(crate) symmetric: bool,
    /// The joint the panel's fields edit.
    pub(crate) joint: Joint,
    /// The rig built from the last cut.
    pub(crate) model: Option<Ref>,
    pub(crate) warnings: Vec<String>,
    /// A cut is running in the background.
    pub(crate) busy: bool,
    /// Skin each part to its neighbours' bones too, so a bend is smooth
    /// across the joint (Roblox's skinned bodies); otherwise each part
    /// stays rigid.
    pub(crate) smooth: bool,
}

/// One cut's output: the parts and the cache paths their meshes went to.
struct Cut {
    body: Body,
    uris: Vec<String>,
}

/// The `rbxasset://` path a part's mesh is cached under.
fn local_uri(folder: &str, part: Part) -> String {
    format!("rbxasset://{CACHE_FOLDER}/{folder}/{}.mesh", part.name())
}

/// Cuts `mesh` along `fit` and writes each part into the local asset cache.
fn cut(mesh: &Mesh, fit: &Fit, smooth: bool) -> Result<Cut, String> {
    let body = rbx_autorig::split(mesh, fit).map_err(|e| e.to_string())?;
    // A skinned part is a `version 4.00` mesh carrying its bones and
    // weights; a rigid one the plain `version 2.00`.
    let skin = smooth.then(|| rbx_autorig::weights(mesh, fit, &body));
    let meshes: Vec<Vec<u8>> = body
        .parts
        .iter()
        .map(|p| match &skin {
            Some(skin) => rbx_autorig::mesh_v4(&p.mesh, Some(skin.part(p.part))),
            None => rbx_autorig::mesh_v2(&p.mesh),
        })
        .collect();
    let hash = meshes.iter().fold(0u64, |h, bytes| {
        h.rotate_left(7) ^ crate::recovery::fnv1a(bytes)
    });
    let folder = format!("{hash:016x}");
    let cache = rbx_assets::AssetCache::new(None).map_err(|e| e.to_string())?;
    let mut uris = Vec::with_capacity(meshes.len());
    for (part, bytes) in body.parts.iter().zip(&meshes) {
        let uri = local_uri(&folder, part.part);
        let relative = uri.trim_start_matches("rbxasset://");
        cache
            .put_native(relative, bytes)
            .map_err(|e| format!("caching {}: {e}", part.part.name()))?;
        uris.push(uri);
    }
    Ok(Cut { body, uris })
}

fn v3(v: Vec3) -> [f32; 3] {
    v.to_array()
}

/// The Rig Builder's body parts for a cut: each part's own size, its
/// local mesh and its rig attachments. `HumanoidRootPart` stays stock.
fn pieces(cut: &Cut, mut stock: Pieces) -> Pieces {
    for (part, uri) in cut.body.parts.iter().zip(&cut.uris) {
        stock.insert(
            part.part.name().to_owned(),
            Piece {
                name: part.part.name().to_owned(),
                family: Family::Classic,
                mesh: 0,
                local: Some(uri.clone()),
                texture: None,
                surface: None,
                face: false,
                init: v3(part.size),
                original: v3(part.size),
                cage: None,
                rig: part
                    .attachments
                    .iter()
                    .map(|(name, at)| (name.clone(), v3(*at)))
                    .collect(),
                extra: part
                    .accessories
                    .iter()
                    .map(|(name, at)| (name.clone(), v3(*at)))
                    .collect(),
            },
        );
    }
    stock
}

/// The rig's name: the file's own.
fn name_of(path: &Path) -> String {
    path.file_stem()
        .map_or_else(|| "Rig".into(), |s| s.to_string_lossy().into_owned())
}

impl Shell {
    /// Animation Editor › Rigging › Auto-rig a mesh…: the file picker.
    pub(crate) fn choose_autorig(&mut self, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Auto-rig a mesh".into()),
        });
        cx.spawn(async move |shell, cx| {
            let Ok(Ok(Some(paths))) = prompt.await else {
                return;
            };
            if let Some(path) = paths.into_iter().next() {
                let _ = shell.update(cx, |shell, cx| shell.start_autorig(path, cx));
            }
        })
        .detach();
    }

    /// Reads `path`, fits the skeleton and builds the first rig, in the
    /// background.
    pub(crate) fn start_autorig(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.autorig_report(Ok(format!("Rigging {}\u{2026}", name_of(&path))), cx);
        let source = path.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let mesh = rbx_autorig::read(&path).map_err(|e| e.to_string())?;
                    let fit = rbx_autorig::fit(&mesh, HEIGHT).map_err(|e| e.to_string())?;
                    let cut = cut(&mesh, &fit, true)?;
                    Ok::<_, String>((mesh, fit, cut))
                })
                .await;
            let _ = this.update(cx, |shell, cx| match result {
                Ok((mesh, fit, cut)) => {
                    let warnings = fit.warnings.clone();
                    shell.anim_set_autorig(Some(AutoRig {
                        source,
                        mesh,
                        fit,
                        symmetric: true,
                        joint: Joint::Neck,
                        model: None,
                        warnings,
                        busy: false,
                        smooth: true,
                    }));
                    shell.place_autorig(cut, cx);
                }
                Err(err) => shell.autorig_report(Err(err), cx),
            });
        })
        .detach();
    }

    /// Moves one joint (and its mirror, when symmetric), then re-cuts.
    pub(crate) fn autorig_move_joint(&mut self, joint: Joint, to: Vec3, cx: &mut Context<Self>) {
        let Some(rig) = self.anim_autorig_mut() else {
            return;
        };
        rig.fit.set_joint(joint, to);
        if rig.symmetric {
            match joint.side() {
                Some(side) => rbx_autorig::mirror(&mut rig.fit, side),
                None => rbx_autorig::mirror(&mut rig.fit, Side::Left),
            }
        }
        self.recut_autorig(cx);
    }

    /// Turns the mesh's front a quarter (or, with `2`, a half) turn and
    /// fits again: Avatar Setup's "Manually Align Front".
    pub(crate) fn autorig_turn(&mut self, quarter_turns: i32, cx: &mut Context<Self>) {
        let Some(rig) = self.anim_autorig_mut() else {
            return;
        };
        let orientation: Orientation = rig.fit.orientation.turned(quarter_turns);
        let options = FitOptions {
            height_studs: HEIGHT,
            orientation: Some(orientation),
            symmetric: rig.symmetric,
            ..FitOptions::default()
        };
        match rbx_autorig::fit_with(&rig.mesh, &options) {
            Ok(fit) => {
                rig.warnings = fit.warnings.clone();
                rig.fit = fit;
                self.recut_autorig(cx);
            }
            Err(err) => self.autorig_report(Err(err.to_string()), cx),
        }
    }

    /// Cuts the parts again for the current fit, in the background, and
    /// rebuilds the rig.
    pub(crate) fn recut_autorig(&mut self, cx: &mut Context<Self>) {
        let Some(rig) = self.anim_autorig_mut() else {
            return;
        };
        if rig.busy {
            return;
        }
        rig.busy = true;
        let (mesh, fit, smooth) = (rig.mesh.clone(), rig.fit.clone(), rig.smooth);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { cut(&mesh, &fit, smooth) })
                .await;
            let _ = this.update(cx, |shell, cx| {
                if let Some(rig) = shell.anim_autorig_mut() {
                    rig.busy = false;
                }
                match result {
                    Ok(cut) => shell.place_autorig(cut, cx),
                    Err(err) => shell.autorig_report(Err(err), cx),
                }
            });
        })
        .detach();
    }

    /// Replaces the previous cut's rig (if any) with one built from `cut`,
    /// and binds the Animation Editor to it.
    fn place_autorig(&mut self, cut: Cut, cx: &mut Context<Self>) {
        let Some(workspace) = explorer::find_by_name(&self.dom, "Workspace") else {
            return self.autorig_report(Err("The place has no Workspace".into()), cx);
        };
        let Some((source, old)) = self.anim_autorig().map(|r| (r.source.clone(), r.model)) else {
            return;
        };
        let joints = JointStyle::of_place(&self.dom);
        let mut options = RigOptions::new(
            RigType::R15,
            BodyShape::Masculine,
            BodyScale::Classic,
            joints,
        );
        options.name = name_of(&source);
        options.pieces = pieces(&cut, std::mem::take(&mut options.pieces));
        options.feet = spawn_top(&self.dom, workspace);
        self.push_history();
        let replacing = old.is_some_and(|m| self.dom.get(m).is_some());
        if let Some(old) = old.filter(|&m| self.dom.get(m).is_some()) {
            self.unbind_rig_for_replace(old, cx);
            self.dom.remove(old);
        }
        let model = build_rig(&mut self.dom, &options, workspace);
        let changes = self.dom.take_changes();
        self.rebuild_explorer(cx);
        self.reselect(vec![model], cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        let mut notes = cut.body.warnings.clone();
        if let Some(rig) = self.anim_autorig_mut() {
            rig.model = Some(model);
            notes.extend(rig.warnings.iter().cloned());
        }
        for note in notes {
            self.output.push(SOURCE, Feedback::Warning(note));
        }
        self.bind_rig_framed(model, !replacing, cx);
        self.resume_debug_steps(cx);
        let triangles = cut.body.triangle_count();
        self.autorig_report(
            Ok(format!(
                "Rigged {}: 15 parts, {triangles} triangles. Adjust the joints, then Publish.",
                options.name
            )),
            cx,
        );
    }

    /// Uploads the 15 parts as one model and points each part of the rig at
    /// its uploaded mesh, keeping the rig's own joints and attachments.
    pub(crate) fn publish_autorig(&mut self, cx: &mut Context<Self>) {
        let Some(rig) = self.anim_autorig() else {
            return;
        };
        let Some(model) = rig.model else {
            return;
        };
        let (mesh, fit, name, smooth) = (
            rig.mesh.clone(),
            rig.fit.clone(),
            name_of(&rig.source),
            rig.smooth,
        );
        self.autorig_report(Ok(format!("Publishing {name}\u{2026}")), cx);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let body = rbx_autorig::split(&mesh, &fit).map_err(|e| e.to_string())?;
                    let file = std::env::temp_dir().join(format!("{name}.glb"));
                    // Skinned, the upload carries the R15 armature and every
                    // part's weights; rigid, just the 15 meshes.
                    let bytes = if smooth {
                        let skin = rbx_autorig::weights(&mesh, &fit, &body);
                        rbx_autorig::gltf_skinned(&body, &skin)
                    } else {
                        rbx_autorig::gltf(&body)
                    };
                    std::fs::write(&file, bytes).map_err(|e| format!("{}: {e}", file.display()))?;
                    let imported = crate::shell::import3d::upload_model(&file, &name);
                    let _ = std::fs::remove_file(&file);
                    imported
                })
                .await;
            let _ = this.update(cx, |shell, cx| match result {
                Ok(imported) => {
                    let swapped = swap_meshes(&mut shell.dom, model, &imported.dom);
                    let changes = shell.dom.take_changes();
                    shell.reflect_changes(&changes, cx);
                    shell.record_history_change(changes);
                    let report = if swapped == 15 {
                        Ok("Published: every part now uses its uploaded mesh".to_owned())
                    } else {
                        Err(format!(
                            "Published, but only {swapped} of 15 parts were found in the upload"
                        ))
                    };
                    shell.autorig_report(report, cx);
                }
                Err(err) => shell.autorig_report(Err(err), cx),
            });
        })
        .detach();
    }

    /// Leaves the auto-rigger, keeping the rig it made.
    pub(crate) fn finish_autorig(&mut self, cx: &mut Context<Self>) {
        self.anim_set_autorig(None);
        cx.notify();
    }

    fn autorig_report(&mut self, result: Result<String, String>, cx: &mut Context<Self>) {
        let feedback = match result {
            Ok(message) => Feedback::Output(message),
            Err(message) => Feedback::Error(message),
        };
        self.output.push_once(SOURCE, feedback.clone());
        self.command_bar.set_feedback(feedback);
        cx.notify();
    }
}

/// Points each body part of `model` at the mesh the upload made of it: the
/// imported `MeshPart` named `<Part>_Geo` (or plain `<Part>`). Returns how
/// many parts were found.
fn swap_meshes(dom: &mut WeakDom, model: Ref, imported: &WeakDom) -> usize {
    let mut found = 0;
    for part in Part::ALL {
        let target = dom.get(model).and_then(|m| {
            m.children()
                .iter()
                .copied()
                .find(|&c| dom.get(c).is_some_and(|i| i.name() == part.name()))
        });
        let Some(target) = target else {
            continue;
        };
        let source = find_named(
            imported,
            &[format!("{}_Geo", part.name()), part.name().into()],
        );
        let Some(source) = source.and_then(|s| imported.get(s)) else {
            continue;
        };
        let Some(mesh) = source.properties().get("MeshId").cloned() else {
            continue;
        };
        let _ = dom.set_property(target, "MeshId", mesh);
        if let Some(size) = source.properties().get("InitialSize").cloned() {
            let _ = dom.set_property(target, "InitialSize", size);
        }
        found += 1;
    }
    found
}

fn find_named(dom: &WeakDom, names: &[String]) -> Option<Ref> {
    let mut pending: Vec<Ref> = dom.root_refs().to_vec();
    while let Some(next) = pending.pop() {
        let instance = dom.get(next)?;
        if instance.class() == "MeshPart" && names.iter().any(|n| n == instance.name()) {
            return Some(next);
        }
        pending.extend(instance.children());
    }
    None
}

/// Whether any `MeshId` under `root` still points at the local cache, which
/// Roblox cannot load: the rig has not been published yet.
pub(crate) fn has_local_meshes(dom: &WeakDom, root: Ref) -> bool {
    let mut pending = vec![root];
    while let Some(next) = pending.pop() {
        let Some(instance) = dom.get(next) else {
            continue;
        };
        if let Some(Variant::Content(rbx_dom::Content::Uri(uri))) =
            instance.properties().get("MeshId")
        {
            if uri.contains(CACHE_FOLDER) {
                return true;
            }
        }
        pending.extend(instance.children());
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_autorig::test_humanoid::{humanoid, Pose};

    /// The rig the Rig Builder makes of a cut, built at the origin.
    fn rig_of(cut: &Cut) -> (WeakDom, Ref) {
        let mut options = RigOptions::new(
            RigType::R15,
            BodyShape::Masculine,
            BodyScale::Classic,
            JointStyle::Motor6D,
        );
        options.pieces = pieces(cut, std::mem::take(&mut options.pieces));
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let model = build_rig(&mut dom, &options, workspace);
        (dom, model)
    }

    #[test]
    fn the_rig_puts_every_joint_where_the_fit_did() {
        let body = humanoid(Pose::A);
        let fit = rbx_autorig::fit(&body.mesh, HEIGHT).unwrap();
        let cut = Cut {
            body: rbx_autorig::split(&body.mesh, &fit).unwrap(),
            uris: Part::ALL.iter().map(|&p| local_uri("test", p)).collect(),
        };
        let (dom, model) = rig_of(&cut);
        let rig = rbx_animation::Rig::read(&dom, model);
        assert_eq!(rig.joints.len(), 15);
        let root = rig
            .root
            .and_then(|r| match dom.get(r)?.properties().get("CFrame")? {
                Variant::CFrame(c) => Some(rbx_animation::Frame::from_data(c)),
                _ => None,
            });
        let world = rig.layout(root.unwrap(), &|_| None);
        // Joint positions agree once both are measured from the root joint
        // (the Rig Builder stands the rig on the place, the fit on y = 0).
        let pivot = |name: &str| {
            let j = rig.joint_for(name).unwrap();
            j.pivot(&world[&j.part0]).position
        };
        let origin = pivot("LowerTorso") - fit.joint(Joint::Root);
        for joint in Joint::ALL {
            let built = pivot(joint.part().name()) - origin;
            let fitted = fit.joint(joint);
            assert!(
                built.distance(fitted) < 1e-3,
                "{}: built at {built}, fitted at {fitted}",
                joint.name()
            );
        }
        // Every part shows its cached mesh.
        let hand = rig.joint_for("LeftHand").unwrap().part1;
        assert_eq!(
            dom.get(hand).unwrap().properties().get("MeshId"),
            Some(&Variant::Content(rbx_dom::Content::Uri(local_uri(
                "test",
                Part::LeftHand
            ))))
        );
        assert!(has_local_meshes(&dom, model));
    }

    #[test]
    fn local_uris_name_the_part_under_the_cut() {
        assert_eq!(
            local_uri("00ff", Part::LeftHand),
            "rbxasset://rbx-native-local/autorig/00ff/LeftHand.mesh"
        );
    }
}
