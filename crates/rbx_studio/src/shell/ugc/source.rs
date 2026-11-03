//! A source `.fbx`/`.obj`/`.gltf`/`.glb` as an [`rbx_ugc::Item`].

use std::path::Path;

use rbx_ugc::{Cage, CageKind, Finding, Geometry, Part, Rule, Severity, Texture};

use super::{Collected, ItemKind};

/// Blocking: reads `path` for checking as `kind`.
pub(super) fn item_from_file(path: &Path, kind: ItemKind) -> Result<Collected, String> {
    let file = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let source = rbx_import::read_geometry(path)?;
    if source.meshes.is_empty() {
        return Err(format!("{file} holds no mesh"));
    }

    let mut parts = Vec::new();
    let mut cages = Vec::new();
    for mesh in source.meshes {
        let cage_kind = if mesh.name.ends_with("_InnerCage") {
            Some(CageKind::Inner)
        } else if mesh.name.ends_with("_OuterCage") {
            Some(CageKind::Outer)
        } else {
            None
        };
        let geometry = Geometry {
            positions: mesh.positions,
            triangles: mesh.triangles,
            uvs: mesh.uvs,
            influences: mesh.influences,
            bones: mesh.bones,
            in_studs: false,
        };
        match cage_kind {
            Some(kind) => cages.push(Cage {
                kind,
                name: mesh.name,
                geometry,
                template: None,
            }),
            None => parts.push(Part {
                name: mesh.name,
                geometry: Some(geometry),
                cframe: rbx_ugc::IDENTITY,
                attachments: Vec::new(),
            }),
        }
    }

    let name = parts.first().map_or_else(
        || {
            path.file_stem()
                .map_or_else(|| file.clone(), |s| s.to_string_lossy().into_owned())
        },
        |p| p.name.clone(),
    );
    let textures = source
        .textures
        .into_iter()
        .map(|t| Texture {
            owner: file.clone(),
            slot: t.name,
            width: t.width,
            height: t.height,
        })
        .collect();
    let notes = source
        .notes
        .into_iter()
        .map(|message| Finding {
            rule: Rule::Unreadable,
            severity: Severity::Note,
            subject: file.clone(),
            measured: None,
            limit: None,
            message,
        })
        .collect();

    Ok(Collected {
        item: rbx_ugc::Item {
            name,
            asset_type: kind.asset,
            scale: kind.scale,
            parts,
            textures,
            cages,
        },
        emissive: false,
        notes,
        from_file: true,
        head: None,
    })
}

/// The findings for `path` checked as `kind`, for the importer.
pub(crate) fn check_file(path: &Path, kind: ItemKind) -> Result<Vec<rbx_ugc::Finding>, String> {
    Ok(item_from_file(path, kind)?.findings())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_ugc::{AssetType, BodyScale};

    fn kind(asset: AssetType) -> ItemKind {
        ItemKind {
            asset,
            scale: BodyScale::Normal,
        }
    }

    fn temp_obj(tag: &str, text: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rbx_ugc_source_{}_{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("item.obj");
        std::fs::write(&path, text).unwrap();
        path
    }

    fn cube(name: &str, base: usize) -> String {
        let mut s = format!("o {name}\n");
        for i in 0..8 {
            s += &format!("v {} {} {}\n", i & 1, (i >> 1) & 1, (i >> 2) & 1);
        }
        let quads = [
            [1, 2, 4, 3],
            [5, 7, 8, 6],
            [1, 5, 6, 2],
            [3, 4, 8, 7],
            [1, 3, 7, 5],
            [2, 6, 8, 4],
        ];
        for q in quads {
            let v: Vec<usize> = q.iter().map(|i| i + base).collect();
            s += &format!(
                "f {} {} {}\nf {} {} {}\n",
                v[0], v[1], v[2], v[0], v[2], v[3]
            );
        }
        s
    }

    #[test]
    fn a_shirt_without_an_outer_cage_reports_it_and_no_attachment() {
        let text = format!("{}{}", cube("Hat", 0), cube("Hat_InnerCage", 8));
        let path = temp_obj("cage", &text);
        let findings = check_file(&path, kind(AssetType::Shirt)).unwrap();
        assert!(findings.iter().any(|f| f.rule == Rule::CageMissing));
        assert!(findings.iter().all(|f| f.rule != Rule::Attachment));
    }

    #[test]
    fn a_hat_over_the_triangle_limit_is_reported() {
        let mut text = String::from("o Big\nv 0 0 0\nv 1 0 0\nv 0 1 0\n");
        for _ in 0..4500 {
            text += "f 1 2 3\n";
        }
        let path = temp_obj("big", &text);
        let findings = check_file(&path, kind(AssetType::Hat)).unwrap();
        assert!(findings.iter().any(|f| f.rule == Rule::AccessoryTriangles));
    }
}
