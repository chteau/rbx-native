//! The fidelity report: draws every scene, compares it with the real
//! client's reference and writes, per scene, our frame, a ΔE heatmap and a
//! side-by-side (real | ours, or real | before | after when `--before`
//! holds an earlier run's frames), then a table of the metrics and the
//! regions' mean colours.
//!
//! ```text
//! cargo run -p rbx_viewer --release --example fidelity -- --out DIR \
//!     [--real DIR] [--before DIR] [--only a,b] [--write-goldens]
//! ```
//!
//! `--real` takes full-size captures (`<scene>.png`, as `record_render_goldens`
//! writes them) instead of the committed references. `--write-goldens`
//! records this run's metrics as the goldens, each with its tolerance.

use std::path::{Path, PathBuf};

use rbx_viewer::fidelity::{self, metrics, metrics::Image};
use serde_json::json;

/// How far a scene may slip before `tests/fidelity.rs` fails it.
const TOLERANCE_DELTA_E: f32 = 0.6;
const TOLERANCE_SSIM: f32 = 0.02;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from)
    };
    let out = flag("--out").expect("--out DIR");
    let real = flag("--real");
    let before = flag("--before");
    let only: Vec<String> = flag("--only")
        .map(|list| {
            list.to_string_lossy()
                .split(',')
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let write_goldens = args.iter().any(|a| a == "--write-goldens");
    let (cw, ch) = fidelity::COMPARE;
    let (w, h) = fidelity::CAPTURE;
    let goldens_path = fidelity::dir().join("goldens.json");
    let mut goldens: serde_json::Map<String, serde_json::Value> =
        std::fs::read_to_string(&goldens_path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
    let mut report = serde_json::Map::new();

    println!(
        "{:<26} {:>7} {:>7} {:>7}  regions (real → ours)",
        "scene", "ΔE", "ΔE p90", "SSIM"
    );
    for scene in fidelity::scenes() {
        if !only.is_empty() && !only.iter().any(|o| o == scene.name) {
            continue;
        }
        let (pixels, warnings) = fidelity::render(&scene).expect("render");
        for warning in &warnings {
            eprintln!("{}: {warning}", scene.name);
        }
        let ours_full = Image::new(w, h, pixels);
        fidelity::write_png(&out.join(format!("{}-ours.png", scene.name)), (w, h), &ours_full.rgba)
            .expect("write");
        let Some(reference) = reference(scene.name, real.as_deref()) else {
            println!("{:<26} no reference", scene.name);
            continue;
        };
        let ours = ours_full.downsample(cw, ch);
        let got = metrics::compare(&reference, &ours);
        let regions: Vec<String> = scene
            .regions
            .iter()
            .map(|(name, rect)| {
                let (a, b) = (
                    metrics::mean_color(&reference, *rect),
                    metrics::mean_color(&ours, *rect),
                );
                format!("{name} {a:?} → {b:?}")
            })
            .collect();
        println!(
            "{:<26} {:>7.2} {:>7.2} {:>7.3}  {}",
            scene.name,
            got.delta_e,
            got.delta_e_p90,
            got.ssim,
            regions.join("; ")
        );
        let heat = metrics::heatmap(&reference, &ours);
        let mut row = vec![reference.clone()];
        if let Some(before) = before
            .as_ref()
            .and_then(|dir| full(&dir.join(format!("{}-ours.png", scene.name))))
        {
            row.push(before.downsample(cw, ch));
        }
        row.push(ours);
        row.push(heat.clone());
        let row: Vec<&Image> = row.iter().collect();
        let joined = metrics::side_by_side(&row);
        fidelity::write_png(
            &out.join(format!("{}-side-by-side.png", scene.name)),
            (joined.width, joined.height),
            &joined.rgba,
        )
        .expect("write");
        fidelity::write_png(
            &out.join(format!("{}-heatmap.png", scene.name)),
            (heat.width, heat.height),
            &heat.rgba,
        )
        .expect("write");
        let mut entry = got.to_json();
        entry["regions"] = json!(regions);
        report.insert(scene.name.to_owned(), entry);
        if write_goldens {
            let mut golden = got.to_json();
            golden["tolerance_delta_e"] = json!(TOLERANCE_DELTA_E);
            golden["tolerance_ssim"] = json!(TOLERANCE_SSIM);
            golden["about"] = json!(scene.about);
            goldens.insert(scene.name.to_owned(), golden);
        }
    }
    std::fs::write(
        out.join("report.json"),
        serde_json::to_string_pretty(&report).expect("json") + "\n",
    )
    .expect("write report");
    if write_goldens {
        std::fs::write(
            &goldens_path,
            serde_json::to_string_pretty(&goldens).expect("json") + "\n",
        )
        .expect("write goldens");
        println!("wrote {}", goldens_path.display());
    }
}

fn full(path: &Path) -> Option<Image> {
    let (width, height, rgba) = fidelity::read_png(path).ok()?;
    Some(Image::new(width, height, rgba))
}

/// The reference at [`fidelity::COMPARE`]: a full-size capture from `real`
/// downsampled, or the committed one.
fn reference(name: &str, real: Option<&Path>) -> Option<Image> {
    let (cw, ch) = fidelity::COMPARE;
    match real {
        Some(dir) => full(&dir.join(format!("{name}.png"))).map(|image| image.downsample(cw, ch)),
        None => full(&fidelity::reference_path(name)),
    }
}
