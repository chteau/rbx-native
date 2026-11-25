//! Projecting a decal onto a mesh's texture (`DrawImageProjected`) and
//! the reverse, reading a texture back through a mesh onto the projector's
//! plane (`SampleImageProjected`).
//!
//! The projector is a box centred on `Position`, looking along
//! `Direction`, `Size.Z` deep. A pixel of the image takes the decal where
//! the mesh surface under it (through its UVs) sits inside the box and
//! faces the projector. What the EditableImageDraw golden shows: back
//! faces get nothing, and neither does a surface the projector meets at
//! an angle, whatever `FadeAngle` says; `BlendIntensity` changes nothing.

use mlua::{AnyUserData, Result, Table, UserDataMethods, Value};
use rbx_dom::editable::lock;

use super::blend::{self, combine, Combine, Paint};
use super::LuaEditableImage;
use crate::editable::mesh::{read_vector, MeshData, V3};

/// How squarely a surface must face the projector to take the decal.
const HEAD_ON: f32 = 0.9999;

struct Projector {
    position: V3,
    direction: V3,
    right: V3,
    up: V3,
    size: V3,
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn unit(v: V3) -> V3 {
    let l = dot(v, v).sqrt();
    if l > 0.0 {
        v.map(|c| c / l)
    } else {
        v
    }
}

fn projector(method: &str, params: &Table) -> Result<Projector> {
    let read = |key: &str| -> Result<V3> {
        match params.get::<Value>(key)? {
            Value::Nil => Err(mlua::Error::runtime(format!(
                "EditableImage::{method} Param ProjectionParams, is missing {key}"
            ))),
            v => read_vector(&v),
        }
    };
    let position = read("Position")?;
    let direction = unit(read("Direction")?);
    let up = read("Up")?;
    let size = read("Size")?;
    let right = unit(cross(direction, up));
    Ok(Projector {
        position,
        direction,
        up: cross(right, direction),
        right,
        size,
    })
}

impl Projector {
    /// Where `p` falls on the decal (0..1 across, 0..1 down), if inside.
    fn decal_point(&self, p: V3) -> Option<(f32, f32)> {
        let rel = sub(p, self.position);
        let x = dot(rel, self.right) / self.size[0] + 0.5;
        let y = 0.5 - dot(rel, self.up) / self.size[1];
        let depth = dot(rel, self.direction);
        ((0.0..1.0).contains(&x)
            && (0.0..1.0).contains(&y)
            && depth.abs() <= self.size[2] / 2.0 + 1e-4)
            .then_some((x, y))
    }

    fn faces(&self, corners: [V3; 3]) -> bool {
        let n = unit(cross(
            sub(corners[1], corners[0]),
            sub(corners[2], corners[0]),
        ));
        -dot(n, self.direction) >= HEAD_ON
    }
}

/// Every triangle of the mesh with its corners' UVs.
fn triangles(m: &MeshData) -> Vec<([V3; 3], [[f32; 2]; 3])> {
    let mut out = Vec::new();
    for (_, face) in m.faces.iter() {
        let positions = m.corners(face);
        for t in crate::editable::mesh::data::corner_triangles(face, &positions) {
            let uv = t.map(|c| m.uvs.get(face.uvs[c]).copied().unwrap_or_default());
            out.push((t.map(|c| positions[c]), uv));
        }
    }
    out
}

fn mesh_of(method: &str, source: &Value) -> Result<MeshData> {
    let mesh = match source {
        Value::UserData(d) => crate::editable::as_mesh(d),
        _ => None,
    };
    match mesh {
        Some(mesh) => mesh.read(|m| Ok(m.clone())),
        None => Err(mlua::Error::runtime(format!(
            "{method}: projectionSource must be an EditableMesh or WrapTextureTransfer"
        ))),
    }
}

fn brush_paint(method: &str, brush: &Table) -> Result<(Combine, blend::Lock)> {
    Ok((
        combine(
            &brush.get::<Value>("ColorBlendType")?,
            Combine::SourceOver,
            method,
        )?,
        blend::lock(&brush.get::<Value>("AlphaBlendType")?)?,
    ))
}

fn texel(rgba: &[u8], (w, h): (u32, u32), (x, y): (f32, f32)) -> [u8; 4] {
    let x = ((x * w as f32) as i64).clamp(0, w as i64 - 1) as usize;
    let y = ((y * h as f32) as i64).clamp(0, h as i64 - 1) as usize;
    let i = (y * w as usize + x) * 4;
    [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
}

/// Barycentric weights of `p` in the 2D triangle `t`.
fn bary2(t: [[f32; 2]; 3], p: [f32; 2]) -> Option<V3> {
    let d = (t[1][1] - t[2][1]) * (t[0][0] - t[2][0]) + (t[2][0] - t[1][0]) * (t[0][1] - t[2][1]);
    if d.abs() < 1e-12 {
        return None;
    }
    let a = ((t[1][1] - t[2][1]) * (p[0] - t[2][0]) + (t[2][0] - t[1][0]) * (p[1] - t[2][1])) / d;
    let b = ((t[2][1] - t[0][1]) * (p[0] - t[2][0]) + (t[0][0] - t[2][0]) * (p[1] - t[2][1])) / d;
    let c = 1.0 - a - b;
    (a >= -1e-6 && b >= -1e-6 && c >= -1e-6).then_some([a, b, c])
}

pub(super) fn add<M: UserDataMethods<LuaEditableImage>>(methods: &mut M) {
    methods.add_method(
        "DrawImageProjected",
        |_, this, (source, params, brush): (Value, Table, Table)| {
            const METHOD: &str = "DrawImageProjected";
            let mesh = mesh_of(METHOD, &source)?;
            let projector = projector(METHOD, &params)?;
            let decal: AnyUserData = match brush.get::<Value>("Decal")? {
                Value::UserData(d) if d.is::<LuaEditableImage>() => d,
                _ => {
                    return Err(mlua::Error::runtime(format!(
                        "EditableImage::{METHOD} Param BrushConfig, is missing Decal"
                    )))
                }
            };
            let decal = lock(&decal.borrow::<LuaEditableImage>()?.pixels).clone();
            let (mode, lock_mode) = brush_paint(METHOD, &brush)?;
            this.edit(|pixels| {
                let (w, h) = (pixels.width as usize, pixels.height as usize);
                for (corners, uv) in triangles(&mesh) {
                    if !projector.faces(corners) {
                        continue;
                    }
                    let screen = uv.map(|[u, v]| [u * w as f32, v * h as f32]);
                    let (x0, x1) = (
                        screen.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min),
                        screen
                            .iter()
                            .map(|p| p[0])
                            .fold(f32::NEG_INFINITY, f32::max),
                    );
                    let (y0, y1) = (
                        screen.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min),
                        screen
                            .iter()
                            .map(|p| p[1])
                            .fold(f32::NEG_INFINITY, f32::max),
                    );
                    for y in (y0.floor().max(0.0) as usize)..(y1.ceil() as usize).min(h) {
                        for x in (x0.floor().max(0.0) as usize)..(x1.ceil() as usize).min(w) {
                            let Some(b) = bary2(screen, [x as f32 + 0.5, y as f32 + 0.5]) else {
                                continue;
                            };
                            let p: V3 = std::array::from_fn(|i| {
                                corners[0][i] * b[0] + corners[1][i] * b[1] + corners[2][i] * b[2]
                            });
                            let Some(at) = projector.decal_point(p) else {
                                continue;
                            };
                            let color = texel(&decal.rgba, (decal.width, decal.height), at);
                            let i = (y * w + x) * 4;
                            Paint::texel(&color, mode, lock_mode)
                                .apply(&mut pixels.rgba[i..i + 4], 1.0);
                        }
                    }
                }
            });
            Ok(())
        },
    );
    methods.add_method(
        "SampleImageProjected",
        |_, this, (source, texture, params, brush): (Value, AnyUserData, Table, Table)| {
            const METHOD: &str = "SampleImageProjected";
            let mesh = mesh_of(METHOD, &source)?;
            let projector = projector(METHOD, &params)?;
            let texture = lock(&texture.borrow::<LuaEditableImage>()?.pixels).clone();
            let (mode, lock_mode) = brush_paint(METHOD, &brush)?;
            let tris: Vec<_> = triangles(&mesh)
                .into_iter()
                .filter(|(c, _)| projector.faces(*c))
                .collect();
            this.edit(|pixels| {
                let (w, h) = (pixels.width as usize, pixels.height as usize);
                for y in 0..h {
                    for x in 0..w {
                        let (s, t) = ((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
                        let at: V3 = std::array::from_fn(|i| {
                            projector.position[i]
                                + projector.right[i] * (s - 0.5) * projector.size[0]
                                + projector.up[i] * (0.5 - t) * projector.size[1]
                        });
                        let hit = tris
                            .iter()
                            .filter_map(|(c, uv)| {
                                let n = cross(sub(c[1], c[0]), sub(c[2], c[0]));
                                let denom = dot(n, projector.direction);
                                let along = dot(n, sub(c[0], at)) / denom;
                                if along.abs() > projector.size[2] / 2.0 + 1e-4 {
                                    return None;
                                }
                                let p: V3 =
                                    std::array::from_fn(|i| at[i] + projector.direction[i] * along);
                                let (e0, e1, e2) = (sub(c[1], c[0]), sub(c[2], c[0]), sub(p, c[0]));
                                let (d00, d01, d11, d20, d21) = (
                                    dot(e0, e0),
                                    dot(e0, e1),
                                    dot(e1, e1),
                                    dot(e2, e0),
                                    dot(e2, e1),
                                );
                                let den = d00 * d11 - d01 * d01;
                                let v = (d11 * d20 - d01 * d21) / den;
                                let w = (d00 * d21 - d01 * d20) / den;
                                let u = 1.0 - v - w;
                                (u >= -1e-6 && v >= -1e-6 && w >= -1e-6).then(|| {
                                    let uv =
                                        [0, 1].map(|k| uv[0][k] * u + uv[1][k] * v + uv[2][k] * w);
                                    (along, uv)
                                })
                            })
                            .min_by(|a, b| a.0.total_cmp(&b.0));
                        if let Some((_, uv)) = hit {
                            let color = texel(
                                &texture.rgba,
                                (texture.width, texture.height),
                                (uv[0], uv[1]),
                            );
                            let i = (y * w + x) * 4;
                            Paint::texel(&color, mode, lock_mode)
                                .apply(&mut pixels.rgba[i..i + 4], 1.0);
                        }
                    }
                }
            });
            Ok(())
        },
    );
}
