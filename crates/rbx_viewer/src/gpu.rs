//! Adapter and device acquisition, shared by the windowed and the offscreen path.

use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;

use crate::quality::QualityLevel;

pub(crate) fn instance() -> wgpu::Instance {
    instance_for(backend_order()[0])
}

/// An instance limited to `backends`, with the rest of wgpu's knobs still
/// read from the environment.
pub(crate) fn instance_for(backends: wgpu::Backends) -> wgpu::Instance {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
    descriptor.backends = backends;
    wgpu::Instance::new(descriptor)
}

/// The backends to try, in order. `WGPU_BACKEND` (e.g. `vulkan`, `dx12`)
/// pins one. Windows goes DX12 first: an AMD laptop iGPU with 512 MB of
/// dedicated memory ran out of memory on Vulkan opening any place, while
/// DX12 spills into shared memory like every other D3D app does.
pub(crate) fn backend_order() -> Vec<wgpu::Backends> {
    if let Some(forced) = wgpu::Backends::from_env() {
        return vec![forced];
    }
    if cfg!(windows) {
        vec![
            wgpu::Backends::DX12,
            wgpu::Backends::VULKAN | wgpu::Backends::GL,
        ]
    } else {
        vec![wgpu::Backends::default()]
    }
}

/// How hard material textures are cut down to fit the GPU, each step halving
/// their side: 0 full size, 1 on an integrated GPU, more for every rung an
/// out-of-memory walks down (see [`Rung`]). Only ever goes up.
static LEAN: AtomicU8 = AtomicU8::new(0);

/// The deepest [`LEAN`]: 64 px material layers.
pub(crate) const MAX_LEAN: u8 = 4;

/// `RBX_GPU_LEAN=0..4` sets where [`LEAN`] starts whatever the GPU, for
/// testing either path anywhere. Running out of memory still steps on down
/// from there.
const LEAN_VARIABLE: &str = "RBX_GPU_LEAN";

fn pinned_lean() -> Option<u8> {
    std::env::var(LEAN_VARIABLE)
        .ok()
        .and_then(|v| v.trim().parse::<u8>().ok())
        .map(|lean| lean.min(MAX_LEAN))
}

pub(crate) fn lean() -> u8 {
    LEAN.load(Ordering::Relaxed).max(pinned_lean().unwrap_or(0))
}

fn lean_at_least(level: u8) {
    LEAN.fetch_max(level.min(MAX_LEAN), Ordering::Relaxed);
}

/// Side of one material texture layer. Each material in a place costs four
/// of these with mips: about 22 MB at 1024, the bulk of what a place
/// allocates, so an iGPU gets 512 (5.6 MB) and a GPU that ran out gets less.
pub(crate) fn material_resolution() -> u32 {
    resolution_at(lean())
}

fn resolution_at(lean: u8) -> u32 {
    1024 >> lean.min(MAX_LEAN)
}

/// Out-of-memory errors no error scope caught, see [`log_error`]: a
/// renderer that sees this move since it was built has something invalid in
/// it and steps down a [`Rung`].
static OOMS: AtomicU32 = AtomicU32::new(0);

pub(crate) fn ooms() -> u32 {
    OOMS.load(Ordering::Relaxed)
}

/// One way of building a place on the GPU: a graphics quality level and a
/// material size (a [`LEAN`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rung {
    pub(crate) level: u8,
    pub(crate) lean: u8,
}

impl Rung {
    /// What to build at after this ran out of GPU memory, or `None` at the
    /// bottom. Materials go to 256 px first: they are most of what a place
    /// allocates and the cheapest to lose. Then the level halves down to the
    /// lowest, which shrinks the shadow map, drops multisampling and post
    /// effects, and in the bottom bands drops shadows altogether. Then the
    /// materials halve again down to 64 px.
    pub(crate) fn step_down(self) -> Option<Rung> {
        if self.lean < 2 {
            return Some(Rung { lean: 2, ..self });
        }
        if self.level > QualityLevel::MIN {
            let level = (self.level / 2).max(QualityLevel::MIN);
            return Some(Rung { level, ..self });
        }
        (self.lean < MAX_LEAN).then_some(Rung {
            lean: self.lean + 1,
            ..self
        })
    }

    /// Makes material uploads from here on use this rung's size.
    pub(crate) fn apply(self) {
        lean_at_least(self.lean);
    }

    pub(crate) fn quality(self) -> QualityLevel {
        QualityLevel::Level(self.level)
    }
}

impl std::fmt::Display for Rung {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Level{:02} with {} px materials",
            self.level,
            resolution_at(self.lean)
        )
    }
}

/// Frees what dropped resources still hold: wgpu keeps a texture written
/// through the queue alive until the next submission finishes, so a failed
/// build's memory is still taken when the next, leaner one starts.
pub(crate) fn release(device: &wgpu::Device, queue: &wgpu::Queue) {
    queue.submit([]);
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
}

/// Compiles one WGSL module.
///
/// Every shader goes through here for the one directive it prepends. WGSL
/// forbids an implicit derivative (`textureSample`, `dpdx`) outside uniform
/// control flow, and a browser's compiler enforces that as an error; naga,
/// which compiles for every native backend, never checked it, and this
/// renderer samples behind per-fragment branches throughout. Turning the
/// check off is what the native build already had: the GPU computes the
/// derivative per 2x2 quad either way.
pub(crate) fn shader(device: &wgpu::Device, label: &str, source: &str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(
            format!("diagnostic(off, derivative_uniformity);\n{source}").into(),
        ),
    })
}

/// Picks an adapter, optionally one able to present to `surface`.
///
/// The chosen adapter and backend are printed: which of Vulkan, GL or a software
/// fallback answered is the first thing to check when a frame looks wrong.
pub(crate) fn adapter(
    instance: &wgpu::Instance,
    surface: Option<&wgpu::Surface<'_>>,
) -> Result<wgpu::Adapter, String> {
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: surface,
        ..Default::default()
    }))
    .map_err(|err| format!("no usable GPU adapter: {err}"))?;

    let info = adapter.get_info();
    eprintln!(
        "rbxview: {} on {:?} ({:?})",
        info.name, info.backend, info.device_type
    );
    Ok(adapter)
}

/// Names the adapter [`adapter`] would pick, for a report that has to say which
/// GPU produced a number.
///
/// Asks for an adapter of its own rather than holding on to the one a device was
/// opened on: the answer is wanted once, off any hot path, and going through
/// [`adapter`] is what makes it name the card the offscreen path actually
/// renders on rather than a second, independently-chosen guess.
pub fn describe_adapter() -> Result<String, String> {
    let instance = instance();
    let info = adapter(&instance, None)?.get_info();
    Ok(format!(
        "{} ({:?}, {:?}, driver {} {})",
        info.name, info.backend, info.device_type, info.driver, info.driver_info
    ))
}

/// Opens a device and command queue on the adapter, with errors logged
/// rather than panicking (see [`log_error`]).
pub(crate) fn device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue), String> {
    let info = adapter.get_info();
    // Apple's GPUs share all of memory; everything else integrated has a
    // small carve-out (512 MB on the laptop that crashed).
    if info.device_type == wgpu::DeviceType::IntegratedGpu
        && info.backend != wgpu::Backend::Metal
        && pinned_lean().is_none()
    {
        lean_at_least(1);
    }
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("rbxview"),
        // Small allocator blocks rather than wgpu's default 128-256 MB ones:
        // the renderer's own allocations are a few large textures made at
        // load, and the default left a small place holding ~4x what it
        // actually used, hundreds of megabytes a low-end GPU does not have.
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        ..Default::default()
    }))
    .map_err(|err| format!("failed to open a GPU device: {err}"))?;
    device.on_uncaptured_error(Arc::new(log_error));
    device.set_device_lost_callback(|reason, message| {
        eprintln!("rbxview: GPU device lost ({reason:?}): {message}");
    });
    eprintln!(
        "rbxview: material textures at {} px{}",
        material_resolution(),
        if material_resolution() < 1024 {
            " to fit this GPU's memory"
        } else {
            " (full size)"
        }
    );
    Ok((device, queue))
}

/// wgpu's default handler panics, and the editor builds its renderer from
/// inside a window-message callback, where a panic cannot unwind and the
/// process aborts (0xc0000409 on Windows). An error is logged instead, the
/// first few in full and then one in a thousand, and running out of memory
/// is counted for the renderer to step down (see [`ooms`]).
fn log_error(error: wgpu::Error) {
    static SEEN: AtomicU32 = AtomicU32::new(0);
    if matches!(error, wgpu::Error::OutOfMemory { .. }) {
        OOMS.fetch_add(1, Ordering::Relaxed);
    }
    let seen = SEEN.fetch_add(1, Ordering::Relaxed);
    if seen < 16 || seen.is_multiple_of(1000) {
        eprintln!("rbxview: GPU error #{}: {error}", seen + 1);
    }
}

/// A device for a unit test that builds real GPU state, or `None` where the
/// machine has no adapter at all (CI, say), such a test reports itself
/// skipped rather than failing, since what it checks is not the GPU.
#[cfg(test)]
pub(crate) fn for_tests() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = instance();
    let adapter = adapter(&instance, None)
        .map_err(|err| eprintln!("skipped: {err}"))
        .ok()?;
    device(&adapter)
        .map_err(|err| eprintln!("skipped: {err}"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::{Rung, MAX_LEAN};
    use crate::quality::QualityLevel;

    fn ladder(from: Rung) -> Vec<Rung> {
        std::iter::successors(Some(from), |rung| rung.step_down()).collect()
    }

    #[test]
    fn materials_shrink_to_256_before_the_level_drops() {
        let rungs = ladder(Rung { level: 21, lean: 1 });
        assert_eq!(rungs[1], Rung { level: 21, lean: 2 });
        assert_eq!(rungs[2], Rung { level: 10, lean: 2 });
    }

    #[test]
    fn the_level_halves_to_the_lowest_then_materials_shrink_to_the_end() {
        let levels: Vec<(u8, u8)> = ladder(Rung { level: 21, lean: 0 })
            .iter()
            .map(|rung| (rung.level, rung.lean))
            .collect();
        assert_eq!(
            levels,
            [
                (21, 0),
                (21, 2),
                (10, 2),
                (5, 2),
                (2, 2),
                (1, 2),
                (1, 3),
                (1, 4)
            ]
        );
    }

    #[test]
    fn the_bottom_rung_has_nothing_below_it() {
        let bottom = Rung {
            level: QualityLevel::MIN,
            lean: MAX_LEAN,
        };
        assert_eq!(bottom.step_down(), None);
    }

    #[test]
    fn every_ladder_ends_and_never_climbs() {
        for level in QualityLevel::MIN..=QualityLevel::MAX {
            for lean in 0..=MAX_LEAN {
                let rungs = ladder(Rung { level, lean });
                assert!(rungs.len() <= 12);
                for pair in rungs.windows(2) {
                    assert!(pair[1].level <= pair[0].level && pair[1].lean >= pair[0].lean);
                    assert_ne!(pair[0], pair[1]);
                }
                assert_eq!(rungs.last().map(|r| (r.level, r.lean)), Some((1, MAX_LEAN)));
            }
        }
    }

    #[test]
    fn a_rung_names_its_level_and_material_size() {
        assert_eq!(
            Rung { level: 1, lean: 3 }.to_string(),
            "Level01 with 128 px materials"
        );
    }
}
