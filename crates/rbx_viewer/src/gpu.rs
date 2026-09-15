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

/// Opens a device and command queue on the adapter.
pub(crate) fn device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue), String> {
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("rbxview"),
        ..Default::default()
    }))
    .map_err(|err| format!("failed to open a GPU device: {err}"))
}
