//! The Properties-panel fast path for one resolved mesh instance — the
//! file-mesh counterpart of `renderer::shaped::Shaped::sync` and
//! `renderer::translucent::Translucent::sync`.

use rbx_dom::Ref;

use super::{blends, build, center, raw, Blended, FileMeshes, GroupKey, Images};
use super::{Binding, Geometry};
use crate::scene::{Resolved, ResolvedInstance};

impl FileMeshes {
    /// Brings both passes in line with one edited instance: rewritten where
    /// it is if its (mesh, skin) batch and opaque/blended side are unchanged,
    /// otherwise taken out of whichever batch held it and put into the one
    /// it belongs in now — built on the spot if no instance drew through
    /// that batch before, against `resolved`'s already-downloaded assets.
    ///
    /// `false` when that new batch would need a mesh or texture `resolved`
    /// never downloaded, which `Scene::patch_mesh_instance` already refuses;
    /// the caller falls back to a full reload.
    pub(in crate::renderer) fn sync(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        resolved: &Resolved,
        instance: &ResolvedInstance,
    ) -> bool {
        let key = GroupKey::of(instance);
        if blends(resolved, &key) || instance.alpha < 1.0 {
            self.opaque.remove(queue, instance.referent);
            self.place_blended(device, queue, resolved, key, instance)
        } else {
            self.remove_blended(instance.referent);
            // Built ahead of `sync` rather than in its closure: the closure
            // would need `self.images` while `self.opaque` is borrowed.
            let geometry = if self.opaque.groups().iter().any(|group| group.key == key) {
                None
            } else {
                match self.geometry_for(device, queue, resolved, &key) {
                    Some(geometry) => Some(geometry),
                    None => return false,
                }
            };
            self.opaque.sync(
                device,
                queue,
                instance.referent,
                Some((key, raw(instance), ())),
                |_| geometry,
            )
        }
    }

    /// Drops an instance the scene no longer draws at all (it turned fully
    /// transparent); a no-op for one neither pass held.
    pub(in crate::renderer) fn remove(&mut self, queue: &wgpu::Queue, referent: Ref) {
        self.opaque.remove(queue, referent);
        self.remove_blended(referent);
    }

    fn remove_blended(&mut self, referent: Ref) {
        let Some(batch) = self.blended_index.remove(&referent) else {
            return;
        };
        let items = &mut self.blended[batch].items;
        if let Some(position) = items.iter().position(|(held, ..)| *held == referent) {
            // Any order will do: `prepare` re-sorts before the next draw.
            items.swap_remove(position);
        }
    }

    fn place_blended(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        resolved: &Resolved,
        key: GroupKey,
        instance: &ResolvedInstance,
    ) -> bool {
        let item = (instance.referent, center(instance), raw(instance));
        if let Some(&batch) = self.blended_index.get(&instance.referent) {
            let blended = &mut self.blended[batch];
            if blended.key == key {
                if let Some(held) = blended
                    .items
                    .iter_mut()
                    .find(|(held, ..)| *held == instance.referent)
                {
                    *held = item;
                    return true;
                }
            }
            self.remove_blended(instance.referent);
        }

        let batch = match self.blended.iter().position(|blended| blended.key == key) {
            Some(batch) => batch,
            None => {
                let Some(geometry) = self.geometry_for(device, queue, resolved, &key) else {
                    return false;
                };
                self.blended
                    .push(Blended::new(device, key, geometry, Vec::new()));
                self.blended.len() - 1
            }
        };
        let blended = &mut self.blended[batch];
        blended.items.push(item);
        if blended.items.len() > blended.capacity {
            blended.capacity = (blended.capacity * 2).max(blended.items.len());
            blended.instances = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rbxview filemesh instances"),
                size: (blended.capacity * std::mem::size_of::<super::InstanceRaw>())
                    as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        self.blended_index.insert(instance.referent, batch);
        true
    }

    /// A new batch's geometry for `key`, binding its texture if no batch has
    /// yet — `None` when the mesh or the image never downloaded.
    fn geometry_for(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        resolved: &Resolved,
        key: &GroupKey,
    ) -> Option<Geometry> {
        let mesh = resolved.meshes.get(&key.mesh)?;
        let binding = Binding {
            layout: &self.image_layout,
            sampler: &self.sampler,
            max_size: self.texture_max_size,
        };
        let skin = Images::slot(&mut self.images, device, queue, binding, resolved, key)?;
        Some(build(device, mesh, skin))
    }
}
