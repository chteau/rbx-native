//! A kit model through Import 3D's own upload (`shell::import3d`), then
//! shaped the way a kit piece is used: one `Model` named after the asset,
//! every part anchored, scaled to studs, its pivot on its base centre at the
//! origin, and credit attributes (`Source`, and for a kit `License` and
//! `Author`) on it.

use glam::Vec3;
use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::pick;

use super::{Credit, Resolved};

const ATTRIBUTES: &str = "AttributesSerialize";

/// An imported kit model, ready to graft.
pub(crate) struct Prepared {
    /// Holds the one `Model`, `root`.
    pub(crate) dom: WeakDom,
    pub(crate) root: Ref,
    /// Its extent in studs.
    pub(crate) size: Vec3,
    pub(crate) asset_id: u64,
    /// A caveat from the importer (a mesh it decimated).
    pub(crate) note: Option<String>,
}

/// Blocking: uploads `resolved.file` and shapes what Roblox made of it.
/// `scale` replaces the kit's own studs per unit.
pub(crate) fn import(resolved: &Resolved, scale: Option<f32>) -> Result<Prepared, String> {
    let imported = crate::shell::import3d::upload_model(&resolved.file, &resolved.name, None)?;
    let mut dom = imported.dom;
    let (root, size) = shape(
        &mut dom,
        &resolved.name,
        &resolved.credit,
        scale.unwrap_or(resolved.scale),
    )?;
    Ok(Prepared {
        dom,
        root,
        size,
        asset_id: imported.asset_id,
        note: imported.note,
    })
}

/// Gathers `dom`'s roots into one `Model` called `name` and makes it a kit
/// piece; its root and size.
pub(crate) fn shape(
    dom: &mut WeakDom,
    name: &str,
    credit: &Credit,
    scale: f32,
) -> Result<(Ref, Vec3), String> {
    let db = ReflectionDatabase::shared();
    let roots = dom.root_refs().to_vec();
    let model = match roots[..] {
        [] => {
            return Err(format!(
                "{name}: Roblox\u{2019}s import holds nothing to place"
            ))
        }
        [only] if dom.get(only).is_some_and(|i| i.class() == "Model") => only,
        _ => {
            let model = dom.new_instance("Model", name, None);
            for root in roots {
                dom.set_parent(root, Some(model));
            }
            model
        }
    };
    let _ = dom.set_name(model, name);
    // The kit's pivot is the base centre, not a part's.
    let _ = dom.remove_property(model, "PrimaryPart");
    for part in parts(dom, &[model]) {
        let _ = dom.set_property(part, "Anchored", Variant::Bool(true));
    }
    rbx_lua::pivot::scale_by(dom, db, model, scale)?;
    let (min, max) = bounds(dom, &[model]).ok_or_else(|| format!("{name} holds no parts"))?;
    let centre = (min + max) * 0.5;
    translate(dom, &[model], -Vec3::new(centre.x, min.y, centre.z));
    let origin = CFrameData {
        position: Vector3Data {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    };
    // Both spellings: the editor's pivot reads `WorldPivot`, Roblox saves
    // and loads `WorldPivotData`.
    let _ = dom.set_property(model, "WorldPivot", Variant::CFrame(origin));
    let _ = dom.set_property(
        model,
        "WorldPivotData",
        Variant::OptionalCFrame(Some(origin)),
    );
    let attributes = [
        Some(("Source", credit.source.as_str())),
        credit.license.map(|license| ("License", license)),
        Some(("Author", credit.author.as_str())).filter(|(_, author)| !author.is_empty()),
    ];
    for (key, value) in attributes.into_iter().flatten() {
        let blob = dom.get(model).and_then(|i| i.properties().get(ATTRIBUTES));
        let value = Variant::String(value.to_owned());
        if let Some(blob) = rbx_dom::attributes::set_attribute(blob, key, Some(&value), None) {
            let _ = dom.set_property(model, ATTRIBUTES, blob);
        }
    }
    Ok((model, max - min))
}

/// Moves every part under `roots` by `offset`, and every model pivot with
/// them, in either spelling.
pub(crate) fn translate(dom: &mut WeakDom, roots: &[Ref], offset: Vec3) {
    let shift = |mut cframe: CFrameData| {
        cframe.position.x += offset.x;
        cframe.position.y += offset.y;
        cframe.position.z += offset.z;
        cframe
    };
    for r in subtree(dom, roots) {
        let Some(properties) = dom.get(r).map(|i| i.properties()) else {
            continue;
        };
        // A part's `CFrame` is in the world; an attachment's, which has no
        // size, is its offset in the part.
        let part = properties.contains_key("size");
        let writes: Vec<(&str, Variant)> = properties
            .iter()
            .filter_map(|(key, value)| match (key.as_str(), value) {
                ("CFrame", Variant::CFrame(c)) if part => {
                    Some(("CFrame", Variant::CFrame(shift(*c))))
                }
                ("WorldPivot", Variant::CFrame(c)) => {
                    Some(("WorldPivot", Variant::CFrame(shift(*c))))
                }
                ("WorldPivotData", Variant::OptionalCFrame(Some(c))) => {
                    Some(("WorldPivotData", Variant::OptionalCFrame(Some(shift(*c)))))
                }
                _ => None,
            })
            .collect();
        for (key, value) in writes {
            let _ = dom.set_property(r, key, value);
        }
    }
}

/// The world-aligned box around every part under `roots`.
pub(crate) fn bounds(dom: &WeakDom, roots: &[Ref]) -> Option<(Vec3, Vec3)> {
    parts(dom, roots)
        .into_iter()
        .filter_map(|part| pick::model_of(dom, part))
        .map(|model| {
            let half = (model.x_axis.truncate().abs()
                + model.y_axis.truncate().abs()
                + model.z_axis.truncate().abs())
                * 0.5;
            let centre = model.w_axis.truncate();
            (centre - half, centre + half)
        })
        .reduce(|(a_min, a_max), (b_min, b_max)| (a_min.min(b_min), a_max.max(b_max)))
}

fn parts(dom: &WeakDom, roots: &[Ref]) -> Vec<Ref> {
    let db = ReflectionDatabase::shared();
    subtree(dom, roots)
        .into_iter()
        .filter(|&r| {
            dom.get(r)
                .is_some_and(|i| db.is_subclass_of(i.class(), "BasePart"))
        })
        .collect()
}

fn subtree(dom: &WeakDom, roots: &[Ref]) -> Vec<Ref> {
    let mut out = Vec::new();
    let mut stack = roots.to_vec();
    while let Some(r) = stack.pop() {
        if let Some(instance) = dom.get(r) {
            out.push(r);
            stack.extend(instance.children().iter().copied());
        }
    }
    out
}
