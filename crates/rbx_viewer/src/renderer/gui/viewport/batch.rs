//! The instance buffer one bake draws, and the textures it draws into.

use std::ops::Range;

use glam::Vec3;

use crate::renderer::instance::InstanceRaw;
use crate::scene::{GuiRect, Part, ShapeKind};

/// Ceiling on either side, like a `SurfaceGui` canvas': a frame stretched
/// over a huge screen would otherwise ask for a texture no adapter allocates.
pub(super) const MAX_SIDE: u32 = 2048;
/// One draw call's worth of the instance buffer: a stretch of one shape,
/// opaque or blended.
pub(super) struct Run {
    pub(super) kind: ShapeKind,
    pub(super) instances: Range<u32>,
    pub(super) blended: bool,
}

/// The instance buffer in draw order — opaque parts grouped by shape, then
/// the translucent ones furthest from `eye` first, the way the main pass
/// sorts its own blended geometry — and the runs that split it.
pub(super) fn batch(parts: &[&Part], eye: Vec3) -> (Vec<InstanceRaw>, Vec<Run>) {
    let mut ordered: Vec<&Part> = Vec::with_capacity(parts.len());
    let mut kinds: Vec<ShapeKind> = Vec::new();
    for part in parts.iter().filter(|part| !part.is_translucent()) {
        if !kinds.contains(&part.kind) {
            kinds.push(part.kind);
        }
    }
    for kind in kinds {
        ordered.extend(
            parts
                .iter()
                .filter(|part| !part.is_translucent() && part.kind == kind),
        );
    }
    let mut translucent: Vec<&Part> = parts
        .iter()
        .copied()
        .filter(|part| part.is_translucent())
        .collect();
    let far = |part: &Part| (part.transform.w_axis.truncate() - eye).length_squared();
    translucent.sort_by(|left, right| far(right).total_cmp(&far(left)));
    ordered.extend(translucent);

    let mut runs: Vec<Run> = Vec::new();
    for (index, part) in ordered.iter().enumerate() {
        let index = index as u32;
        let blended = part.is_translucent();
        match runs.last_mut() {
            Some(run) if run.kind == part.kind && run.blended == blended => {
                run.instances.end = index + 1;
            }
            _ => runs.push(Run {
                kind: part.kind,
                instances: index..index + 1,
                blended,
            }),
        }
    }
    let instances = ordered
        .iter()
        .map(|part| InstanceRaw::from_part(part))
        .collect();
    (instances, runs)
}

/// The texture size a box asks for, `None` where it has no whole pixel to
/// its name. Rounded up rather than to the nearest: the quad is stretched
/// over the box, and half a pixel of extra resolution is invisible while
/// half a pixel too little blurs an edge.
pub(super) fn pixels(rect: &GuiRect) -> Option<(u32, u32)> {
    let side = |extent: f32| (extent >= 1.0).then(|| (extent.ceil() as u32).min(MAX_SIDE));
    Some((side(rect.width)?, side(rect.height)?))
}

pub(super) fn target(
    device: &wgpu::Device,
    size: (u32, u32),
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rbxview viewport frame"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}
