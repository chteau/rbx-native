//! Getting a rendered texture back into system memory one frame ahead of the
//! one being drawn.
//!
//! A texture-to-buffer copy is only *queued* by the CPU; the bytes exist once the
//! GPU has run it, and waiting for that right away idles the CPU for a whole
//! frame. So two buffers alternate: the copy of frame N is queued and its
//! mapping asked for, then frame N-1 — whose copy the GPU finished long ago — is
//! trimmed and handed back. One frame of latency buys the entire overlap.

use std::sync::mpsc::{self, Receiver};

pub(super) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const BYTES_PER_PIXEL: u32 = 4;
const SLOTS: usize = 2;

/// The texture frames are drawn into, plus the buffers they are read back
/// through. Kept between frames and rebuilt only on a size change, which is what
/// makes a continuously redrawn embedded view affordable.
pub(super) struct Target {
    texture: wgpu::Texture,
    readback: [wgpu::Buffer; SLOTS],
    /// Which buffer the next copy goes to. The other one is where the frame
    /// being handed back this very moment lives.
    next: usize,
    size: (u32, u32),
}

/// A copy the GPU has been asked for, not yet mapped.
///
/// Must reach [`Target::collect`]: a slot left mapped is one the next copy into
/// it cannot use.
pub(super) struct Pending {
    slot: usize,
    /// Which submission has to have run for the bytes to be there. Waiting on
    /// this one rather than on the queue keeps the newer frame out of the wait.
    submission: wgpu::SubmissionIndex,
    mapped: Receiver<Result<(), wgpu::BufferAsyncError>>,
}

impl Target {
    pub(super) fn new(device: &wgpu::Device, size: (u32, u32)) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rbxview offscreen"),
            size: extent(size),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            // The GUI overlay composites through the non-sRGB twin of this
            // format (see `renderer::gui::pipeline::encoded`).
            view_formats: &[FORMAT.remove_srgb_suffix()],
        });
        let bytes = u64::from(padded_row(size.0 * BYTES_PER_PIXEL)) * u64::from(size.1);
        let readback = std::array::from_fn(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rbxview readback"),
                size: bytes,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            })
        });

        Target {
            texture,
            readback,
            next: 0,
            size,
        }
    }

    pub(super) fn size(&self) -> (u32, u32) {
        self.size
    }

    /// What the renderer draws into. The texture rather than a view: the GUI
    /// overlay needs to make one of its own in the non-sRGB twin format.
    pub(super) fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    /// Queues the copy out of the drawn texture and asks for its mapping.
    pub(super) fn copy(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> Pending {
        let slot = self.next;
        self.next = (self.next + 1) % SLOTS;

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("rbxview readback"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback[slot],
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row(self.size.0 * BYTES_PER_PIXEL)),
                    rows_per_image: Some(self.size.1),
                },
            },
            extent(self.size),
        );
        let submission = queue.submit(std::iter::once(encoder.finish()));

        let (sender, mapped) = mpsc::channel();
        self.readback[slot]
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                // The receiver is dropped only if the wait below already failed.
                let _ = sender.send(result);
            });

        Pending {
            slot,
            submission,
            mapped,
        }
    }

    /// Waits for `pending` and returns its tightly packed RGBA8 (sRGB) rows.
    pub(super) fn collect(
        &self,
        device: &wgpu::Device,
        pending: Pending,
    ) -> Result<Vec<u8>, String> {
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(pending.submission),
                timeout: None,
            })
            .map_err(|err| format!("the GPU never finished the frame: {err}"))?;
        pending
            .mapped
            .recv()
            .map_err(|_| "the GPU dropped the readback callback".to_string())?
            .map_err(|err| format!("failed to map the rendered frame: {err}"))?;

        let buffer = &self.readback[pending.slot];
        let mapped = buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|err| format!("failed to read back the frame: {err}"))?;

        let row = (self.size.0 * BYTES_PER_PIXEL) as usize;
        let mut pixels = Vec::with_capacity(row * self.size.1 as usize);
        for padded in mapped.chunks(padded_row(self.size.0 * BYTES_PER_PIXEL) as usize) {
            pixels.extend_from_slice(&padded[..row]);
        }

        // The mapped range must go out of scope before the buffer may be unmapped.
        drop(mapped);
        buffer.unmap();

        Ok(pixels)
    }
}

// Texture-to-buffer copies need every row aligned, so the readback rows are wider
// than the image and have to be trimmed afterwards.
fn padded_row(row: u32) -> u32 {
    let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    row.div_ceil(alignment) * alignment
}

fn extent(size: (u32, u32)) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: size.0,
        height: size.1,
        depth_or_array_layers: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::padded_row;

    #[test]
    fn rows_are_padded_up_to_the_copy_alignment() {
        assert_eq!(padded_row(256), 256);
        assert_eq!(padded_row(1280 * 4), 1280 * 4);
        // 300 pixels wide: 1200 bytes rounds up to 1280.
        assert_eq!(padded_row(1200), 1280);
    }
}
