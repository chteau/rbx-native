//! Adapter and device acquisition, shared by the windowed and the offscreen path.

pub(crate) fn instance() -> wgpu::Instance {
    // `from_env` so a backend can be forced with WGPU_BACKEND while debugging.
    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env())
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

/// Opens a device and command queue on the adapter.
pub(crate) fn device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue), String> {
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("rbxview"),
        ..Default::default()
    }))
    .map_err(|err| format!("failed to open a GPU device: {err}"))
}

/// A device for a unit test that builds real GPU state, or `None` where the
/// machine has no adapter at all (CI, say) — such a test reports itself
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
