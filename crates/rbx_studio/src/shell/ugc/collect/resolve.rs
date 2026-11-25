//! The download half of `collect`: meshes and textures fetched and parsed
//! into the item.

use glam::{Mat4, Vec3};

use super::super::visual::{cframe_matrix, Place};
use super::*;

fn note(subject: String, message: String) -> Finding {
    Finding {
        rule: Rule::Unreadable,
        severity: Severity::Note,
        subject,
        measured: None,
        limit: None,
        message,
    }
}

/// A mesh file's LOD 0, centred and stretched to `scale_to` (a part's
/// `Size`) when given, else as the file holds it.
pub(in crate::shell::ugc) fn mesh_geometry(
    bytes: &[u8],
    scale_to: Option<[f32; 3]>,
) -> Result<Geometry, rbx_mesh::MeshError> {
    placed_geometry(bytes, scale_to).map(|(g, _)| g)
}

/// [`mesh_geometry`] and the stretch the part draws the file's vertices
/// with: the viewer scales them about the file's origin, not their centre.
fn placed_geometry(
    bytes: &[u8],
    scale_to: Option<[f32; 3]>,
) -> Result<(Geometry, Stretch), rbx_mesh::MeshError> {
    let sm = rbx_mesh::parse_skinned(bytes)?;
    let mesh = &sm.mesh;
    let center = mesh.bounds.center();
    let bsize = mesh.bounds.size();
    let factor: [f32; 3] = std::array::from_fn(|a| match scale_to {
        Some(size) if bsize[a] != 0.0 => size[a] / bsize[a],
        _ => 1.0,
    });
    let positions = mesh
        .vertices
        .iter()
        .map(|v| {
            if scale_to.is_some() {
                std::array::from_fn(|a| (v.position[a] - center[a]) * factor[a])
            } else {
                v.position
            }
        })
        .collect();
    let triangles = sm.lod0().as_chunks::<3>().0.to_vec();
    let influences = sm.skin.as_ref().map_or_else(Vec::new, |skin| {
        skin.influences
            .iter()
            .map(|i| i.iter().count().min(255) as u8)
            .collect()
    });
    let stretch = Stretch {
        factor: Vec3::from(factor),
        centre: match scale_to {
            Some(_) => Vec3::from(center) * Vec3::from(factor),
            None => Vec3::ZERO,
        },
    };
    let geometry = Geometry {
        positions,
        triangles,
        uvs: mesh.vertices.iter().map(|v| v.uv).collect(),
        influences,
        bones: sm.skin.as_ref().map_or_else(Vec::new, |skin| {
            skin.bones.iter().map(|b| b.name.clone()).collect()
        }),
        in_studs: scale_to.is_some(),
    };
    Ok((geometry, stretch))
}

/// How a part draws its mesh file: each vertex times `factor`, so the
/// centred geometry sits `centre` off the part's origin.
#[derive(Clone, Copy)]
struct Stretch {
    factor: Vec3,
    centre: Vec3,
}

/// The stretch a cage that is a `MeshPart` of its own draws with: its file
/// stretched to the part's `Size` about the file's origin.
fn stretch_to(geometry: &Geometry, size: [f32; 3]) -> Vec3 {
    let (lo, hi) = geometry.positions.iter().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(lo, hi), p| (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p))),
    );
    let extent = hi - lo;
    Vec3::select(
        extent.cmpgt(Vec3::ZERO),
        Vec3::from(size) / extent,
        Vec3::ONE,
    )
}

/// `m` as translation, then its 3 x 3 part row by row.
fn affine(m: Mat4) -> rbx_ugc::Affine {
    let t = m.w_axis;
    let r = |i| m.row(i);
    let (a, b, c) = (r(0), r(1), r(2));
    [t.x, t.y, t.z, a.x, a.y, a.z, b.x, b.y, b.z, c.x, c.y, c.z]
}

/// Blocking. `fetch` downloads an asset's bytes by id.
pub(in crate::shell::ugc) fn resolve(
    plan: &Plan,
    fetch: &dyn Fn(&Key) -> Result<Vec<u8>, String>,
) -> Collected {
    let mut notes = plan.notes.clone();
    let mut places: Vec<Place> = Vec::new();
    let mut stretches: Vec<(Ref, Vec3)> = Vec::new();
    // The first read mesh's part and where its centred geometry sits on
    // it, which the cages are measured against.
    let mut handle: Option<(Mat4, Vec3)> = None;
    let parts = plan
        .parts
        .iter()
        .map(|p| {
            let geometry = p.mesh.as_ref().and_then(|id| {
                match fetch(id)
                    .and_then(|b| placed_geometry(&b, Some(p.size)).map_err(|e| e.to_string()))
                {
                    Ok((g, stretch)) => {
                        handle.get_or_insert((cframe_matrix(&p.cframe), stretch.centre));
                        places.push(Place {
                            name: p.name.clone(),
                            node: p.node,
                            local: Mat4::from_translation(stretch.centre),
                        });
                        stretches.push((p.node, stretch.factor));
                        Some(g)
                    }
                    Err(e) => {
                        notes.push(note(
                            p.name.clone(),
                            format!("The mesh could not be read: {e}"),
                        ));
                        None
                    }
                }
            });
            Part {
                name: p.name.clone(),
                geometry,
                cframe: p.cframe,
                attachments: p.attachments.clone(),
            }
        })
        .collect();
    let mut textures = Vec::new();
    for t in &plan.textures {
        let dims = fetch(&Key::Id(t.id)).and_then(|bytes| {
            image::ImageReader::new(std::io::Cursor::new(bytes))
                .with_guessed_format()
                .map_err(|e| e.to_string())?
                .into_dimensions()
                .map_err(|e| e.to_string())
        });
        match dims {
            Ok((width, height)) => textures.push(Texture {
                owner: t.owner.clone(),
                slot: t.slot.clone(),
                width,
                height,
            }),
            Err(e) => notes.push(note(
                format!("{}.{}", t.owner, t.slot),
                format!("The texture could not be read: {e}"),
            )),
        }
    }
    let mut cages: Vec<Cage> = Vec::new();
    let mut inner: Option<Geometry> = None;
    let mut ordered: Vec<&CagePlan> = plan.cages.iter().collect();
    ordered.sort_by_key(|c| c.kind == CageKind::Outer);
    for c in ordered {
        match fetch(&c.mesh).and_then(|b| mesh_geometry(&b, None).map_err(|e| e.to_string())) {
            Ok(g) => {
                // A WrapLayer's cages are drawn the way its handle draws
                // its mesh, after the layer's origin.
                let stretch = match c.fit {
                    Some(size) => stretch_to(&g, size),
                    None => stretches
                        .iter()
                        .find(|(node, _)| *node == c.node)
                        .map_or(Vec3::ONE, |(_, f)| *f),
                };
                let local = Mat4::from_scale(stretch) * cframe_matrix(&c.origin);
                places.push(Place {
                    name: c.name.clone(),
                    node: c.node,
                    local,
                });
                let to_mesh = handle.map_or(rbx_ugc::IDENTITY, |(part, centre)| {
                    affine(
                        Mat4::from_translation(-centre)
                            * part.inverse()
                            * cframe_matrix(&c.cframe)
                            * local,
                    )
                });
                let template = match c.kind {
                    CageKind::Inner => {
                        inner.get_or_insert_with(|| g.clone());
                        None
                    }
                    CageKind::Outer => inner.clone(),
                };
                cages.push(Cage {
                    kind: c.kind,
                    name: c.name.clone(),
                    geometry: g,
                    template,
                    to_mesh,
                });
            }
            Err(e) => notes.push(note(
                c.name.clone(),
                format!("The cage mesh could not be read: {e}"),
            )),
        }
    }
    Collected {
        item: Item {
            name: plan.name.clone(),
            asset_type: plan.kind.asset,
            scale: plan.kind.scale,
            parts,
            textures,
            cages,
        },
        emissive: plan.emissive,
        notes,
        from_file: false,
        head: None,
        places,
    }
}

/// An asset's bytes through the editor's asset cache, downloading what it
/// lacks; a local file is only ever in the cache. Blocking.
pub(in crate::shell::ugc) fn fetch_cached(key: &Key) -> Result<Vec<u8>, String> {
    let cache = rbx_assets::AssetCache::new(None).map_err(|e| e.to_string())?;
    let id = match key {
        Key::Id(id) => *id,
        Key::Local(path) => {
            return cache
                .get_native(path)
                .ok_or_else(|| format!("{path} is no longer in the local asset cache"))
        }
    };
    if let Some(bytes) = cache.get_id(id) {
        return Ok(bytes);
    }
    let client = rbx_cloud::Client::new(rbx_cloud::ApiKey::from_env_or_config());
    let bytes = client
        .asset(id)
        .map_err(|e| format!("downloading asset {id}: {e}"))?
        .bytes;
    let _ = cache.put_id(id, &bytes);
    Ok(bytes)
}
