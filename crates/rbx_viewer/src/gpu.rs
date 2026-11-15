//! Adapter and device acquisition, shared by the windowed and the offscreen path.

use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;

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

/// How hard material textures are cut down to fit the GPU: 0 full size,
/// 1 on an integrated GPU, 2 after an out-of-memory. Only ever goes up.
static LEAN: AtomicU8 = AtomicU8::new(0);

/// `RBX_GPU_LEAN=0|1|2` pins [`LEAN`] for testing either path anywhere.
const LEAN_VARIABLE: &str = "RBX_GPU_LEAN";

fn lean_at_least(level: u8) {
    if std::env::var_os(LEAN_VARIABLE).is_none() {
        LEAN.fetch_max(level, Ordering::Relaxed);
    }
}

/// Side of one material texture layer. Each material in a place costs four
/// of these with mips: about 22 MB at 1024, the bulk of what a place
/// allocates, so an iGPU gets 512 (5.6 MB) and a GPU that already ran out
/// gets 256.
pub(crate) fn material_resolution() -> u32 {
    let level = std::env::var(LEAN_VARIABLE)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or_else(|| LEAN.load(Ordering::Relaxed));
    match level {
        0 => 1024,
        1 => 512,
        _ => 256,
    }
}

/// The renderer's answer to running out of GPU memory: one step leaner.
pub(crate) fn lean_after_oom() {
    lean_at_least(2);
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
    if info.device_type == wgpu::DeviceType::IntegratedGpu && info.backend != wgpu::Backend::Metal {
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
    if material_resolution() < 1024 {
        eprintln!(
            "rbxview: material textures at {} px to fit this GPU's memory",
            material_resolution()
        );
    }
    Ok((device, queue))
}

/// wgpu's default handler panics, and the editor builds its renderer from
/// inside a window-message callback, where a panic cannot unwind and the
/// process aborts (0xc0000409 on Windows). An error is logged instead, the
/// first few in full and then one in a thousand, and running out of memory
/// makes the next rebuild leaner.
fn log_error(error: wgpu::Error) {
    static SEEN: AtomicU32 = AtomicU32::new(0);
    if matches!(error, wgpu::Error::OutOfMemory { .. }) {
        lean_after_oom();
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
