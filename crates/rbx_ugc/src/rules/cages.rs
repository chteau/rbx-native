use crate::{AssetType, Cage, CageKind, Category, Finding, Geometry, Item, Rule, Severity, Value};

const UV_TOLERANCE: f32 = 1e-4;

pub(crate) fn cages(item: &Item, out: &mut Vec<Finding>) {
    if item.asset_type.category() != Category::Layered {
        return;
    }
    let inner = find(item, CageKind::Inner, "InnerCage", "inner", out);
    let outer = find(item, CageKind::Outer, "OuterCage", "outer", out);

    if let (Some(inner), Some(outer)) = (inner, outer) {
        compare(
            outer,
            &inner.geometry,
            "the inner cage",
            "the outer cage starts as a copy of the inner one",
            out,
        );
    }
    for cage in item.cages.iter().filter(|c| c.template.is_some()) {
        if let Some(template) = &cage.template {
            compare(
                cage,
                template,
                "the cage it was made from",
                "a cage starts as a copy of the cage it was made from",
                out,
            );
        }
    }
}

fn find<'a>(
    item: &'a Item,
    kind: CageKind,
    suffix: &str,
    word: &str,
    out: &mut Vec<Finding>,
) -> Option<&'a Cage> {
    let expected = format!("{}_{}", item.name, suffix);
    let cage = item.cages.iter().find(|c| c.kind == kind);
    let message = match cage {
        None => {
            let field = if kind == CageKind::Inner {
                "ReferenceMeshId"
            } else {
                "CageMeshId"
            };
            let mut message = format!("Add the {word} cage: a mesh named {expected} in the source file, which the importer turns into the WrapLayer\u{2019}s {field}.");
            if matches!(item.asset_type, AssetType::Eyebrow | AssetType::Eyelash) {
                message.push_str(" Eyebrows and eyelashes are layered: Studio\u{2019}s Save to Roblox refuses them as rigid accessories.");
            }
            message
        }
        Some(c) if c.name != expected => {
            format!(
                "Rename {} to {expected}; Roblox finds cages by name.",
                c.name
            )
        }
        Some(c) => return Some(c),
    };
    out.push(Finding {
        rule: Rule::CageMissing,
        severity: Severity::Error,
        subject: cage.map_or(expected, |c| c.name.clone()),
        measured: None,
        limit: None,
        message,
    });
    cage
}

/// The vertices of `cage` with a UV `base` doesn't have (every vertex when
/// only one of the two has UVs); none when the vertex counts differ. UVs
/// are looked up, not compared index by index: Roblox's own cages list
/// their vertices in another order than the cage they were made from
/// (4720 of the leather jacket's 4949 outer-cage vertices sit at another
/// index than in its inner cage), and Studio counts UVs not found.
pub fn cage_uv_mismatches(cage: &Geometry, base: &Geometry) -> Vec<usize> {
    if cage.positions.len() != base.positions.len() {
        return Vec::new();
    }
    let (a, b) = (&cage.uvs, &base.uvs);
    if a.is_empty() != b.is_empty() {
        return (0..cage.positions.len()).collect();
    }
    let key = |uv: &[f32; 2]| uv.map(|c| (c / UV_TOLERANCE).round() as i64);
    let known: std::collections::HashSet<[i64; 2]> = b.iter().map(key).collect();
    a.iter()
        .enumerate()
        .filter(|(_, uv)| !known.contains(&key(uv)))
        .map(|(i, _)| i)
        .collect()
}

fn compare(cage: &Cage, base: &Geometry, base_name: &str, lead: &str, out: &mut Vec<Finding>) {
    let count = cage.geometry.positions.len();
    let expected = base.positions.len();
    // Not an error: Studio 0.745 passed a shirt whose inner and outer cages
    // are 5410 and 5408 vertices; it checks where the cage UVs land on the
    // body instead, which needs its own body regions.
    if count != expected {
        out.push(Finding {
            rule: Rule::CageTopology,
            severity: Severity::Warning,
            subject: cage.name.clone(),
            measured: Some(Value::Count(count)),
            limit: Some(Value::Count(expected)),
            message: format!(
                "{lead}, so it should keep the vertex count of {base_name}. Move its vertices; don\u{2019}t add or delete any."
            ),
        });
        return;
    }
    let differing = cage_uv_mismatches(&cage.geometry, base).len();
    if differing > 0 {
        out.push(Finding {
            rule: Rule::CageUvs,
            severity: Severity::Error,
            subject: cage.name.clone(),
            measured: Some(Value::Count(differing)),
            limit: Some(Value::Count(0)),
            message: format!(
                "Cage UVs must stay as they were in {base_name}. Reset the UVs of {} to match it.",
                cage.name
            ),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BodyScale;

    fn geo(n: usize, u: f32) -> Geometry {
        Geometry {
            positions: vec![[0.0; 3]; n],
            uvs: vec![[u, u]; n],
            ..Default::default()
        }
    }

    fn cage(kind: CageKind, name: &str, g: Geometry) -> Cage {
        Cage {
            kind,
            name: name.into(),
            geometry: g,
            template: None,
            to_mesh: crate::IDENTITY,
        }
    }

    fn item(asset_type: AssetType, cages: Vec<Cage>) -> Item {
        Item {
            name: "Coat".into(),
            asset_type,
            scale: BodyScale::Normal,
            parts: vec![],
            textures: vec![],
            cages,
        }
    }

    fn run(i: &Item) -> Vec<Finding> {
        let mut out = Vec::new();
        cages(i, &mut out);
        out
    }

    #[test]
    fn rigid_items_are_not_checked() {
        assert!(run(&item(AssetType::Hat, vec![])).is_empty());
    }

    #[test]
    fn missing_cages_are_errors() {
        let f = run(&item(AssetType::Jacket, vec![]));
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].subject, "Coat_InnerCage");
        assert_eq!(
            f[0].message,
            "Add the inner cage: a mesh named Coat_InnerCage in the source file, which the importer turns into the WrapLayer\u{2019}s ReferenceMeshId."
        );
    }

    #[test]
    fn a_rigid_eyebrow_is_missing_its_cages_as_studio_says() {
        for asset in [AssetType::Eyebrow, AssetType::Eyelash] {
            let f = run(&item(asset, vec![]));
            assert_eq!(f.len(), 2);
            assert!(f.iter().all(|f| f.rule == Rule::CageMissing));
            assert!(f[0].message.ends_with(
                "Eyebrows and eyelashes are layered: Studio\u{2019}s Save to Roblox refuses them as rigid accessories."
            ));
        }
    }

    #[test]
    fn a_misnamed_cage_is_reported_by_its_name() {
        let i = item(
            AssetType::Jacket,
            vec![
                cage(CageKind::Inner, "Inner", geo(3, 0.0)),
                cage(CageKind::Outer, "Coat_OuterCage", geo(3, 0.0)),
            ],
        );
        let f = run(&i);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].subject, "Inner");
        assert_eq!(
            f[0].message,
            "Rename Inner to Coat_InnerCage; Roblox finds cages by name."
        );
    }

    #[test]
    fn matching_cages_pass() {
        let i = item(
            AssetType::Jacket,
            vec![
                cage(CageKind::Inner, "Coat_InnerCage", geo(3, 0.0)),
                cage(CageKind::Outer, "Coat_OuterCage", geo(3, 0.0)),
            ],
        );
        assert!(run(&i).is_empty());
    }

    #[test]
    fn a_different_vertex_count_is_a_topology_warning() {
        let i = item(
            AssetType::Jacket,
            vec![
                cage(CageKind::Inner, "Coat_InnerCage", geo(3, 0.0)),
                cage(CageKind::Outer, "Coat_OuterCage", geo(4, 0.0)),
            ],
        );
        let f = run(&i);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].rule, Rule::CageTopology);
        assert_eq!(f[0].severity, Severity::Warning);
        assert_eq!(f[0].measured, Some(Value::Count(4)));
        assert_eq!(f[0].limit, Some(Value::Count(3)));
    }

    #[test]
    fn uv_mismatches_name_the_moved_vertices() {
        let base = geo(4, 0.5);
        let mut moved = base.clone();
        moved.uvs[1] = [0.1, 0.5];
        moved.uvs[3] = [0.5, 0.9];
        assert_eq!(cage_uv_mismatches(&moved, &base), vec![1, 3]);
        assert!(cage_uv_mismatches(&geo(3, 0.1), &base).is_empty());
        let bare = Geometry {
            positions: vec![[0.0; 3]; 4],
            ..Default::default()
        };
        assert_eq!(cage_uv_mismatches(&bare, &base), vec![0, 1, 2, 3]);
    }

    #[test]
    fn reordered_uvs_are_not_moved_ones() {
        let base = Geometry {
            positions: vec![[0.0; 3]; 3],
            uvs: vec![[0.1, 0.2], [0.3, 0.4], [0.5, 0.6]],
            ..Default::default()
        };
        let mut cage = base.clone();
        cage.uvs.reverse();
        assert!(cage_uv_mismatches(&cage, &base).is_empty());
    }

    #[test]
    fn changed_uvs_are_counted() {
        let i = item(
            AssetType::Jacket,
            vec![
                cage(CageKind::Inner, "Coat_InnerCage", geo(3, 0.0)),
                cage(CageKind::Outer, "Coat_OuterCage", geo(3, 0.5)),
            ],
        );
        let f = run(&i);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].rule, Rule::CageUvs);
        assert_eq!(f[0].measured, Some(Value::Count(3)));
    }

    #[test]
    fn a_template_is_checked_too() {
        let mut inner = cage(CageKind::Inner, "Coat_InnerCage", geo(3, 0.0));
        inner.template = Some(geo(5, 0.0));
        let i = item(
            AssetType::Jacket,
            vec![inner, cage(CageKind::Outer, "Coat_OuterCage", geo(3, 0.0))],
        );
        let f = run(&i);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].rule, Rule::CageTopology);
        assert_eq!(f[0].limit, Some(Value::Count(5)));
        assert!(f[0].message.contains("the cage it was made from"));
    }
}
