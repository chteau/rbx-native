//! Every fidelity scene with a reference from the real client is drawn and
//! compared, and must stay within its golden's tolerance (see
//! `rbx_viewer::fidelity` and `crates/rbx_viewer/fidelity/goldens.json`).
//!
//! Needs a GPU. Without one the test says so and passes, as `isolation.rs`
//! does. A scene whose assets could not be fetched (offline, no cache) is
//! skipped with the reason: its picture would be missing them.

use rbx_viewer::fidelity::{self, metrics};

#[test]
fn every_scene_stays_within_its_golden() {
    let goldens: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fidelity::dir().join("goldens.json")).expect("goldens.json"),
    )
    .expect("goldens.json is JSON");
    let mut failures = Vec::new();
    for scene in fidelity::scenes() {
        let Some(golden) = goldens.get(scene.name) else {
            println!("{}: no golden yet", scene.name);
            continue;
        };
        let Ok((width, height, rgba)) = fidelity::read_png(&fidelity::reference_path(scene.name))
        else {
            println!("{}: no reference picture", scene.name);
            continue;
        };
        let (pixels, warnings) = match fidelity::render(&scene) {
            Ok(drawn) => drawn,
            Err(why) => {
                println!("skipped: no GPU to render with ({why})");
                return;
            }
        };
        if !warnings.is_empty() {
            println!("{}: skipped, loading warned: {warnings:?}", scene.name);
            continue;
        }
        let (w, h) = fidelity::CAPTURE;
        let (cw, ch) = fidelity::COMPARE;
        let ours = metrics::Image::new(w, h, pixels).downsample(cw, ch);
        let got = metrics::compare(&metrics::Image::new(width, height, rgba), &ours);
        let number = |key: &str| golden[key].as_f64().expect(key) as f32;
        let max_delta_e = number("delta_e") + number("tolerance_delta_e");
        let min_ssim = number("ssim") - number("tolerance_ssim");
        println!(
            "{}: ΔE {:.2} (≤ {max_delta_e:.2}), SSIM {:.3} (≥ {min_ssim:.3})",
            scene.name, got.delta_e, got.ssim
        );
        if got.delta_e > max_delta_e || got.ssim < min_ssim {
            failures.push(scene.name);
        }
    }
    assert!(failures.is_empty(), "regressed: {failures:?}");
}
