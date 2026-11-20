//! Adapter and device acquisition, shared by the windowed and the offscreen path.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

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

/// Every other error no scope caught, see [`log_error`]: pipelines are
/// compiled on worker threads, out of reach of an error scope pushed on the
/// thread building the renderer, so a build counts these instead.
static INVALID: AtomicU32 = AtomicU32::new(0);

pub(crate) fn invalid() -> u32 {
    INVALID.load(Ordering::Relaxed)
}

/// How far the sun's shadow map has been cut down, in halvings of its side,
/// `SHADOW_OFF` being no shadows at all. Only ever goes up, like [`LEAN`].
static SHADOW_CUT: AtomicU8 = AtomicU8::new(0);

/// Halvings of the sun map before shadows go.
const MAX_SHADOW_CUT: u8 = 3;

/// The [`Rung::shadow`] that casts no shadows.
pub(crate) const SHADOW_OFF: u8 = MAX_SHADOW_CUT + 1;

/// No sun map is cut below this side.
const MIN_SHADOW_SIDE: u32 = 256;

/// The biggest sun map this GPU starts with (see [`shadow_side_cap`]).
static SHADOW_SIDE_CAP: AtomicU32 = AtomicU32::new(u32::MAX);

/// The sun map a small-memory GPU starts at. An 8192 px map is 256 MB on
/// its own, half of the 512 MB laptop iGPU that failed; that laptop built
/// Level21 fine at 4096.
const SMALL_GPU_SHADOW_SIDE: u32 = 4096;

/// Whether the adapter has little memory of its own: integrated, except
/// Apple's, which share all of memory, and software adapters (WARP,
/// llvmpipe), which draw on the CPU. `RBX_GPU_LEAN` (see [`pinned_lean`])
/// pins the answer either way.
fn small_memory(device_type: wgpu::DeviceType, backend: wgpu::Backend) -> bool {
    pinned_lean().map_or(
        (device_type == wgpu::DeviceType::IntegratedGpu && backend != wgpu::Backend::Metal)
            || device_type == wgpu::DeviceType::Cpu,
        |lean| lean > 0,
    )
}

/// The biggest sun map side to start at, so a small GPU's first build
/// already fits rather than failing and walking down from 8192.
fn shadow_side_cap(small: bool, max_texture_side: u32) -> u32 {
    let cap = if small {
        SMALL_GPU_SHADOW_SIDE
    } else {
        u32::MAX
    };
    cap.min(max_texture_side)
}

/// `full` cut `cut` times, never past this GPU's cap or under
/// [`MIN_SHADOW_SIDE`] (unless `full` itself is smaller).
fn cut_side(full: u32, cut: u8, cap: u32) -> u32 {
    let base = full.min(cap);
    (base >> cut).max(MIN_SHADOW_SIDE).min(base)
}

/// The level the host last asked to draw at. A place built after that
/// (a reopen, a plugin widget) starts there rather than at the top and
/// walking down from it.
static PREFERRED_LEVEL: AtomicU8 = AtomicU8::new(QualityLevel::MAX);

/// Builds from here on start at or under `level`. The editor calls it with
/// the saved quality before it opens a place, so a Manual 1 never builds at
/// the top first.
pub fn prefer_level(level: QualityLevel) {
    PREFERRED_LEVEL.store(level.resolved(), Ordering::Relaxed);
}

/// `RBX_GPU_BUDGET_MB=n` pretends the GPU holds `n` MB: a build whose shadow
/// map and material arrays add up to more counts as out of memory, so the
/// walk down a small card takes can be watched on any machine.
const BUDGET_VARIABLE: &str = "RBX_GPU_BUDGET_MB";

fn budget_bytes() -> Option<u64> {
    let mb = std::env::var(BUDGET_VARIABLE)
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()?;
    Some(mb << 20)
}

/// One way of building a place on the GPU: a graphics quality level, a
/// material size (a [`LEAN`]) and a sun map cut (a [`SHADOW_CUT`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rung {
    pub(crate) level: u8,
    pub(crate) lean: u8,
    pub(crate) shadow: u8,
}

impl Rung {
    /// Where a build starts: no higher than `cap` or the level last asked
    /// for, at the sizes earlier out-of-memory steps settled on.
    pub(crate) fn start(cap: u8) -> Rung {
        Rung::starting(cap, PREFERRED_LEVEL.load(Ordering::Relaxed))
    }

    fn starting(cap: u8, preferred: u8) -> Rung {
        Rung {
            level: cap.min(preferred),
            lean: lean(),
            shadow: SHADOW_CUT.load(Ordering::Relaxed),
        }
    }

    /// The level a place built at this rung, after starting at `first`, may
    /// be drawn up to: `cap` when the first try fit (it only started lower
    /// because a lower level was asked for), this rung's level when running
    /// out of memory walked down to it.
    pub(crate) fn ceiling(self, first: Rung, cap: u8) -> u8 {
        if self == first {
            cap
        } else {
            self.level
        }
    }

    /// What to build at after this ran out of GPU memory, or `None` at the
    /// bottom. The sun map is what allocates one big texture at once, so it
    /// halves first, down to 256 px. Then materials go to 256 px, then the
    /// shadows go, then the level halves down to the lowest (multisampling
    /// and post effects), then the materials halve again down to 64 px.
    pub(crate) fn step_down(self) -> Option<Rung> {
        let side = self.sun_side(self.shadow);
        if self.shadow < MAX_SHADOW_CUT && side > MIN_SHADOW_SIDE {
            return Some(Rung {
                shadow: self.shadow + 1,
                ..self
            });
        }
        if self.lean < 2 {
            return Some(Rung { lean: 2, ..self });
        }
        if self.shadow < SHADOW_OFF && side > 0 {
            return Some(Rung {
                shadow: SHADOW_OFF,
                ..self
            });
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

    /// The sun map side this level gets after `cut` halvings, 0 where it
    /// casts no shadows.
    fn sun_side(self, cut: u8) -> u32 {
        let profile = QualityLevel::Level(self.level).profile();
        if !profile.shadows || cut >= SHADOW_OFF {
            return 0;
        }
        cut_side(
            profile.shadow_map_size,
            cut,
            SHADOW_SIDE_CAP.load(Ordering::Relaxed),
        )
    }

    /// Makes material uploads and sun maps from here on use this rung's size.
    pub(crate) fn apply(self) {
        lean_at_least(self.lean);
        SHADOW_CUT.fetch_max(self.shadow, Ordering::Relaxed);
    }

    pub(crate) fn quality(self) -> QualityLevel {
        QualityLevel::Level(self.level)
    }

    /// `profile` for a level at or under this rung's, with the sun map cut.
    pub(crate) fn limit(
        self,
        mut profile: crate::quality::QualityProfile,
    ) -> crate::quality::QualityProfile {
        if self.shadow >= SHADOW_OFF {
            profile.shadows = false;
        } else {
            profile.shadow_map_size = cut_side(
                profile.shadow_map_size,
                self.shadow,
                SHADOW_SIDE_CAP.load(Ordering::Relaxed),
            );
        }
        profile
    }

    /// The profile a build at this rung uses.
    pub(crate) fn profile(self) -> crate::quality::QualityProfile {
        self.limit(self.quality().profile())
    }

    /// Whether the pretend budget (see [`BUDGET_VARIABLE`]) is too small for
    /// this rung with `layers` material layers.
    pub(crate) fn over_budget(self, layers: usize) -> bool {
        let Some(budget) = budget_bytes() else {
            return false;
        };
        let side = u64::from(self.sun_side(self.shadow));
        let material = u64::from(resolution_at(self.lean));
        let bytes = side * side * 4 + layers as u64 * 4 * material * material * 4 * 4 / 3;
        bytes > budget
    }
}

impl std::fmt::Display for Rung {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Level{:02} with {} px materials",
            self.level,
            resolution_at(self.lean)
        )?;
        match self.sun_side(self.shadow) {
            0 => write!(f, ", no shadows"),
            side => write!(f, ", {side} px shadows"),
        }
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
///
/// On a device that cannot take the full surface shader (see
/// [`overflows_on_surface_shader`]) local lights are left unshadowed.
pub(crate) fn shader(device: &wgpu::Device, label: &str, source: &str) -> wgpu::ShaderModule {
    let source = if overflows_on_surface_shader(&device.adapter_info()) {
        source.replace(LOCAL_SHADOW_LOOKUP, "1.0")
    } else {
        source.to_owned()
    };
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(
            format!("diagnostic(off, derivative_uniformity);\n{source}").into(),
        ),
    })
}

/// Where `lights.wgsl` asks how much of a local light reaches a fragment.
const LOCAL_SHADOW_LOOKUP: &str = "local_light_visibility(receiver.position, facing, index)";

/// WARP (Microsoft Basic Render Driver) on Windows on ARM: what a Windows VM
/// with no DirectX 12 driver gets, VMware Fusion on an Apple Silicon Mac
/// for one, whether the build is ARM64 or x64 under emulation. Its shader
/// compiler overflows its own thread's stack on the surface shader, and the
/// process dies at the first frame that draws a part (0xC0000409, no panic).
/// The local lights' shadow lookup is what tips it over: without it the
/// shader builds and draws. x64 WARP takes the full shader.
fn overflows_on_surface_shader(info: &wgpu::AdapterInfo) -> bool {
    info.backend == wgpu::Backend::Dx12
        && info.device_type == wgpu::DeviceType::Cpu
        && on_arm64_windows()
}

/// Whether the machine is ARM64 Windows, the process native or emulated.
fn on_arm64_windows() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{
            IMAGE_FILE_MACHINE, IMAGE_FILE_MACHINE_ARM64,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, IsWow64Process2};
        let mut process: IMAGE_FILE_MACHINE = 0;
        let mut native: IMAGE_FILE_MACHINE = 0;
        // SAFETY: the pseudo-handle needs no closing and both out-pointers
        // are valid for the call.
        let answered = unsafe { IsWow64Process2(GetCurrentProcess(), &mut process, &mut native) };
        answered != 0 && native == IMAGE_FILE_MACHINE_ARM64
    }
    #[cfg(not(windows))]
    false
}

/// Lib tests open adapters and devices from many threads at once, and the
/// NVIDIA driver has segfaulted doing that: under test it's one at a time.
#[cfg(test)]
static OPENING: Mutex<()> = Mutex::new(());

/// Picks an adapter, optionally one able to present to `surface`.
///
/// The chosen adapter and backend are printed: which of Vulkan, GL or a software
/// fallback answered is the first thing to check when a frame looks wrong.
pub(crate) fn adapter(
    instance: &wgpu::Instance,
    surface: Option<&wgpu::Surface<'_>>,
) -> Result<wgpu::Adapter, String> {
    #[cfg(test)]
    let _one = OPENING.lock().unwrap_or_else(|e| e.into_inner());
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
    *CHOSEN.lock().unwrap_or_else(|e| e.into_inner()) = Some(describe(&info));
    Ok(adapter)
}

/// The adapter [`adapter`] last picked, for a crash report.
static CHOSEN: Mutex<Option<String>> = Mutex::new(None);

/// The adapter the renderer last opened, described as in [`describe_adapter`],
/// or `None` before any was.
pub fn chosen_adapter() -> Option<String> {
    CHOSEN.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn describe(info: &wgpu::AdapterInfo) -> String {
    format!(
        "{} ({:?}, {:?}, driver {} {})",
        info.name, info.backend, info.device_type, info.driver, info.driver_info
    )
}

/// Why no GPU could draw a place, for the viewport to say instead of a
/// picture: every adapter that was tried and what the last one failed on.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Unavailable {
    /// Each adapter tried, as [`describe_adapter`] names it.
    pub found: Vec<String>,
    pub reason: String,
}

impl Unavailable {
    pub(crate) fn found(&mut self, adapter: &wgpu::Adapter) {
        let info = adapter.get_info();
        self.found.push(format!(
            "{} ({:?}, {:?})",
            info.name, info.backend, info.device_type
        ));
    }
}

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let found = if self.found.is_empty() {
            "no GPU adapter".to_owned()
        } else {
            self.found.join(", ")
        };
        write!(
            f,
            "Your graphics driver doesn't support what the 3D view needs \
             (DirectX 12, Vulkan or Metal). Found: {found}."
        )?;
        if !self.reason.is_empty() {
            write!(f, " ({})", self.reason)?;
        }
        Ok(())
    }
}

/// Every adapter each backend offers, one line each with what the renderer
/// cares about: what `--gpu-probe` prints first.
pub fn adapters_report() -> String {
    let mut report = String::new();
    for backends in [
        wgpu::Backends::DX12,
        wgpu::Backends::VULKAN,
        wgpu::Backends::METAL,
        wgpu::Backends::GL,
    ] {
        let instance = instance_for(backends);
        for adapter in pollster::block_on(instance.enumerate_adapters(backends)) {
            let limits = adapter.limits();
            let downlevel = adapter.get_downlevel_capabilities();
            let _ = writeln!(
                report,
                "{}: max texture {}, {} storage buffers/stage, {} sampled textures/stage, \
                 WebGPU compliant {}, shader model {:?}",
                describe(&adapter.get_info()),
                limits.max_texture_dimension_2d,
                limits.max_storage_buffers_per_shader_stage,
                limits.max_sampled_textures_per_shader_stage,
                downlevel.is_webgpu_compliant(),
                downlevel.shader_model,
            );
        }
    }
    if report.is_empty() {
        report.push_str("no GPU adapter on any backend\n");
    }
    report
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
    Ok(describe(&adapter(&instance, None)?.get_info()))
}

/// Opens a device and command queue on the adapter, with errors logged
/// rather than panicking (see [`log_error`]).
pub(crate) fn device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue), String> {
    #[cfg(test)]
    let _one = OPENING.lock().unwrap_or_else(|e| e.into_inner());
    let info = adapter.get_info();
    // Everything integrated but Apple's has a small carve-out (512 MB on the
    // laptop that crashed).
    let small = small_memory(info.device_type, info.backend);
    if small && pinned_lean().is_none() {
        // A software adapter (WARP, llvmpipe) draws on the CPU out of system
        // memory: full-size materials and an 8192 px sun map made a
        // Windows-on-ARM VM take seconds per frame, so it starts at 256 px.
        lean_at_least(if info.device_type == wgpu::DeviceType::Cpu {
            2
        } else {
            1
        });
    }
    SHADOW_SIDE_CAP.store(
        shadow_side_cap(small, adapter.limits().max_texture_dimension_2d),
        Ordering::Relaxed,
    );
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
    let counter = match error {
        wgpu::Error::OutOfMemory { .. } => &OOMS,
        _ => &INVALID,
    };
    counter.fetch_add(1, Ordering::Relaxed);
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
    use super::{cut_side, shadow_side_cap, small_memory, Rung, MAX_LEAN, SHADOW_OFF};
    use crate::quality::QualityLevel;

    fn ladder(from: Rung) -> Vec<Rung> {
        std::iter::successors(Some(from), |rung| rung.step_down()).collect()
    }

    fn r(level: u8, lean: u8, shadow: u8) -> Rung {
        Rung {
            level,
            lean,
            shadow,
        }
    }

    #[test]
    fn the_warp_arm64_workaround_still_finds_the_local_shadow_lookup() {
        let lights = include_str!("renderer/lights.wgsl");
        assert_eq!(lights.matches(super::LOCAL_SHADOW_LOOKUP).count(), 1);
    }

    #[test]
    fn the_sun_map_shrinks_before_materials_then_shadows_go_before_the_level_drops() {
        let steps: Vec<_> = ladder(r(21, 1, 0))
            .iter()
            .map(|rung| (rung.level, rung.lean, rung.shadow))
            .collect();
        assert_eq!(
            steps[..8],
            [
                (21, 1, 0),
                (21, 1, 1),
                (21, 1, 2),
                (21, 1, 3),
                (21, 2, 3),
                (21, 2, SHADOW_OFF),
                (10, 2, SHADOW_OFF),
                (5, 2, SHADOW_OFF)
            ]
        );
    }

    #[test]
    fn a_level_without_shadows_skips_the_sun_map_steps() {
        let steps: Vec<_> = ladder(r(1, 0, 0))
            .iter()
            .map(|rung| (rung.lean, rung.shadow))
            .collect();
        assert_eq!(steps, [(0, 0), (2, 0), (3, 0), (4, 0)]);
    }

    #[test]
    fn the_sun_map_halves_three_times_then_goes() {
        // Level 11 has a 4096 map.
        let sides: Vec<_> = ladder(r(11, 2, 0))
            .iter()
            .take_while(|rung| rung.level == 11)
            .map(|rung| rung.sun_side(rung.shadow))
            .collect();
        assert_eq!(sides, [4096, 2048, 1024, 512, 0]);
    }

    #[test]
    fn the_bottom_rung_has_nothing_below_it() {
        assert_eq!(r(QualityLevel::MIN, MAX_LEAN, 0).step_down(), None);
    }

    #[test]
    fn every_ladder_ends_and_never_climbs() {
        for level in QualityLevel::MIN..=QualityLevel::MAX {
            for lean in 0..=MAX_LEAN {
                let rungs = ladder(r(level, lean, 0));
                assert!(rungs.len() <= 20);
                for pair in rungs.windows(2) {
                    assert!(
                        pair[1].level <= pair[0].level
                            && pair[1].lean >= pair[0].lean
                            && pair[1].shadow >= pair[0].shadow
                    );
                    assert_ne!(pair[0], pair[1]);
                }
                let last = rungs.last().unwrap();
                assert_eq!((last.level, last.lean), (1, MAX_LEAN));
            }
        }
    }

    #[test]
    fn a_rung_names_what_it_builds() {
        assert_eq!(
            r(1, 3, 0).to_string(),
            "Level01 with 128 px materials, no shadows"
        );
        assert_eq!(
            r(21, 2, 2).to_string(),
            format!(
                "Level21 with 256 px materials, {} px shadows",
                r(21, 2, 2).sun_side(2)
            )
        );
    }

    #[test]
    fn a_dropped_sun_map_is_one_texel_and_a_cut_one_is_smaller() {
        let full = QualityLevel::Level(11).profile();
        assert!(!r(11, 0, SHADOW_OFF).limit(full).shadows);
        assert_eq!(
            r(11, 0, 2).limit(full).shadow_map_size,
            full.shadow_map_size / 4
        );
    }

    #[test]
    fn a_saved_low_level_is_where_the_first_build_starts() {
        let manual_1 = QualityLevel::Level(1).resolved();
        assert_eq!(Rung::starting(QualityLevel::MAX, manual_1).level, 1);
        assert_eq!(Rung::starting(5, QualityLevel::MAX).level, 5);
    }

    #[test]
    fn a_build_that_fit_can_go_up_to_the_cap_and_one_that_walked_down_cannot() {
        let first = r(1, 1, 0);
        assert_eq!(first.ceiling(first, 21), 21);
        assert_eq!(r(1, 2, 0).ceiling(first, 21), 1);
        assert_eq!(r(10, 2, SHADOW_OFF).ceiling(r(21, 1, 0), 21), 10);
    }

    #[test]
    fn a_small_gpu_starts_with_a_sun_map_that_fits() {
        use wgpu::{Backend, DeviceType};
        // Only meaningful without RBX_GPU_LEAN pinning the answer.
        if std::env::var("RBX_GPU_LEAN").is_err() {
            assert!(small_memory(DeviceType::IntegratedGpu, Backend::Dx12));
            assert!(small_memory(DeviceType::IntegratedGpu, Backend::Vulkan));
            assert!(!small_memory(DeviceType::IntegratedGpu, Backend::Metal));
            assert!(!small_memory(DeviceType::DiscreteGpu, Backend::Dx12));
            assert!(small_memory(DeviceType::Cpu, Backend::Dx12));
            assert!(small_memory(DeviceType::Cpu, Backend::Vulkan));
        }
        let small = shadow_side_cap(true, 16384);
        assert_eq!(small, 4096);
        assert_eq!(cut_side(8192, 0, small), 4096);
        assert_eq!(cut_side(8192, 1, small), 2048);
        assert_eq!(cut_side(2048, 0, small), 2048);
        assert_eq!(cut_side(8192, 0, shadow_side_cap(false, 16384)), 8192);
        // A GPU whose textures top out under the map is capped there too.
        assert_eq!(cut_side(8192, 0, shadow_side_cap(false, 4096)), 4096);
    }
}
