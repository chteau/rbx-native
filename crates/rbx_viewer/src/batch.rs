//! `--batch`: screenshots every place file under a directory in one process,
//! sharing the GPU device, the reflection database and the decoded-asset
//! cache across the whole run instead of paying for each — see
//! `capture::write_png_on` and `load::Resident` — for a caller photographing
//! a large corpus of files where spawning `rbxview` once per file would pay
//! that setup cost (and re-decode every asset two places share) again and
//! again.

use std::path::{Path, PathBuf};
use std::time::Instant;

use glam::Vec3;
use rbx_reflection::ReflectionDatabase;

use crate::capture::{self, Framing};
use crate::cli::Options;
use crate::gpu;
use crate::load::{self, Loaded, Resident, Toggles};
use crate::quality::QualityProfile;
use crate::toggles;

pub(crate) fn run(options: &Options, out_dir: &Path) -> Result<(), String> {
    let root = options.path();
    let inputs = collect(root)?;
    if inputs.is_empty() {
        return Err(format!(
            "no .rbxl/.rbxm/.rbxlx/.rbxmx file found under {root:?}"
        ));
    }
    std::fs::create_dir_all(out_dir)
        .map_err(|err| format!("failed to create {out_dir:?}: {err}"))?;

    let instance = gpu::instance();
    let adapter = gpu::adapter(&instance, None)?;
    let (device, queue) = gpu::device(&adapter)?;
    let database = ReflectionDatabase::embedded();
    let file_toggles = toggles(options);
    // Blocking, like the single-shot path (`Loaded::read`) — nothing here
    // streams into a next frame — but kept for the whole run rather than one
    // file: two places drawing on the same catalog decal, material pack or
    // file mesh decode it once between them.
    //
    // ponytail: unbounded over the run, so a corpus with little asset reuse
    // and huge individual assets could grow this past what fits in memory;
    // shard the run (several `--batch` invocations over subsets) if that
    // bites, rather than capping the cache here.
    let mut resident = Resident::default();
    let mut profile = options.quality().profile();
    profile.particles = options.particles();
    profile.beams = options.beams();
    profile.trails = options.trails();
    profile.gui = options.gui();

    let eye_look_at = options
        .eye_look_at()
        .map(|(eye, look_at)| (Vec3::from(eye), Vec3::from(look_at)));
    let framing = Framing {
        yaw: options.yaw(),
        pitch: options.pitch(),
        eye_look_at,
        orthographic: options.orthographic(),
    };

    let started = Instant::now();
    let mut failed = 0usize;
    for (index, input) in inputs.iter().enumerate() {
        let output = output_path(root, input, out_dir);
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create {parent:?}: {err}"))?;
        }
        eprint!("[{}/{}] {}...", index + 1, inputs.len(), input.display());
        let context = Context {
            database: &database,
            toggles: file_toggles,
            device: &device,
            queue: &queue,
            profile: &profile,
            size: options.size(),
            framing,
        };
        match render_one(input, &context, &mut resident, &output) {
            Ok(()) => eprintln!(" -> {}", output.display()),
            Err(err) => {
                eprintln!(" FAILED: {err}");
                failed += 1;
            }
        }
    }

    eprintln!(
        "rbxview: rendered {}/{} in {:.1}s",
        inputs.len() - failed,
        inputs.len(),
        started.elapsed().as_secs_f32()
    );
    if failed > 0 {
        return Err(format!("{failed} of {} file(s) failed", inputs.len()));
    }
    Ok(())
}

/// What every file in a `--batch` run shares — the GPU device, the
/// reflection database and the camera/quality options — bundled together for
/// the same reason [`Framing`] is: one argument beats several to a function
/// called once per input file.
struct Context<'a> {
    database: &'a ReflectionDatabase,
    toggles: Toggles,
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    profile: &'a QualityProfile,
    size: (u32, u32),
    framing: Framing,
}

fn render_one(
    input: &Path,
    context: &Context<'_>,
    resident: &mut Resident,
    output: &Path,
) -> Result<(), String> {
    let mut dom = load::read_place(input)?;
    if context.toggles.show_development_gui {
        load::show_development_gui(&mut dom);
    }
    let loaded = Loaded::from_dom(&dom, context.database, context.toggles, resident)
        .map_err(|err| format!("nothing to show in {input:?}: {err}"))?;
    capture::write_png_on(
        context.device,
        context.queue,
        loaded.world(),
        context.profile,
        output,
        context.size,
        context.framing,
    )
}

/// Every `.rbxl`/`.rbxm`/`.rbxlx`/`.rbxmx` file under `root`, recursively, in
/// a stable order — `root` itself if it names one directly rather than a
/// directory.
fn collect(root: &Path) -> Result<Vec<PathBuf>, String> {
    if root.is_file() {
        return Ok(vec![root.to_path_buf()]);
    }
    let mut inputs = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let entries =
            std::fs::read_dir(&dir).map_err(|err| format!("failed to read {dir:?}: {err}"))?;
        for entry in entries {
            let entry = entry.map_err(|err| format!("failed to read {dir:?}: {err}"))?;
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if is_place_file(&path) {
                inputs.push(path);
            }
        }
    }
    inputs.sort();
    Ok(inputs)
}

fn is_place_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("rbxl" | "rbxm" | "rbxlx" | "rbxmx")
    )
}

/// Where `input` (found under `root` by [`collect`]) writes its PNG under
/// `out_dir`: `root`'s own subdirectories mirrored, so a batch run never
/// collides two same-named files from different folders into one output.
fn output_path(root: &Path, input: &Path, out_dir: &Path) -> PathBuf {
    let relative = if root.is_dir() {
        input.strip_prefix(root).unwrap_or(input)
    } else {
        Path::new(input.file_name().unwrap_or(input.as_os_str()))
    };
    out_dir.join(relative).with_extension("png")
}

#[cfg(test)]
mod tests;
