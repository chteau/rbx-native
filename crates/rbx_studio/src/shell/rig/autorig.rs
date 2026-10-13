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

use glam::{Quat, Vec3};
use gpui_kit::{AppContext as _, Context, PathPromptOptions};
use rbx_animation::Frame;
use rbx_autorig::{Body, CageTemplate, Fit, FitOptions, Joint, Mesh, Orientation, Part, Side};
use rbx_dom::{Ref, Variant, WeakDom};

use crate::command_bar::Feedback;
use crate::explorer;
use crate::shell::Shell;
use crate::workspace_view::JointPhase;

use super::bundle::{Family, Piece, Pieces};
use super::cage_templates;
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
    /// The fit moved while a cut was running: cut again when it lands.
    stale: bool,
    /// Where the joint a marker drag holds stood when it was grabbed.
    grabbed: Option<Vec3>,
    /// Where the fit's origin stands in the place: the built rig's root
    /// joint less the fit's (the Rig Builder stands the rig on the place).
    origin: Vec3,
    /// The mesh's bounds under the fit, which a moved joint stays inside
    /// (see [`rbx_autorig::guard`]).
    bounds: (Vec3, Vec3),
    /// Each rig built so far, newest last, with the fit it was cut along:
    /// an undo or redo that brings one back brings its fit back with it.
    placed: Vec<Placed>,
}

/// A rig the auto-rigger built, and the fit and origin it stood on.
#[derive(Clone)]
struct Placed {
    model: Ref,
    fit: Fit,
    origin: Vec3,
}

/// How many built rigs [`AutoRig::placed`] remembers: more than the undo
/// history keeps.
const PLACED: usize = 256;

/// One cut's output: the parts, the cache paths their meshes went to, and
/// where the fit it was cut along put the root joint.
struct Cut {
    body: Body,
    uris: Vec<String>,
    root: Vec3,
    /// The fit it was cut along.
    fit: Fit,
}

/// The `rbxasset://` path a part's mesh is cached under.
fn local_uri(folder: &str, part: Part) -> String {
    format!("rbxasset://{CACHE_FOLDER}/{folder}/{}.mesh", part.name())
}

/// What Publish uploads, cached beside the parts' meshes under each cut's
/// folder: so a rig still pointing at those meshes can be published from
/// the model alone, after the auto-rigger was left or the place reopened.
const UPLOAD_FILE: &str = "upload.glb";

/// The same upload without the body cages, cached beside it when it has
/// them: Publish falls back to it should Roblox refuse the caged one, so a
/// cage the importer dislikes never costs the rig its meshes.
const PLAIN_FILE: &str = "upload-plain.glb";

/// The cache path (relative to the native cache) of the upload cut with the
/// local meshes under `root`, if any part still points at them.
fn local_upload(dom: &WeakDom, root: Ref) -> Option<String> {
    let mut pending = vec![root];
    while let Some(next) = pending.pop() {
        let Some(instance) = dom.get(next) else {
            continue;
        };
        if let Some(Variant::Content(rbx_dom::Content::Uri(uri))) =
            instance.properties().get("MeshId")
        {
            if let Some(relative) = uri.strip_prefix("rbxasset://") {
                if let Some((folder, _)) = relative
                    .strip_prefix(CACHE_FOLDER)
                    .and_then(|rest| rest.strip_prefix('/'))
                    .and_then(|rest| rest.split_once('/'))
                {
                    return Some(format!("{CACHE_FOLDER}/{folder}/{UPLOAD_FILE}"));
                }
            }
        }
        pending.extend(instance.children());
    }
    None
}

/// Cuts `mesh` along `fit` and writes each part into the local asset cache.
///
/// With `templates` the body also gets outer cages bent onto the fit, for
/// layered clothing; without them (`cage_error` says why) it is warned.
fn cut(
    mesh: &Mesh,
    fit: &Fit,
    smooth: bool,
    templates: Option<&[CageTemplate]>,
    cage_error: Option<String>,
) -> Result<Cut, String> {
    let mut body = rbx_autorig::split(mesh, fit).map_err(|e| e.to_string())?;
    let mut cages = Vec::new();
    let mut reason = cage_error;
    if let Some(templates) = templates {
        match rbx_autorig::fit_cages(&body, templates) {
            Ok(fitted) => cages = fitted,
            Err(e) => reason = Some(e.to_string()),
        }
    }
    if let Some(reason) = reason {
        body.warnings.push(format!(
            "This rig has no body cages, so layered clothing will not fit it: {reason}"
        ));
    }
    // A skinned part is a `version 4.00` mesh carrying its bones and
    // weights; a rigid one the plain `version 2.00`.
    let skin = smooth.then(|| rbx_autorig::weights(mesh, fit, &body));
    // Skinned, the upload carries the R15 armature and every part's
    // weights; rigid, just the 15 meshes.
    let write = |cages: &[Mesh]| match &skin {
        Some(skin) => rbx_autorig::gltf_skinned(&body, skin, cages),
        None => rbx_autorig::gltf(&body, cages),
    };
    let upload = write(&cages);
    let plain = (!cages.is_empty()).then(|| write(&[]));
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
    cache
        .put_native(&format!("{CACHE_FOLDER}/{folder}/{UPLOAD_FILE}"), &upload)
        .map_err(|e| format!("caching the upload: {e}"))?;
    if let Some(plain) = plain {
        cache
            .put_native(&format!("{CACHE_FOLDER}/{folder}/{PLAIN_FILE}"), &plain)
            .map_err(|e| format!("caching the upload: {e}"))?;
    }
    Ok(Cut {
        body,
        uris,
        root: fit.joint(Joint::Root),
        fit: fit.clone(),
    })
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
                    let cut = match cage_templates::mannequin() {
                        Ok(t) => cut(&mesh, &fit, true, Some(&t), None)?,
                        Err(e) => cut(&mesh, &fit, true, None, Some(e))?,
                    };
                    Ok::<_, String>((mesh, fit, cut))
                })
                .await;
            let _ = this.update(cx, |shell, cx| match result {
                Ok((mesh, fit, cut)) => {
                    let warnings = fit.warnings.clone();
                    shell.anim_set_autorig(Some(AutoRig {
                        source,
                        symmetric: true,
                        joint: Joint::Neck,
                        model: None,
                        warnings,
                        busy: false,
                        smooth: true,
                        stale: false,
                        grabbed: None,
                        origin: Vec3::ZERO,
                        bounds: rbx_autorig::mesh_bounds(&mesh, &fit),
                        placed: Vec::new(),
                        mesh,
                        fit,
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
        let guarded = self
            .anim_autorig_mut()
            .is_some_and(|rig| rig.set_joint(joint, to));
        if guarded {
            self.autorig_guarded(joint, cx);
        }
        self.recut_autorig(cx);
    }

    /// A drag on a joint marker in the 3D view (see
    /// `workspace_view::joints`): the press selects the joint, each step
    /// moves it (and its mirror) with the markers following, and the release
    /// cuts again, as a panel edit does — one rebuild, one undo step.
    pub(crate) fn autorig_drag_joint(
        &mut self,
        index: usize,
        to: Vec3,
        phase: JointPhase,
        cx: &mut Context<Self>,
    ) {
        let Some(&joint) = Joint::ALL.get(index) else {
            return;
        };
        let Some(rig) = self.anim_autorig_mut() else {
            return;
        };
        let at = to - rig.origin;
        match phase {
            JointPhase::Press => {
                rig.grabbed = Some(rig.fit.joint(joint));
                rig.joint = joint;
                self.anim_select_joint(joint.part().name());
            }
            JointPhase::Drag => {
                rig.set_joint(joint, at);
            }
            JointPhase::Release => {
                let guarded = rig.set_joint(joint, at);
                // A click on a marker only selects its joint.
                let from = rig.grabbed.take();
                if from.is_none_or(|from| from.distance(at) > 1e-4) {
                    if guarded {
                        self.autorig_guarded(joint, cx);
                    }
                    self.recut_autorig(cx);
                }
            }
        }
        self.sync_autorig_markers(cx);
        cx.notify();
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
                rig.bounds = rbx_autorig::mesh_bounds(&rig.mesh, &fit);
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
            rig.stale = true;
            return;
        }
        rig.busy = true;
        rig.stale = false;
        let (mesh, fit, smooth) = (rig.mesh.clone(), rig.fit.clone(), rig.smooth);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    match cage_templates::mannequin() {
                        Ok(t) => cut(&mesh, &fit, smooth, Some(&t), None),
                        Err(e) => cut(&mesh, &fit, smooth, None, Some(e)),
                    }
                })
                .await;
            let _ = this.update(cx, |shell, cx| {
                let stale = shell.anim_autorig_mut().is_some_and(|rig| {
                    rig.busy = false;
                    rig.stale
                });
                match result {
                    Ok(cut) => shell.place_autorig(cut, cx),
                    Err(err) => {
                        // Back to the fit the standing rig was cut along.
                        let restored = shell.anim_autorig_mut().is_some_and(AutoRig::restore);
                        let err = if restored {
                            format!("{err}. The joints went back to the last rig that cut.")
                        } else {
                            err
                        };
                        shell.autorig_report(Err(err), cx);
                        shell.sync_autorig_markers(cx);
                    }
                }
                // An edit made while this cut ran gets a cut of its own.
                if stale {
                    shell.recut_autorig(cx);
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
        let origin = root_pivot(&self.dom, model).map(|at| at - cut.root);
        let changes = self.dom.take_changes();
        self.rebuild_explorer(cx);
        self.reselect(vec![model], cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        let mut notes = cut.body.warnings.clone();
        if let Some(rig) = self.anim_autorig_mut() {
            rig.model = Some(model);
            rig.origin = origin.unwrap_or(rig.origin);
            rig.remember(model, cut.fit.clone());
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

    /// The rig Publish would upload: the one being auto-rigged, else the
    /// bound one, else the selected one, so long as its parts still point
    /// at the auto-rigger's local meshes.
    pub(crate) fn publishable_rig(&self) -> Option<Ref> {
        let candidates = [
            self.anim_autorig().and_then(|rig| rig.model),
            self.anim_bound_model(),
            self.selected_all()
                .first()
                .and_then(|&r| crate::anim_editor::place::rig_of(&self.dom, r)),
        ];
        candidates
            .into_iter()
            .flatten()
            .find(|&model| has_local_meshes(&self.dom, model))
    }

    /// Uploads the 15 parts of [`Shell::publishable_rig`] as one model and
    /// points each part at its uploaded mesh, keeping the rig's own joints
    /// and attachments. Needs only the model: the upload was cached beside
    /// its local meshes when it was cut.
    pub(crate) fn publish_autorig(&mut self, cx: &mut Context<Self>) {
        if self.anim.publishing {
            return;
        }
        let Some(model) = self.publishable_rig() else {
            return self.autorig_report(
                Err("No rig to publish: its parts already use uploaded meshes".into()),
                cx,
            );
        };
        let name = self
            .dom
            .get(model)
            .map(|m| m.name().to_owned())
            .unwrap_or_default();
        let Some(upload) = local_upload(&self.dom, model) else {
            return;
        };
        self.autorig_report(Ok(format!("Publishing {name}\u{2026}")), cx);
        self.anim.publishing = true;
        let model_name = name.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let cache = rbx_assets::AssetCache::new(None).map_err(|e| e.to_string())?;
                    let bytes = cache.get_native(&upload).ok_or_else(|| {
                        format!("{name}'s cut is no longer in the local cache: auto-rig the mesh again")
                    })?;
                    let send = |bytes: Vec<u8>| {
                        let file = std::env::temp_dir().join(format!("{name}.glb"));
                        std::fs::write(&file, bytes)
                            .map_err(|e| format!("{}: {e}", file.display()))?;
                        let imported = crate::shell::import3d::upload_model(&file, &name);
                        let _ = std::fs::remove_file(&file);
                        imported
                    };
                    let plain = upload.replace(UPLOAD_FILE, PLAIN_FILE);
                    match (send(bytes), cache.get_native(&plain)) {
                        (Ok(imported), plain) => Ok((imported, None, plain.is_some())),
                        (Err(err), Some(plain)) => send(plain).map(|imported| {
                            let note = format!(
                                "Roblox refused the upload with body cages ({err}), so it went without them: layered clothing will not fit this rig"
                            );
                            (imported, Some(note), false)
                        }).map_err(|_| err),
                        (Err(err), None) => Err(err),
                    }
                })
                .await;
            let _ = this.update(cx, |shell, cx| match result {
                Ok((imported, note, caged)) => {
                    let swapped = swap_meshes(&mut shell.dom, model, &imported.dom);
                    let note = note.or_else(|| {
                        (caged && wrap_targets(&shell.dom, model) == 0).then(|| {
                            "The upload carried body cages, but Roblox's import made no WrapTarget of them: layered clothing will not fit this rig".to_string()
                        })
                    });
                    if let Some(note) = note {
                        shell.output.push(SOURCE, Feedback::Warning(note));
                    }
                    let changes = shell.dom.take_changes();
                    shell.reflect_changes(&changes, cx);
                    shell.record_history_change(changes);
                    let id = imported.asset_id;
                    let report = if swapped == 15 {
                        Ok(format!(
                            "Published as model {id}: every part now uses its uploaded mesh"
                        ))
                    } else {
                        Err(format!(
                            "Published as model {id}, but only {swapped} of 15 parts were found in the upload"
                        ))
                    };
                    shell.anim.publishing = false;
                    shell.anim.publish_result = Some(super::PublishResult {
                        name: model_name,
                        outcome: super::PublishOutcome::Published { id, swapped },
                    });
                    shell.autorig_report(report, cx);
                }
                Err(err) => {
                    shell.anim.publishing = false;
                    shell.anim.publish_result = Some(super::PublishResult {
                        name: model_name,
                        outcome: super::PublishOutcome::Failed(err.clone()),
                    });
                    shell.autorig_report(Err(err), cx);
                }
            });
            let _ = this.update(cx, |shell, cx| shell.resume_debug_steps(cx));
        })
        .detach();
    }

    /// Leaves the auto-rigger, keeping the rig it made.
    pub(crate) fn finish_autorig(&mut self, cx: &mut Context<Self>) {
        self.anim_set_autorig(None);
        self.sync_autorig_markers(cx);
        cx.notify();
    }

    /// Says a joint stopped short of where it was asked to go.
    fn autorig_guarded(&mut self, joint: Joint, cx: &mut Context<Self>) {
        let note = format!(
            "{} stops where its limb ends: a joint stays between its neighbours and inside the mesh.",
            joint.name()
        );
        self.output.push_once(SOURCE, Feedback::Warning(note));
        cx.notify();
    }

    /// After an undo or redo replaced the place: the auto-rigger takes up
    /// the fit of whichever of its rigs is back (joints, markers and the
    /// panel's fields all read it) and the Animation Editor binds to that
    /// rig. Called by [`Shell::undo`] and [`Shell::redo`].
    pub(crate) fn autorig_follow_history(&mut self, cx: &mut Context<Self>) {
        let dom = &self.dom;
        let Some(rig) = self.anim.autorig.as_mut() else {
            return;
        };
        let Some((old, new)) = rig.follow(|r| dom.get(r).is_some()) else {
            return;
        };
        if let Some(old) = old {
            self.unbind_rig_for_replace(old, cx);
        }
        if let Some(new) = new {
            self.bind_rig_framed(new, false, cx);
        }
        self.sync_autorig_markers(cx);
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

impl AutoRig {
    /// Puts `joint` on `to` (fit space), or as near as the joint guard lets
    /// it (see [`rbx_autorig::guard`]), and its mirror too when symmetric.
    /// Whether the guard moved it off `to`.
    fn set_joint(&mut self, joint: Joint, to: Vec3) -> bool {
        let at = rbx_autorig::guard(&self.fit, joint, to, self.bounds);
        self.fit.set_joint(joint, at);
        if self.symmetric {
            match joint.side() {
                Some(side) => rbx_autorig::mirror(&mut self.fit, side),
                None => rbx_autorig::mirror(&mut self.fit, Side::Left),
            }
        }
        at.distance(to) > 1e-4
    }

    /// Notes that `model` was just built from `fit`, standing on the
    /// current origin.
    fn remember(&mut self, model: Ref, fit: Fit) {
        if self.placed.len() >= PLACED {
            self.placed.remove(0);
        }
        self.placed.push(Placed {
            model,
            fit,
            origin: self.origin,
        });
    }

    /// Puts the fit back to the one the standing rig was cut along, after a
    /// cut failed. Whether there was one.
    fn restore(&mut self) -> bool {
        let Some(placed) = self
            .placed
            .iter()
            .rev()
            .find(|p| Some(p.model) == self.model)
            .cloned()
        else {
            return false;
        };
        self.fit = placed.fit;
        self.origin = placed.origin;
        true
    }

    /// After an undo or redo replaced the place: when the rig the fit
    /// belongs to is gone, takes up the fit of the built rig that is back
    /// (the newest one `exists`), or none. Answers the rig left and the rig
    /// taken up when they differ.
    fn follow(&mut self, exists: impl Fn(Ref) -> bool) -> Option<(Option<Ref>, Option<Ref>)> {
        if self.model.is_some_and(&exists) {
            return None;
        }
        let back = self.placed.iter().rev().find(|p| exists(p.model)).cloned();
        let old = self.model;
        self.grabbed = None;
        match back {
            Some(placed) => {
                self.model = Some(placed.model);
                self.fit = placed.fit;
                self.origin = placed.origin;
            }
            None => self.model = None,
        }
        (old != self.model).then_some((old, self.model))
    }

    /// Where the 3D view draws the joint markers: every joint of the fit,
    /// in `Joint::ALL` order, where it stands in the place, and which one
    /// the panel edits. None before the first rig is built.
    pub(crate) fn markers(&self) -> (Vec<Vec3>, Option<usize>) {
        if self.model.is_none() {
            return (Vec::new(), None);
        }
        let at = Joint::ALL
            .iter()
            .map(|&j| self.fit.joint(j) + self.origin)
            .collect();
        (at, Joint::ALL.iter().position(|&j| j == self.joint))
    }
}

/// Where the built rig's root joint (`LowerTorso`'s) stands in the place.
fn root_pivot(dom: &WeakDom, model: Ref) -> Option<Vec3> {
    let rig = rbx_animation::Rig::read(dom, model);
    let root = match dom.get(rig.root?)?.properties().get("CFrame")? {
        Variant::CFrame(c) => rbx_animation::Frame::from_data(c),
        _ => return None,
    };
    let world = rig.layout(root, &|_| None);
    let joint = rig.joint_for("LowerTorso")?;
    Some(joint.pivot(world.get(&joint.part0)?).position)
}

/// Points each body part of `model` at the mesh the upload made of it: the
/// imported `MeshPart` named `<Part>_Geo` (or plain `<Part>`). Returns how
/// many parts were found.
///
/// A rigid upload comes back as `<Part>_Geo` parts with no joints; a skinned
/// one is recognised as R15 and comes back as `<Part>` parts with the
/// importer's own Motor6Ds, rig attachments, Humanoid and
/// `HumanoidRootPart` (models 95056738325785 and 91357319063637). Only the
/// mesh and the skinning flags are taken, so the rig keeps its own joints
/// and nothing is doubled. A `WrapTarget` the import carries (the body
/// cage) replaces any under our part, with every property of the imported
/// one. `JointOffset` is the bone's position from the
/// part's centre; the importer's centre is the cut part's, as its
/// `InitialSize` matches the cut's size exactly.
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
        let Some((source_ref, source)) = source.and_then(|s| Some((s, imported.get(s)?))) else {
            continue;
        };
        let Some(mesh) = source.properties().get("MeshId").cloned() else {
            continue;
        };
        let _ = dom.set_property(target, "MeshId", mesh);
        if let Some(turn) = served_turn(dom, model, imported, source_ref, target) {
            turn_part(dom, model, target, turn);
        }
        for property in [
            "InitialSize",
            "HasSkinnedMesh",
            "HasJointOffset",
            "JointOffset",
        ] {
            if let Some(value) = source.properties().get(property).cloned() {
                let _ = dom.set_property(target, property, value);
            }
        }
        let wrap = source.children().iter().find_map(|&c| {
            let i = imported.get(c)?;
            (i.class() == "WrapTarget").then_some(i)
        });
        if let Some(wrap) = wrap {
            let old: Vec<Ref> = dom.get(target).map_or_else(Vec::new, |t| {
                t.children()
                    .iter()
                    .copied()
                    .filter(|&c| dom.get(c).is_some_and(|i| i.class() == "WrapTarget"))
                    .collect()
            });
            for r in old {
                dom.remove(r);
            }
            let fresh = dom.new_instance("WrapTarget", wrap.name(), Some(target));
            for (property, value) in wrap.properties() {
                let _ = dom.set_property(fresh, property, value.clone());
            }
        }
        found += 1;
    }
    found
}

/// A part's `CFrame`-typed `property`.
fn frame_of(dom: &WeakDom, referent: Ref, property: &str) -> Option<Frame> {
    match dom.get(referent)?.properties().get(property)? {
        Variant::CFrame(c) => Some(Frame::from_data(c)),
        _ => None,
    }
}

/// Which way a body faces, from its upper arms: +1 for −Z (Roblox's front,
/// the right arm on +X), −1 for +Z.
fn facing(dom: &WeakDom, right: Ref, left: Ref) -> Option<f32> {
    let x = |r| frame_of(dom, r, "CFrame").map(|f| f.position.x);
    Some((x(right)? - x(left)?).signum())
}

/// How `target` must turn, in its own frame, for the mesh served for
/// `source` to stand in it as it stood in the cut, or None when it already
/// does.
///
/// The importer turns the whole upload half round about Y. A glTF written
/// facing −Z (Roblox's front) came back facing +Z, the right arm on −X,
/// every part unrotated (models 95056738325785 and 91357319063637), so the
/// writer now faces the file +Z (`rbx_autorig::write::file_space`) and the
/// answer should face as our rig does, with nothing to turn here. Should it
/// not, a served mesh is in its imported part's frame, so in a part of ours
/// facing −Z it would stand back to front: a forearm with the hand's skin at
/// the elbow (test `the_served_forearm_is_the_cut_turned_half_round`).
/// Turning the part on its centre fixes how a rigid part looks, but a
/// skinned body still stood back to front in game (89657914261022), which
/// is why the file itself is turned.
fn served_turn(
    dom: &WeakDom,
    model: Ref,
    imported: &WeakDom,
    source: Ref,
    target: Ref,
) -> Option<Quat> {
    let ours = |name: &str| {
        let m = dom.get(model)?;
        m.children()
            .iter()
            .copied()
            .find(|&c| dom.get(c).is_some_and(|i| i.name() == name))
    };
    let theirs = |name: &str| find_named(imported, &[format!("{name}_Geo"), name.to_string()]);
    let ours_facing = facing(dom, ours("RightUpperArm")?, ours("LeftUpperArm")?)?;
    let theirs_facing = facing(imported, theirs("RightUpperArm")?, theirs("LeftUpperArm")?)?;
    // The imported part's rotation, brought into our world.
    let mut served = frame_of(imported, source, "CFrame").map_or(Quat::IDENTITY, |f| f.rotation);
    if ours_facing != theirs_facing {
        served = Quat::from_rotation_y(std::f32::consts::PI) * served;
    }
    let current = frame_of(dom, target, "CFrame")?.rotation;
    let turn = (current.inverse() * served).normalize();
    (turn.angle_between(Quat::IDENTITY) > 1e-3).then_some(turn)
}

/// Turns `part` by `turn` about its centre, in its own frame, and moves
/// everything held in that frame (its attachments, its pivot, and the
/// `C0`/`C1` of every joint in `model` it is a side of) the other way, so
/// that only the part's own geometry turns: every joint and attachment stays
/// where it stood in the place.
fn turn_part(dom: &mut WeakDom, model: Ref, part: Ref, turn: Quat) {
    let back = Frame::new(Vec3::ZERO, turn.inverse());
    let set = |dom: &mut WeakDom, r: Ref, property: &str, f: Frame| {
        let _ = dom.set_property(r, property, Variant::CFrame(f.to_data()));
    };
    if let Some(frame) = frame_of(dom, part, "CFrame") {
        set(
            dom,
            part,
            "CFrame",
            frame.mul(&Frame::new(Vec3::ZERO, turn)),
        );
    }
    if let Some(pivot) = frame_of(dom, part, "PivotOffset") {
        set(dom, part, "PivotOffset", back.mul(&pivot));
    }
    let held: Vec<Ref> = dom.get(part).map_or(Vec::new(), |p| p.children().to_vec());
    for child in held {
        let class = dom.get(child).map(|c| c.class().to_string());
        if matches!(class.as_deref(), Some("Attachment" | "Bone")) {
            if let Some(frame) = frame_of(dom, child, "CFrame") {
                set(dom, child, "CFrame", back.mul(&frame));
            }
        }
    }
    let mut pending = vec![model];
    while let Some(next) = pending.pop() {
        let Some(instance) = dom.get(next) else {
            continue;
        };
        pending.extend(instance.children());
        for (side, offset) in [("Part0", "C0"), ("Part1", "C1")] {
            let holds = matches!(
                dom.get(next).and_then(|i| i.properties().get(side)),
                Some(Variant::Ref(r)) if *r == part
            );
            if holds {
                if let Some(frame) = frame_of(dom, next, offset) {
                    set(dom, next, offset, back.mul(&frame));
                }
            }
        }
    }
}

/// How many `WrapTarget`s the body parts of `model` carry.
fn wrap_targets(dom: &WeakDom, model: Ref) -> usize {
    let parts = dom.get(model).map_or(&[][..], |m| m.children());
    parts
        .iter()
        .filter_map(|&p| dom.get(p))
        .flat_map(|p| p.children())
        .filter(|&&c| dom.get(c).is_some_and(|i| i.class() == "WrapTarget"))
        .count()
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
            root: fit.joint(Joint::Root),
            fit: fit.clone(),
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

    /// A distinct `ImportOrigin` per part, as the importer writes one.
    fn import_origin(i: usize) -> Variant {
        Variant::CFrame(rbx_dom::CFrameData {
            position: rbx_dom::Vector3Data {
                x: 0.,
                y: i as f32 * 0.25,
                z: 0.,
            },
            rotation: rbx_dom::rotation::IDENTITY,
        })
    }

    #[test]
    fn publishing_takes_each_parts_mesh_and_skinning_but_not_the_joints() {
        let body = humanoid(Pose::A);
        let fit = rbx_autorig::fit(&body.mesh, HEIGHT).unwrap();
        let cut = Cut {
            body: rbx_autorig::split(&body.mesh, &fit).unwrap(),
            uris: Part::ALL.iter().map(|&p| local_uri("test", p)).collect(),
            root: fit.joint(Joint::Root),
            fit: fit.clone(),
        };
        // A rigid upload names its parts `<Part>_Geo`, a skinned one `<Part>`.
        for (suffix, skinned) in [("_Geo", false), ("", true)] {
            let (mut dom, model) = rig_of(&cut);
            let joints = |dom: &WeakDom| {
                let (mut pending, mut count) = (vec![model], 0);
                while let Some(next) = pending.pop() {
                    let instance = dom.get(next).unwrap();
                    count += usize::from(instance.class() == "Motor6D");
                    pending.extend(instance.children());
                }
                count
            };
            let before = joints(&dom);
            let mut imported = WeakDom::new();
            let root = imported.new_instance("Model", "Upload", None);
            for (i, part) in Part::ALL.iter().enumerate() {
                let name = format!("{}{suffix}", part.name());
                let p = imported.new_instance("MeshPart", &name, Some(root));
                let uri = format!("rbxassetid://{}", 100 + i);
                let props = [
                    ("MeshId", Variant::Content(rbx_dom::Content::Uri(uri))),
                    ("HasSkinnedMesh", Variant::Bool(skinned)),
                    ("HasJointOffset", Variant::Bool(skinned)),
                    (
                        "JointOffset",
                        Variant::Vector3(rbx_dom::Vector3Data {
                            x: 0.,
                            y: -0.3,
                            z: 0.,
                        }),
                    ),
                ];
                for (property, value) in props {
                    imported.set_property(p, property, value).unwrap();
                }
                // The importer's own joint, which must not come across.
                imported.new_instance("Motor6D", part.name(), Some(p));
                // Shaped like the live import of model 134707049711522
                // (2026-10-13): one `<Part>WrapTarget` per part, with the
                // cage's id, origin and `ImportOrigin`.
                let wrap_name = format!("{}WrapTarget", part.name());
                let wrap = imported.new_instance("WrapTarget", &wrap_name, Some(p));
                let cage = format!("rbxassetid://{}", 500 + i);
                imported
                    .set_property(
                        wrap,
                        "CageMeshId",
                        Variant::Content(rbx_dom::Content::Uri(cage)),
                    )
                    .unwrap();
                imported
                    .set_property(
                        wrap,
                        "CageOrigin",
                        Variant::CFrame(rbx_dom::CFrameData {
                            position: rbx_dom::Vector3Data {
                                x: 0.,
                                y: 0.,
                                z: 0.,
                            },
                            rotation: rbx_dom::rotation::IDENTITY,
                        }),
                    )
                    .unwrap();
                imported
                    .set_property(wrap, "ImportOrigin", import_origin(i))
                    .unwrap();
            }
            assert_eq!(swap_meshes(&mut dom, model, &imported), 15);
            assert!(!has_local_meshes(&dom, model));
            assert_eq!(joints(&dom), before);
            for (i, part) in Part::ALL.iter().enumerate() {
                let target = dom
                    .get(model)
                    .unwrap()
                    .children()
                    .iter()
                    .copied()
                    .find(|&c| dom.get(c).unwrap().name() == part.name())
                    .unwrap();
                let wraps: Vec<_> = dom
                    .get(target)
                    .unwrap()
                    .children()
                    .iter()
                    .filter_map(|&c| dom.get(c))
                    .filter(|c| c.class() == "WrapTarget")
                    .collect();
                assert_eq!(wraps.len(), 1);
                assert_eq!(wraps[0].name(), format!("{}WrapTarget", part.name()));
                assert_eq!(
                    wraps[0].properties().get("ImportOrigin"),
                    Some(&import_origin(i))
                );
                assert_eq!(
                    wraps[0].properties().get("CageMeshId"),
                    Some(&Variant::Content(rbx_dom::Content::Uri(format!(
                        "rbxassetid://{}",
                        500 + i
                    ))))
                );
            }
            assert_eq!(wrap_targets(&dom, model), 15);
            let rig = rbx_animation::Rig::read(&dom, model);
            let head = rig.joint_for("Head").unwrap().part1;
            let props = dom.get(head).unwrap().properties();
            assert_eq!(props.get("HasSkinnedMesh"), Some(&Variant::Bool(skinned)));
            assert_eq!(
                props.get("MeshId"),
                Some(&Variant::Content(rbx_dom::Content::Uri(format!(
                    "rbxassetid://{}",
                    100 + Part::ALL.iter().position(|&p| p == Part::Head).unwrap()
                ))))
            );
        }
    }

    /// Every joint's pivot in the place, by the name of its `Part1`.
    fn pivots(dom: &WeakDom, model: Ref) -> Vec<(String, Frame)> {
        let rig = rbx_animation::Rig::read(dom, model);
        let root = frame_of(dom, rig.root.unwrap(), "CFrame").unwrap();
        let world = rig.layout(root, &|_| None);
        Part::ALL
            .iter()
            .map(|part| {
                let joint = rig.joint_for(part.name()).unwrap();
                (part.name().to_string(), joint.pivot(&world[&joint.part0]))
            })
            .collect()
    }

    #[test]
    fn a_served_mesh_turned_half_round_turns_its_part_but_no_joint() {
        let body = humanoid(Pose::A);
        let fit = rbx_autorig::fit(&body.mesh, HEIGHT).unwrap();
        let cut = Cut {
            body: rbx_autorig::split(&body.mesh, &fit).unwrap(),
            uris: Part::ALL.iter().map(|&p| local_uri("test", p)).collect(),
            root: fit.joint(Joint::Root),
            fit: fit.clone(),
        };
        let (mut dom, model) = rig_of(&cut);
        let half = Quat::from_rotation_y(std::f32::consts::PI);
        // The importer's answer: the same body facing +Z, parts unrotated.
        let mut imported = WeakDom::new();
        let root = imported.new_instance("Model", "Upload", None);
        let mut before = Vec::new();
        for (i, part) in Part::ALL.iter().enumerate() {
            let ours = dom
                .get(model)
                .unwrap()
                .children()
                .iter()
                .copied()
                .find(|&c| dom.get(c).unwrap().name() == part.name())
                .unwrap();
            let frame = frame_of(&dom, ours, "CFrame").unwrap();
            before.push((ours, frame));
            let p = imported.new_instance("MeshPart", part.name(), Some(root));
            let at = Frame::at(half * frame.position);
            let uri = format!("rbxassetid://{}", 100 + i);
            for (property, value) in [
                ("MeshId", Variant::Content(rbx_dom::Content::Uri(uri))),
                ("CFrame", Variant::CFrame(at.to_data())),
            ] {
                imported.set_property(p, property, value).unwrap();
            }
        }
        let joints = pivots(&dom, model);
        assert_eq!(swap_meshes(&mut dom, model, &imported), 15);
        for (name, pivot) in pivots(&dom, model).iter().zip(&joints).map(|(a, b)| {
            assert_eq!(a.0, b.0);
            (&a.0, (a.1, b.1))
        }) {
            assert!(
                pivot.0.approx_eq(&pivot.1, 1e-4),
                "{name}: the joint moved from {:?} to {:?}",
                pivot.1,
                pivot.0
            );
        }
        for (part, was) in before {
            let now = frame_of(&dom, part, "CFrame").unwrap();
            // Turned half round about its own centre.
            assert!(now.position.distance(was.position) < 1e-5);
            assert!(now.rotation.angle_between(was.rotation * half) < 1e-4);
            // A served point (a cut point turned half round) lands where
            // the cut point stood.
            let cut_point = Vec3::new(0.3, -0.2, 0.1);
            let served = half * cut_point;
            assert!(
                now.transform_point(served)
                    .distance(was.transform_point(cut_point))
                    < 1e-4
            );
        }
        // A second swap from the same upload changes nothing more.
        let again: Vec<Frame> = Part::ALL
            .iter()
            .map(|p| {
                let r = dom
                    .get(model)
                    .unwrap()
                    .children()
                    .iter()
                    .copied()
                    .find(|&c| dom.get(c).unwrap().name() == p.name())
                    .unwrap();
                frame_of(&dom, r, "CFrame").unwrap()
            })
            .collect();
        swap_meshes(&mut dom, model, &imported);
        for (p, was) in Part::ALL.iter().zip(again) {
            let r = dom
                .get(model)
                .unwrap()
                .children()
                .iter()
                .copied()
                .find(|&c| dom.get(c).unwrap().name() == p.name())
                .unwrap();
            assert!(frame_of(&dom, r, "CFrame").unwrap().approx_eq(&was, 1e-5));
        }
    }

    /// The Woman body's right forearm (CC0, quevedo-woman-body) as the
    /// auto-rigger cut it and as Roblox served it back from the skinned
    /// upload 91357319063637 (mesh 130732292096244).
    #[test]
    fn the_served_forearm_is_the_cut_turned_half_round() {
        let cut = rbx_mesh::parse_skinned(include_bytes!(
            "../../../tests/fixtures/autorig/woman-right-lower-arm-cut.mesh"
        ))
        .unwrap();
        let served = rbx_mesh::parse_skinned(include_bytes!(
            "../../../tests/fixtures/autorig/woman-right-lower-arm-served.mesh"
        ))
        .unwrap();
        let half = Quat::from_rotation_y(std::f32::consts::PI);
        let bone = |mesh: &rbx_mesh::SkinnedMesh, name: &str| {
            let skin = mesh.skin.as_ref().unwrap();
            Vec3::from(skin.bones.iter().find(|b| b.name == name).unwrap().position)
        };
        // The served elbow bone is the part's `JointOffset` on the published
        // model, and the cut's elbow turned half round.
        let joint_offset = Vec3::new(0.33292568, 0.022104025, 0.07772821);
        let elbow = bone(&served, "RightLowerArm");
        assert!(elbow.distance(joint_offset) < 1e-4, "{elbow}");
        assert!((half * bone(&cut, "RightLowerArm")).distance(elbow) < 1e-3);
        // So is every vertex: the importer turned the geometry, not just the
        // bones (in its own vertex order).
        assert_eq!(cut.mesh.vertices.len(), served.mesh.vertices.len());
        let turned: Vec<Vec3> = cut
            .mesh
            .vertices
            .iter()
            .map(|v| half * Vec3::from(v.position))
            .collect();
        for b in &served.mesh.vertices {
            let b = Vec3::from(b.position);
            let nearest = turned
                .iter()
                .map(|t| t.distance(b))
                .fold(f32::MAX, f32::min);
            assert!(nearest < 1e-3, "{b} is {nearest} from the turned cut");
        }
    }

    fn auto_rig(mesh: Mesh, fit: Fit) -> AutoRig {
        AutoRig {
            source: PathBuf::from("body.obj"),
            bounds: rbx_autorig::mesh_bounds(&mesh, &fit),
            mesh,
            fit,
            symmetric: true,
            joint: Joint::Neck,
            model: None,
            warnings: Vec::new(),
            busy: false,
            smooth: true,
            stale: false,
            grabbed: None,
            origin: Vec3::ZERO,
            placed: Vec::new(),
        }
    }

    #[test]
    fn undo_and_redo_bring_back_the_fit_of_the_rig_they_bring_back() {
        let body = humanoid(Pose::A);
        let fit = rbx_autorig::fit(&body.mesh, HEIGHT).unwrap();
        let mut rig = auto_rig(body.mesh.clone(), fit.clone());
        let mut dom = WeakDom::new();
        let first = dom.new_instance("Model", "First", None);
        rig.model = Some(first);
        rig.origin = Vec3::new(0., 3., 0.);
        rig.remember(first, fit.clone());
        // A panel edit moves the elbow and the re-cut replaces the rig.
        let elbow = fit.joint(Joint::LeftElbow);
        let moved = elbow + Vec3::new(0., 0.1, 0.05);
        assert!(!rig.set_joint(Joint::LeftElbow, moved));
        dom.remove(first);
        let second = dom.new_instance("Model", "Second", None);
        rig.model = Some(second);
        rig.remember(second, rig.fit.clone());
        assert_eq!(rig.follow(|r| dom.get(r).is_some()), None);

        // Undo: the snapshot brings the first rig back under its own
        // referent, and its elbow with it.
        assert_eq!(
            rig.follow(|r| r == first),
            Some((Some(second), Some(first)))
        );
        assert_eq!(rig.fit.joint(Joint::LeftElbow), elbow);
        assert_eq!(
            rig.markers().0[Joint::ALL
                .iter()
                .position(|&j| j == Joint::LeftElbow)
                .unwrap()],
            elbow + rig.origin
        );
        // Redo: the second rig and the moved elbow.
        assert_eq!(
            rig.follow(|r| r == second),
            Some((Some(first), Some(second)))
        );
        assert!(rig.fit.joint(Joint::LeftElbow).distance(moved) < 1e-6);
        // Undo past the first rig: nothing to show markers for.
        assert_eq!(rig.follow(|_| false), Some((Some(second), None)));
        assert!(rig.markers().0.is_empty());
    }

    #[test]
    fn a_failed_cut_goes_back_to_the_standing_rigs_fit() {
        let body = humanoid(Pose::A);
        let fit = rbx_autorig::fit(&body.mesh, HEIGHT).unwrap();
        let mut rig = auto_rig(body.mesh.clone(), fit.clone());
        let mut dom = WeakDom::new();
        let model = dom.new_instance("Model", "Rig", None);
        rig.model = Some(model);
        rig.remember(model, fit.clone());
        rig.set_joint(Joint::Neck, fit.joint(Joint::Neck) + Vec3::Y * 0.05);
        assert!(rig.restore());
        assert_eq!(rig.fit.joint(Joint::Neck), fit.joint(Joint::Neck));
    }

    #[test]
    fn a_knee_dragged_onto_the_neck_is_held_on_its_leg() {
        let body = humanoid(Pose::A);
        let fit = rbx_autorig::fit(&body.mesh, HEIGHT).unwrap();
        let mut rig = auto_rig(body.mesh.clone(), fit.clone());
        assert!(rig.set_joint(Joint::LeftKnee, fit.joint(Joint::Neck)));
        let knee = rig.fit.joint(Joint::LeftKnee);
        assert!(knee.y < fit.joint(Joint::LeftHip).y, "{knee}");
        // Its mirror followed.
        let right = rig.fit.joint(Joint::RightKnee);
        assert!((right.x + knee.x).abs() < 1e-4 && (right.y - knee.y).abs() < 1e-4);
    }

    #[test]
    fn local_uris_name_the_part_under_the_cut() {
        assert_eq!(
            local_uri("00ff", Part::LeftHand),
            "rbxasset://rbx-native-local/autorig/00ff/LeftHand.mesh"
        );
    }
}
