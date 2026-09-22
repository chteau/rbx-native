//! Which instances each highlight's mask draws: the batches a highlight
//! list builds, and which runs of them each highlight owns.

use std::collections::HashMap;
use std::ops::Range;

use bytemuck::Pod;
use rbx_assets::AssetRef;
use rbx_dom::Ref;

use super::super::cull::visible_runs;
use super::super::shadow::casters;
use super::super::slots::keyed::{Group, Keyed};
use super::super::slots::Roster;
use super::pipelines::MaskInstance;
use super::{Claim, MeshBatches, ShapeBatches};
use crate::scene::{DepthMode, Part, Resolved};

pub(super) fn shape_batches(
    device: &wgpu::Device,
    claims: &HashMap<Ref, Claim>,
    parts: &[Part],
) -> ShapeBatches {
    let mut batches = Keyed::new("rbxview highlight shapes");
    if claims.is_empty() {
        return batches;
    }
    for kind in super::super::shaped::kinds(parts) {
        let roster = Roster::from_iter(
            parts
                .iter()
                .filter(|part| part.kind == kind && part.is_drawn())
                .filter_map(|part| {
                    let &claim = claims.get(&part.referent())?;
                    Some((
                        part.id,
                        MaskInstance::new(part.transform.to_cols_array_2d(), claim.0),
                        claim,
                    ))
                }),
        );
        if roster.len() > 0 {
            batches.add_group(device, kind, (), roster);
        }
    }
    batches
}

pub(super) fn mesh_batches(
    device: &wgpu::Device,
    claims: &HashMap<Ref, Claim>,
    resolved: &Resolved,
) -> MeshBatches {
    let mut batches = Keyed::new("rbxview highlight meshes");
    let mut order: Vec<AssetRef> = Vec::new();
    for instance in &resolved.instances {
        if claims.contains_key(&instance.referent) && !order.contains(&instance.mesh) {
            order.push(instance.mesh.clone());
        }
    }
    for reference in order {
        let Some(geometry) = casters::geometry(device, resolved, &reference) else {
            continue;
        };
        let roster = Roster::from_iter(
            resolved
                .instances
                .iter()
                .filter(|instance| instance.mesh == reference)
                .filter_map(|instance| {
                    let &claim = claims.get(&instance.referent)?;
                    Some((
                        instance.referent,
                        MaskInstance::new(instance.model.to_cols_array_2d(), claim.0),
                        claim,
                    ))
                }),
        );
        batches.add_group(device, reference, geometry, roster);
    }
    batches
}

/// Each batch's runs of `claim`'s instances, by the batch's position,
/// leaving out the batches it has none in.
pub(super) fn claim_runs<K, G, T: Pod, Id: Copy>(
    groups: &[Group<K, G, T, Claim, Id>],
    claim: Claim,
) -> Vec<(usize, Vec<Range<u32>>)> {
    groups
        .iter()
        .enumerate()
        .map(|(position, batch)| {
            let runs = visible_runs(batch.slots.count(), |index| {
                batch.slots.side(index) == claim
            });
            (position, runs)
        })
        .filter(|(_, runs)| !runs.is_empty())
        .collect()
}

/// The order the mask draws the claims in: the depth modes as they always
/// went, `AlwaysOnTop` first, and within each the highlights from the last to
/// the first. A pixel two highlights both reach is the last one drawn's, so
/// the first highlight listed wins it — which is what lets the editor's
/// selection cue, listed before its hover cue, keep its outline in front of
/// a hovered part behind it.
pub(super) fn draw_order(claims: impl Iterator<Item = Claim>) -> Vec<Claim> {
    let mut order: Vec<Claim> = Vec::new();
    for claim in claims {
        if !order.contains(&claim) {
            order.push(claim);
        }
    }
    let rank = |mode: DepthMode| usize::from(mode == DepthMode::Occluded);
    order.sort_by_key(|&(index, mode)| (rank(mode), std::cmp::Reverse(index)));
    order
}
