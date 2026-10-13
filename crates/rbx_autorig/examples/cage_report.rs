//! Fits Mannequin cage templates onto an auto-rigged body and draws the result.
//!
//! `cargo run -p rbx_autorig --example cage_report -- <mesh> <templates.json> <out.svg>`

use std::fmt::Write as _;
use std::path::Path;

use glam::{Vec2, Vec3};
use rbx_autorig::{fit, fit_cages_report, read, split, CageTemplate, Mesh, Part};
use serde_json::Value;

const PX_PER_STUD: f32 = 120.0;
const MARGIN: f32 = 0.5;

fn main() {
    if let Err(message) = run() {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

fn vec3(value: &Value, what: &str) -> Result<Vec3, String> {
    let array = value
        .as_array()
        .filter(|a| a.len() == 3)
        .ok_or_else(|| format!("{what} must be an array of three numbers"))?;
    let mut out = [0.0f32; 3];
    for (slot, v) in out.iter_mut().zip(array) {
        *slot = v
            .as_f64()
            .ok_or_else(|| format!("{what} must be an array of three numbers"))?
            as f32;
    }
    Ok(Vec3::from(out))
}

fn load_templates(path: &str) -> Result<Vec<CageTemplate>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("couldn't read {path}: {e}"))?;
    let json: Value = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    let entries = json
        .as_array()
        .ok_or_else(|| format!("{path}: expected an array of templates"))?;
    let base = Path::new(path).parent().unwrap_or(Path::new(""));
    let mut templates = Vec::new();
    for entry in entries {
        let name = entry["name"].as_str().ok_or("a template has no \"name\"")?;
        let part = Part::ALL
            .into_iter()
            .find(|p| p.name() == name)
            .ok_or_else(|| format!("unknown part {name}"))?;
        let cage_path = entry["cage_path"]
            .as_str()
            .ok_or_else(|| format!("{name}: no \"cage_path\""))?;
        let cage_file = if Path::new(cage_path).is_absolute() || Path::new(cage_path).exists() {
            Path::new(cage_path).to_path_buf()
        } else {
            base.join(cage_path)
        };
        let bytes = std::fs::read(&cage_file)
            .map_err(|e| format!("couldn't read {}: {e}", cage_file.display()))?;
        let parsed = rbx_mesh::parse(&bytes).map_err(|e| format!("{name}: {e}"))?;
        let origin = vec3(&entry["origin"], &format!("{name}: origin"))?;
        let size = vec3(&entry["init"], &format!("{name}: init"))?;
        let mut mesh = Mesh::default();
        for v in &parsed.vertices {
            mesh.positions.push(Vec3::from(v.position) + origin);
            mesh.uvs.push(Vec2::from(v.uv));
        }
        mesh.triangles = parsed.indices.as_chunks::<3>().0.to_vec();
        let rig = entry["rig"]
            .as_object()
            .ok_or_else(|| format!("{name}: no \"rig\" object"))?;
        let mut attachments = Vec::new();
        for (key, value) in rig {
            attachments.push((key.clone(), vec3(value, &format!("{name}: {key}"))?));
        }
        templates.push(CageTemplate {
            part,
            mesh,
            size,
            attachments,
        });
    }
    Ok(templates)
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [mesh_path, templates_path, out_path] = args.as_slice() else {
        return Err("usage: cage_report <mesh> <templates.json> <out.svg>".into());
    };
    let mesh = read(Path::new(mesh_path)).map_err(|e| e.to_string())?;
    let templates = load_templates(templates_path)?;
    let skeleton = fit(&mesh, 5.2).map_err(|e| e.to_string())?;
    let body = split(&mesh, &skeleton).map_err(|e| e.to_string())?;
    let (cages, folded) = fit_cages_report(&body, &templates).map_err(|e| e.to_string())?;
    let (mut all_vertices, mut all_folded) = (0, 0);

    let mut body_tris: Vec<[Vec3; 3]> = Vec::new();
    let mut cage_tris: Vec<[Vec3; 3]> = Vec::new();
    for ((part, cage), &folds) in body.parts.iter().zip(&cages).zip(&folded) {
        all_vertices += cage.positions.len();
        all_folded += folds;
        let world = |m: &Mesh, t: &[u32; 3]| t.map(|i| part.centre + m.positions[i as usize]);
        body_tris.extend(part.mesh.triangles.iter().map(|t| world(&part.mesh, t)));
        cage_tris.extend(cage.triangles.iter().map(|t| world(cage, t)));

        let distances: Vec<f32> = cage
            .positions
            .iter()
            .map(|c| {
                part.mesh
                    .positions
                    .iter()
                    .map(|p| p.distance(*c))
                    .fold(f32::INFINITY, f32::min)
            })
            .collect();
        let min = distances.iter().copied().fold(f32::INFINITY, f32::min);
        let mean = distances.iter().sum::<f32>() / distances.len().max(1) as f32;
        println!(
            "{:<14} vertices {:>6}  min {:.4}  mean {:.4}  folded {}",
            part.part.name(),
            distances.len(),
            min,
            mean,
            folds
        );
    }
    println!("TOTAL vertices {all_vertices} folded {all_folded}");
    // `CAGE_OBJ=<path>` also writes the cages, and the body, in world space.
    if let Ok(path) = std::env::var("CAGE_OBJ") {
        let mut obj = String::new();
        let mut base = 1;
        for (name, tris) in [("cage", &cage_tris), ("body", &body_tris)] {
            obj.push_str(&format!("o {name}\n"));
            for t in tris.iter() {
                for v in t {
                    obj.push_str(&format!("v {} {} {}\n", v.x, v.y, v.z));
                }
                obj.push_str(&format!("f {} {} {}\n", base, base + 1, base + 2));
                base += 3;
            }
        }
        std::fs::write(&path, obj).map_err(|e| format!("{path}: {e}"))?;
    }

    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    for p in body_tris.iter().chain(&cage_tris).flatten() {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    if !lo.is_finite() || !hi.is_finite() {
        return Err("nothing to draw".into());
    }
    lo -= Vec3::splat(MARGIN);
    hi += Vec3::splat(MARGIN);

    let height = (hi.y - lo.y) * PX_PER_STUD;
    let front_width = (hi.x - lo.x) * PX_PER_STUD;
    let side_width = (hi.z - lo.z) * PX_PER_STUD;
    let mut svg = String::new();
    let fmt = |e: std::fmt::Error| e.to_string();
    writeln!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{:.0}\" height=\"{:.0}\">",
        front_width + side_width,
        height
    )
    .map_err(fmt)?;
    type Panel = (f32, fn(Vec3) -> f32, f32);
    let panels: [Panel; 2] = [(0.0, |p| p.x, lo.x), (front_width, |p| p.z, lo.z)];
    for (offset, horizontal, h_min) in panels {
        let point = |p: Vec3| {
            (
                offset + (horizontal(p) - h_min) * PX_PER_STUD,
                (hi.y - p.y) * PX_PER_STUD,
            )
        };
        let path = |tri: &[Vec3; 3]| {
            let [a, b, c] = tri.map(point);
            format!(
                "M{:.1} {:.1}L{:.1} {:.1}L{:.1} {:.1}Z",
                a.0, a.1, b.0, b.1, c.0, c.1
            )
        };
        let body_d: String = body_tris.iter().map(path).collect();
        let cage_d: String = cage_tris.iter().map(path).collect();
        writeln!(svg, "<path d=\"{body_d}\" fill=\"#ccc\" stroke=\"none\"/>").map_err(fmt)?;
        writeln!(
            svg,
            "<path d=\"{cage_d}\" fill=\"none\" stroke=\"#d33\" stroke-width=\"0.5\" opacity=\"0.8\"/>"
        )
        .map_err(fmt)?;
    }
    svg.push_str("</svg>\n");
    std::fs::write(out_path, svg).map_err(|e| format!("couldn't write {out_path}: {e}"))
}
