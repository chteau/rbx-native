use crate::spec::{self, AssetType, Category};
use crate::{Finding, Item, Rule, Severity, Value};

fn error(rule: Rule, subject: &str, measured: Value, limit: Value, message: String) -> Finding {
    Finding {
        rule,
        severity: Severity::Error,
        subject: subject.to_string(),
        measured: Some(measured),
        limit: Some(limit),
        message,
    }
}

pub(crate) fn unreadable(item: &Item, out: &mut Vec<Finding>) {
    for part in item.parts.iter().filter(|p| p.geometry.is_none()) {
        out.push(Finding {
            rule: Rule::Unreadable,
            severity: Severity::Note,
            subject: part.name.clone(),
            measured: None,
            limit: None,
            message: format!(
                "The mesh of {} could not be loaded, so its triangle, size, skin and watertight checks were skipped.",
                part.name
            ),
        });
    }
}

fn mesh_budget(name: &str, count: usize, out: &mut Vec<Finding>) {
    if count > spec::MESH_TRIANGLES {
        out.push(error(
            Rule::MeshTriangles,
            name,
            Value::Count(count),
            Value::Count(spec::MESH_TRIANGLES),
            format!(
                "{name} has {count} triangles, over the {} a mesh may have. Retopologize or decimate it to the limit.",
                spec::MESH_TRIANGLES
            ),
        ));
    }
}

fn part_triangles(item: &Item, name: &str) -> Option<usize> {
    item.parts
        .iter()
        .find(|p| p.name == name)
        .and_then(|p| p.geometry.as_ref())
        .map(|g| g.triangles.len())
}

pub(crate) fn triangles(item: &Item, out: &mut Vec<Finding>) {
    for part in &item.parts {
        if let Some(g) = &part.geometry {
            mesh_budget(&part.name, g.triangles.len(), out);
        }
    }
    for cage in &item.cages {
        mesh_budget(&cage.name, cage.geometry.triangles.len(), out);
    }

    match item.asset_type.category() {
        Category::Rigid | Category::Layered => {
            let total: usize = item
                .parts
                .iter()
                .filter_map(|p| p.geometry.as_ref())
                .map(|g| g.triangles.len())
                .sum();
            if total > spec::ACCESSORY_TRIANGLES {
                out.push(error(
                    Rule::AccessoryTriangles,
                    &item.name,
                    Value::Count(total),
                    Value::Count(spec::ACCESSORY_TRIANGLES),
                    format!(
                        "{} has {total} triangles, over the {} an accessory may have. Retopologize or decimate it to the limit.",
                        item.name,
                        spec::ACCESSORY_TRIANGLES
                    ),
                ));
            }
            if item.parts.len() > 1 {
                out.push(error(
                    Rule::SingleMesh,
                    &item.name,
                    Value::Count(item.parts.len()),
                    Value::Count(1),
                    format!(
                        "{} has {} meshes, but an accessory must be a single mesh. Merge them into one.",
                        item.name,
                        item.parts.len()
                    ),
                ));
            }
        }
        Category::Body => {
            let only_head = item.asset_type == AssetType::DynamicHead;
            for (row, parts, budget) in spec::BODY_ASSET_TRIANGLES {
                if only_head && row != "DynamicHead" {
                    continue;
                }
                let total: usize = parts.iter().filter_map(|n| part_triangles(item, n)).sum();
                if total > budget {
                    out.push(error(
                        Rule::BodyTriangles,
                        row,
                        Value::Count(total),
                        Value::Count(budget),
                        format!(
                            "{row} has {total} triangles, over the {budget} it may have. Retopologize or decimate it to the limit."
                        ),
                    ));
                }
            }
            if item.asset_type == AssetType::Body {
                let total: usize = item
                    .parts
                    .iter()
                    .filter_map(|p| p.geometry.as_ref())
                    .map(|g| g.triangles.len())
                    .sum();
                if total > spec::BODY_TRIANGLES {
                    out.push(error(
                        Rule::BodyTriangles,
                        &item.name,
                        Value::Count(total),
                        Value::Count(spec::BODY_TRIANGLES),
                        format!(
                            "The body has {total} triangles, over the {} a whole body may have. Retopologize or decimate it to the limit.",
                            spec::BODY_TRIANGLES
                        ),
                    ));
                }
            }
        }
    }
}

pub(crate) fn textures(item: &Item, out: &mut Vec<Finding>) {
    for t in &item.textures {
        if t.width > spec::TEXTURE_SIZE || t.height > spec::TEXTURE_SIZE {
            out.push(error(
                Rule::TextureSize,
                &format!("{}.{}", t.owner, t.slot),
                Value::Pixels {
                    width: t.width,
                    height: t.height,
                },
                Value::Pixels {
                    width: spec::TEXTURE_SIZE,
                    height: spec::TEXTURE_SIZE,
                },
                "Marketplace textures can\u{2019}t exceed 2048 \u{d7} 2048. Resize it.".to_string(),
            ));
        }
    }
}

pub(crate) fn influences(item: &Item, out: &mut Vec<Finding>) {
    for part in &item.parts {
        let Some(g) = &part.geometry else { continue };
        let over = g
            .influences
            .iter()
            .filter(|&&n| usize::from(n) > spec::MAX_INFLUENCES)
            .count();
        if over == 0 {
            continue;
        }
        let max = g.influences.iter().copied().max().unwrap_or(0);
        out.push(error(
            Rule::BoneInfluences,
            &part.name,
            Value::Count(usize::from(max)),
            Value::Count(spec::MAX_INFLUENCES),
            format!(
                "{over} vertices in {} follow more than {} bones. Limit the weights to {} bones per vertex in Blender or Maya.",
                part.name,
                spec::MAX_INFLUENCES,
                spec::MAX_INFLUENCES
            ),
        ));
    }
}

pub(crate) fn attachment(item: &Item, out: &mut Vec<Finding>) {
    if !matches!(
        item.asset_type.category(),
        Category::Rigid | Category::Layered
    ) {
        return;
    }
    let allowed = item.asset_type.attachments();
    let Some(part) = item.parts.first() else {
        return;
    };
    if allowed.is_empty()
        || part
            .attachments
            .iter()
            .any(|a| allowed.contains(&a.name.as_str()))
    {
        return;
    }
    let names = allowed.join(", ");
    out.push(Finding {
        rule: Rule::Attachment,
        severity: Severity::Error,
        subject: part.name.clone(),
        measured: None,
        limit: Some(Value::Text(names.clone())),
        message: format!(
            "{} has none of the attachments a {} needs. Add an Attachment named {names}.",
            part.name,
            item.asset_type.label()
        ),
    });
}

/// Eyebrows and eyelashes follow the face through these two bones, and
/// Studio refuses one whose mesh lacks either.
pub(crate) fn joints(item: &Item, out: &mut Vec<Finding>) {
    if !matches!(item.asset_type, AssetType::Eyebrow | AssetType::Eyelash) {
        return;
    }
    let Some(part) = item.parts.first() else {
        return;
    };
    let Some(g) = &part.geometry else {
        return;
    };
    for joint in ["RBX_Leader", "RBX_Follower"] {
        if !g.bones.iter().any(|b| b == joint) {
            out.push(Finding {
                rule: Rule::Skinning,
                severity: Severity::Error,
                subject: part.name.clone(),
                measured: None,
                limit: Some(Value::Text(joint.to_string())),
                message: format!(
                    "{} must use skinning transfer, but you are missing the required joint {joint}.",
                    item.asset_type.type_name()
                ),
            });
        }
    }
}

#[cfg(test)]
#[path = "basic/tests.rs"]
mod tests;
