//! Freeze Rotation from the Explorer's menu: each selected turned
//! `MeshPart` baked (see `crate::freeze`), uploaded through the stored key
//! off the UI thread, and written back as its own undo step once Roblox has
//! the new mesh.

use std::time::Duration;

use glam::Vec3;
use gpui_kit::*;
use rbx_assets::AssetRef;
use rbx_cloud::{ApiKey, Client, CloudError, ModelFile};
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::command_bar::Feedback;
use crate::freeze::{self, Plan, Route};

use super::Shell;

const SOURCE: &str = "Freeze Rotation";

/// Answers the upload with a canned result instead of reaching Roblox, for
/// screenshots: `ok` (mesh 1, proportions taken from the bake) or anything
/// else as the error text.
const MOCK_VARIABLE: &str = "RBX_STUDIO_FREEZE_MOCK";

/// How many times to ask for the new model before giving up: a fresh upload
/// can take a moment to become downloadable.
const DOWNLOAD_ATTEMPTS: u32 = 6;

/// Whether any of `selected` can be frozen.
pub(super) fn has_freezable(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    selected: &[Ref],
) -> bool {
    selected
        .iter()
        .any(|&target| freeze::freezable(dom, database, target))
}

impl Shell {
    /// Every selected instance that can be frozen: models, blocks and balls
    /// at once as one undo step, each `MeshPart` once its upload is back.
    pub(super) fn freeze_selected(&mut self, cx: &mut Context<Self>) {
        let (mut local, mut uploads) = (Vec::new(), Vec::new());
        for &target in self.selected_all() {
            match freeze::route(&self.dom, &self.database, target) {
                Some(Route::Local) => local.push(target),
                Some(Route::Upload) => uploads.push(target),
                None => {}
            }
        }
        if !local.is_empty() {
            self.freeze_in_place(&local, cx);
        }
        let meshes = self.viewport.read(cx).meshes().clone();
        for target in uploads {
            let Some(instance) = self.dom.get(target) else {
                continue;
            };
            let name = instance.name().to_string();
            let mesh = freeze::mesh_uri(instance.properties())
                .and_then(|uri| AssetRef::parse(uri).ok())
                .and_then(|asset| meshes.get(&asset).cloned());
            let Some(mesh) = mesh else {
                self.report(
                    Err(format!("{name}: its mesh hasn\u{2019}t loaded yet")),
                    cx,
                );
                continue;
            };
            let plan = match freeze::plan(&self.dom, &self.database, target, &mesh) {
                Ok(plan) => plan,
                Err(err) => {
                    self.report(Err(err), cx);
                    continue;
                }
            };
            self.command_bar.set_feedback(Feedback::Output(format!(
                "Uploading the frozen mesh of {name}\u{2026}"
            )));
            let document = rbx_viewer::export::gltf(&plan.export);
            cx.spawn(async move |this, cx| {
                let result = cx
                    .background_spawn(async move {
                        let uploaded = upload(&plan, document.into_bytes());
                        uploaded.map(|done| (plan, done))
                    })
                    .await;
                let _ = this.update(cx, |shell, cx| shell.finish_freeze(result, cx));
            })
            .detach();
        }
        cx.notify();
    }

    fn freeze_in_place(&mut self, targets: &[Ref], cx: &mut Context<Self>) {
        self.push_history();
        let mut lines = Vec::new();
        for &target in targets {
            let name = self
                .dom
                .get(target)
                .map(|i| i.name().to_string())
                .unwrap_or_default();
            match freeze::freeze_local(&mut self.dom, &self.database, target) {
                Ok(notes) => {
                    lines.push(Ok(format!("Froze {name}\u{2019}s rotation")));
                    lines.extend(notes.into_iter().map(Err));
                }
                Err(err) => lines.push(Err(err)),
            }
        }
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        for line in lines {
            match line {
                Ok(done) => self.report(Ok(done), cx),
                Err(note) => self.output.push(SOURCE, Feedback::Warning(note)),
            }
        }
    }

    fn finish_freeze(
        &mut self,
        result: Result<(Plan, (u64, Vec3)), String>,
        cx: &mut Context<Self>,
    ) {
        let outcome = result.and_then(|(plan, (mesh_id, native))| {
            self.push_history();
            let written = freeze::apply(&mut self.dom, &self.database, &plan, mesh_id, native);
            let changes = self.dom.take_changes();
            self.reflect_changes(&changes, cx);
            self.record_history_change(changes);
            written
                .map(|()| format!("Froze {}\u{2019}s rotation into mesh {mesh_id}", plan.name))
                .map_err(|err| format!("{}: {err}", plan.name))
        });
        self.report(outcome, cx);
    }

    fn report(&mut self, result: Result<String, String>, cx: &mut Context<Self>) {
        let feedback = match result {
            Ok(message) => Feedback::Output(message),
            Err(message) => Feedback::Error(message),
        };
        self.output.push(SOURCE, feedback.clone());
        self.command_bar.set_feedback(feedback);
        cx.notify();
    }
}

/// Blocking: uploads the bake, finds the mesh Roblox made of it, and checks
/// that mesh is the bake's shape. The new mesh's id and its own extent.
fn upload(plan: &Plan, gltf: Vec<u8>) -> Result<(u64, Vec3), String> {
    if let Ok(which) = std::env::var(MOCK_VARIABLE) {
        return match which.as_str() {
            "ok" => Ok((1, plan.size)),
            err => Err(err.to_string()),
        };
    }
    let (client, user) = authorize()?;
    let file = ModelFile {
        name: "frozen.gltf",
        content_type: "model/gltf+json",
        bytes: &gltf,
    };
    let model = client
        .create_model_asset(
            &format!("{} (frozen)", plan.name),
            "Rotation applied by rbx-native's Freeze Rotation.",
            user,
            &file,
        )
        .map_err(|err| describe(&err))?;
    let fetch = |id: u64| fetch_asset(&client, id);
    resolve(&fetch(model)?, fetch, plan.size)
        .map_err(|err| format!("uploaded as model {model}, but {err}"))
}

/// A client for the key the editor holds, and the user it belongs to.
pub(super) fn authorize() -> Result<(Client, u64), String> {
    let key = ApiKey::from_env_or_config()
        .ok_or("No Open Cloud API key is set up. Add one from Home \u{203a} Manage key.")?;
    let client = Client::new(Some(key));
    let user = client
        .introspect()
        .map_err(|err| describe(&err))?
        .authorized_user_id;
    Ok((client, user))
}

/// A fresh asset is usually downloadable within seconds, not at once.
pub(super) fn fetch_asset(client: &Client, id: u64) -> Result<Vec<u8>, String> {
    let mut attempt = 0;
    loop {
        match client.asset(id) {
            Ok(content) => return Ok(content.bytes),
            Err(_) if attempt + 1 < DOWNLOAD_ATTEMPTS => {
                // ponytail: fixed back-off, a fresh asset is usually
                // downloadable within seconds; poll smarter if not.
                attempt += 1;
                std::thread::sleep(Duration::from_secs(2 * u64::from(attempt)));
            }
            Err(err) => return Err(format!("asset {id}: {err}")),
        }
    }
}

/// A downloaded `.rbxm`/`.rbxmx`.
pub(super) fn read_model(model: &[u8]) -> Result<WeakDom, String> {
    if rbx_xml::is_xml(model) {
        let text = std::str::from_utf8(model).map_err(|err| err.to_string())?;
        rbx_xml::deserialize(text).map_err(|err| err.to_string())
    } else {
        rbx_binary::deserialize(model).map_err(|err| err.to_string())
    }
}

/// The mesh the uploaded model's `MeshPart` draws, checked against the
/// bake's extent `baked`. `fetch` downloads an asset by id.
pub(super) fn resolve(
    model: &[u8],
    fetch: impl Fn(u64) -> Result<Vec<u8>, String>,
    baked: Vec3,
) -> Result<(u64, Vec3), String> {
    let dom = read_model(model)?;
    let mut stack = dom.root_refs().to_vec();
    let mesh_id = loop {
        let instance = stack
            .pop()
            .and_then(|r| dom.get(r))
            .ok_or("Roblox's import holds no MeshPart")?;
        stack.extend_from_slice(instance.children());
        if instance.class() == "MeshPart" {
            if let Some(id) = freeze::mesh_asset_id(instance.properties()) {
                break id;
            }
        }
    };
    let mesh = rbx_mesh::parse(&fetch(mesh_id)?).map_err(|err| format!("mesh {mesh_id}: {err}"))?;
    let native = Vec3::from(mesh.bounds.size());
    if !freeze::same_shape(baked, native) {
        return Err(format!(
            "Roblox's import changed the mesh's proportions ({native} for {baked}); the part was left as it was"
        ));
    }
    Ok((mesh_id, native))
}

/// The scope that is missing, ahead of the raw error, where that is what it
/// most likely means.
pub(super) fn describe(err: &CloudError) -> String {
    match err {
        CloudError::Http { status: 401 | 403, .. } | CloudError::Refused { status: 401 | 403, .. } => format!(
            "The API key can\u{2019}t upload assets: it needs the asset:read and asset:write permissions \u{2014} Home \u{203a} Manage key. ({err})"
        ),
        CloudError::NoApiKey => {
            "No Open Cloud API key is set up. Add one from Home \u{203a} Manage key.".to_string()
        }
        _ => err.to_string(),
    }
}

#[cfg(test)]
#[path = "freeze/tests.rs"]
mod tests;
