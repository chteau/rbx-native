//! Model › Import 3D Model… / Import Image…, and a file dropped on the
//! window: a local `.fbx`/`.obj`/`.gltf`/`.glb` becomes a real Roblox asset
//! through Open Cloud (a mesh cannot be referenced in-game any other way, as
//! in Studio's own Importer) and lands in `Workspace` as the `Model` of
//! `MeshPart`s Roblox's importer made of it. A mesh over Roblox's triangle
//! budget is refused before anything is uploaded (see `rbx_import`).
//!
//! `RBX_STUDIO_IMPORT_MOCK=<model file>` stands in for the upload with a
//! local `.rbxm`/`.rbxmx`, for screenshots and tests without a key (an image
//! upload then just reports asset 0). `RBX_STUDIO_IMPORT_KIND=mesh` makes the
//! launch import behave as Insert Mesh….
//!
//! Open Cloud refuses a new `Mesh` upload, so Model › Insert Mesh… uploads
//! the file as a model like the rest and then places only the `MeshPart`s
//! Roblox made of it, with no `Model` around a lone one.

use std::path::{Path, PathBuf};

use gpui_kit::*;
use rbx_cloud::ModelFile;
use rbx_dom::{Ref, Variant, Vector3Data, WeakDom};
use rbx_import::Format;
use rbx_reflection::ReflectionDatabase;

use crate::command_bar::Feedback;
use crate::explorer;

use super::freeze::{authorize, describe, fetch_asset, read_model};
use super::{clipboard, Shell};

const SOURCE: &str = "Import";
const MOCK_VARIABLE: &str = "RBX_STUDIO_IMPORT_MOCK";
const IMAGE_EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "bmp"];

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// What a file menu item imports: a model, only its meshes, or an image.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImportKind {
    Model,
    Mesh,
    Image,
}

/// What a finished model upload brings back.
struct Imported {
    dom: WeakDom,
    asset_id: u64,
    /// A caveat to show beside the success (a triangle count that could not
    /// be taken).
    note: Option<String>,
}

impl Shell {
    /// The file picker behind both Import menu items.
    pub(crate) fn choose_import(&mut self, kind: ImportKind, cx: &mut Context<Self>) {
        let images = kind == ImportKind::Image;
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(
                if images {
                    "Import Image"
                } else {
                    "Import 3D Model"
                }
                .into(),
            ),
        });
        cx.spawn(async move |shell, cx| {
            let Ok(Ok(Some(paths))) = prompt.await else {
                return;
            };
            let _ = shell.update(cx, |shell, cx| shell.import_as(paths, kind, cx));
        })
        .detach();
    }

    /// Imports each file by what it is: an image as a `Decal` asset, a 3D
    /// file as a model.
    pub(crate) fn import_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.import_as(paths, ImportKind::Model, cx);
    }

    pub(crate) fn import_as(
        &mut self,
        paths: Vec<PathBuf>,
        kind: ImportKind,
        cx: &mut Context<Self>,
    ) {
        for path in paths {
            if is_image(&path) {
                self.import_image(path, cx);
            } else if Format::of(&path).is_some() {
                self.import_model(path, kind == ImportKind::Mesh, cx);
            } else {
                self.import_report(
                    Err(format!("{}: not a 3D or image file", path.display())),
                    cx,
                );
            }
        }
    }

    fn import_model(&mut self, path: PathBuf, mesh: bool, cx: &mut Context<Self>) {
        let name = stem(&path);
        self.command_bar
            .set_feedback(Feedback::Output(format!("Importing {name}\u{2026}")));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { upload_model(&path, &stem(&path)) })
                .await;
            let _ = this.update(cx, |shell, cx| shell.finish_model(name, mesh, result, cx));
        })
        .detach();
    }

    fn finish_model(
        &mut self,
        name: String,
        mesh: bool,
        result: Result<Imported, String>,
        cx: &mut Context<Self>,
    ) {
        let imported = match result {
            Ok(imported) => imported,
            Err(err) => return self.import_report(Err(err), cx),
        };
        let Some(workspace) = explorer::find_by_name(&self.dom, "Workspace") else {
            return self.import_report(Err("The place has no Workspace".into()), cx);
        };
        self.push_history();
        let root = if mesh {
            place_meshes(&mut self.dom, &imported.dom, &name, workspace)
        } else {
            place_model(&mut self.dom, &imported.dom, &name, workspace)
        };
        let changes = self.dom.take_changes();
        self.rebuild_explorer(cx);
        if let Some(root) = root {
            self.reselect(vec![root], cx);
        }
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        if let Some(note) = imported.note {
            self.output.push(SOURCE, Feedback::Warning(note));
        }
        let outcome = match root {
            Some(_) => Ok(format!("Imported {name} as asset {}", imported.asset_id)),
            None => Err(format!(
                "{name}: Roblox\u{2019}s import holds nothing to place"
            )),
        };
        self.import_report(outcome, cx);
    }

    fn import_image(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let name = stem(&path);
        self.command_bar
            .set_feedback(Feedback::Output(format!("Uploading {name}\u{2026}")));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { upload_image(&path) })
                .await;
            let _ = this.update(cx, |shell, cx| shell.finish_image(name, result, cx));
        })
        .detach();
    }

    /// The image becomes a `Decal` on the selected part, or on a new
    /// anchored `Part` in `Workspace` when no part is selected.
    fn finish_image(&mut self, name: String, result: Result<u64, String>, cx: &mut Context<Self>) {
        let id = match result {
            Ok(id) => id,
            Err(err) => return self.import_report(Err(err), cx),
        };
        let selected = self
            .selected()
            .filter(|&r| is_base_part(&self.dom, &self.database, r));
        let Some(workspace) = explorer::find_by_name(&self.dom, "Workspace") else {
            return self.import_report(Err("The place has no Workspace".into()), cx);
        };
        self.push_history();
        let target = selected.unwrap_or_else(|| {
            let part = self.dom.new_instance("Part", &name, Some(workspace));
            if let Some(instance) = self.dom.get_mut(part) {
                let properties = instance.properties_mut();
                properties.insert("Anchored".into(), Variant::Bool(true));
                properties.insert(
                    "Size".into(),
                    Variant::Vector3(Vector3Data {
                        x: 4.0,
                        y: 4.0,
                        z: 0.2,
                    }),
                );
            }
            part
        });
        let decal = self.dom.new_instance("Decal", &name, Some(target));
        let _ = crate::properties::edit::commit(
            &mut self.dom,
            &self.database,
            decal,
            "Texture",
            &format!("rbxassetid://{id}"),
        );
        let changes = self.dom.take_changes();
        self.rebuild_explorer(cx);
        self.reselect(vec![decal], cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        self.import_report(
            Ok(format!("Added {name} as a Decal (rbxassetid://{id})")),
            cx,
        );
    }

    fn import_report(&mut self, result: Result<String, String>, cx: &mut Context<Self>) {
        let feedback = match result {
            Ok(message) => Feedback::Output(message),
            Err(message) => Feedback::Error(message),
        };
        self.output.push(SOURCE, feedback.clone());
        self.command_bar.set_feedback(feedback);
        cx.notify();
    }
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map_or_else(|| "Import".into(), |s| s.to_string_lossy().into_owned())
}

fn is_base_part(dom: &WeakDom, database: &ReflectionDatabase, target: Ref) -> bool {
    dom.get(target)
        .is_some_and(|i| database.is_subclass_of(i.class(), "BasePart"))
}

/// Blocking: reads and checks the file, uploads it, and downloads the model
/// Roblox made of it.
fn upload_model(path: &Path, name: &str) -> Result<Imported, String> {
    let (report, upload) = rbx_import::prepare(path)?;
    if let Some(refusal) = report.refusal() {
        return Err(refusal);
    }
    if let Ok(mock) = std::env::var(MOCK_VARIABLE) {
        let bytes = std::fs::read(&mock).map_err(|err| format!("{mock}: {err}"))?;
        return Ok(Imported {
            dom: read_model(&bytes)?,
            asset_id: 0,
            note: report.unchecked,
        });
    }
    let (client, user) = authorize()?;
    let file = ModelFile {
        name: &upload.file_name,
        content_type: upload.content_type,
        bytes: &upload.bytes,
    };
    let asset_id = client
        .create_model_asset(name, "Imported by rbx-native.", user, &file)
        .map_err(|err| describe(&err))?;
    let dom = read_model(&fetch_asset(&client, asset_id)?)
        .map_err(|err| format!("uploaded as model {asset_id}, but {err}"))?;
    Ok(Imported {
        dom,
        asset_id,
        note: report.unchecked,
    })
}

fn upload_image(path: &Path) -> Result<u64, String> {
    if std::env::var_os(MOCK_VARIABLE).is_some() {
        return Ok(0);
    }
    let bytes = std::fs::read(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let (client, user) = authorize()?;
    client
        .create_image_asset(&stem(path), "Imported by rbx-native.", user, &bytes)
        .map_err(|err| describe(&err))
}

/// Only the `MeshPart`s of the import under `parent`: a lone one by its own
/// name, several in a `Model`.
fn place_meshes(dom: &mut WeakDom, imported: &WeakDom, name: &str, parent: Ref) -> Option<Ref> {
    let mut meshes = Vec::new();
    let mut pending: Vec<Ref> = imported.root_refs().to_vec();
    while let Some(next) = pending.pop() {
        let instance = imported.get(next)?;
        if instance.class() == "MeshPart" {
            meshes.push(next);
        } else {
            pending.extend(instance.children());
        }
    }
    meshes.reverse();
    let root = match meshes[..] {
        [] => return None,
        [only] => clipboard::graft(dom, imported, only, parent)?,
        _ => {
            let holder = dom.new_instance("Model", name, Some(parent));
            for mesh in meshes {
                clipboard::graft(dom, imported, mesh, holder);
            }
            holder
        }
    };
    let _ = dom.set_name(root, name);
    Some(root)
}

/// The imported model under `parent`, named `name`: Roblox's `Model` as it
/// came, or a new one holding whatever else the import's top level held.
fn place_model(dom: &mut WeakDom, imported: &WeakDom, name: &str, parent: Ref) -> Option<Ref> {
    let roots = imported.root_refs();
    let single_model = match roots {
        [only] => imported
            .get(*only)
            .filter(|i| i.class() == "Model")
            .map(|_| *only),
        _ => None,
    };
    let root = match single_model {
        Some(model) => clipboard::graft(dom, imported, model, parent)?,
        None if roots.is_empty() => return None,
        None => {
            let holder = dom.new_instance("Model", name, Some(parent));
            for &root in roots {
                clipboard::graft(dom, imported, root, holder);
            }
            holder
        }
    };
    let _ = dom.set_name(root, name);
    Some(root)
}

#[cfg(test)]
#[path = "import3d/tests.rs"]
mod tests;
