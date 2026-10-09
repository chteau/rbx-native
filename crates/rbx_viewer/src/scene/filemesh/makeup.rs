//! Face makeup: a `Decal` carrying a `WrapTextureTransfer`, which is not a
//! projection onto the part but an image in the head's own UV layout. Roblox
//! names the region of the head cage's UVs it covers with `UVMinBound` and
//! `UVMaxBound` (the cage sits in the UV tile its integer part names), so the
//! image is laid into that rectangle of the head's colour map and the two are
//! drawn as one texture.

use std::collections::HashMap;
use std::sync::Arc;

use rbx_assets::AssetRef;
use rbx_dom::{Instance, Variant, WeakDom};

use super::Entry;
use crate::assets::Image;
use crate::scene;
use crate::textures::is_makeup;

const TRANSFER: &str = "WrapTextureTransfer";
/// The colour map's size when the head has none of its own to take it from.
const FALLBACK_SIZE: u32 = 1024;

/// One makeup image and the rectangle of the head's UVs it covers, as
/// `[u0, v0, u1, v1]` in the tile the cage's bounds name, `v` counted from
/// the top as the image is.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Makeup {
    image: AssetRef,
    region: [f32; 4],
    alpha: f32,
}

/// The makeup among a part's children, lowest `ZIndex` first.
pub(super) fn of(dom: &WeakDom, part: &Instance) -> Vec<Makeup> {
    let mut found: Vec<(i32, Makeup)> = part
        .children()
        .iter()
        .filter_map(|&child| layer(dom, dom.get(child)?))
        .collect();
    found.sort_by_key(|(z, _)| *z);
    found.into_iter().map(|(_, makeup)| makeup).collect()
}

fn layer(dom: &WeakDom, decal: &Instance) -> Option<(i32, Makeup)> {
    if !is_makeup(dom, decal) {
        return None;
    }
    let properties = decal.properties();
    let image = super::parsed_asset_ref(properties.get("Texture")?)?;
    let transfer = decal
        .children()
        .iter()
        .filter_map(|&child| dom.get(child))
        .find(|child| child.class() == TRANSFER)?;
    let bound = |key, default: [f32; 2]| match transfer.properties().get(key) {
        Some(&Variant::Vector2(v)) => [v.x, v.y],
        _ => default,
    };
    let (min, max) = (bound("UVMinBound", [0.0; 2]), bound("UVMaxBound", [1.0; 2]));
    let tile = min[0].floor();
    let alpha = match properties.get("Transparency") {
        Some(&Variant::Float32(t)) => 1.0 - t.clamp(0.0, 1.0),
        _ => 1.0,
    };
    let z = match properties.get("ZIndex") {
        Some(&Variant::Int32(z)) => z,
        _ => 1,
    };
    Some((
        z,
        Makeup {
            image,
            // The cage's `v` runs up from the bottom of the map.
            region: [min[0] - tile, 1.0 - max[1], max[0] - tile, 1.0 - min[1]],
            alpha,
        },
    ))
}

pub(super) fn images(makeup: &[Makeup]) -> impl Iterator<Item = &AssetRef> {
    makeup.iter().map(|layer| &layer.image)
}

/// The key of the colour map `entry` draws once its makeup is on, `None`
/// when it has none that has arrived.
pub(super) fn key(entry: &Entry, images: &HashMap<AssetRef, Arc<Image>>) -> Option<AssetRef> {
    let arrived: Vec<&Makeup> = entry
        .makeup
        .iter()
        .filter(|layer| images.contains_key(&layer.image))
        .collect();
    if arrived.is_empty() {
        return None;
    }
    let under = match (&entry.texture, &entry.appearance) {
        (Some(texture), _) => format!("{texture:?}"),
        (None, Some(appearance)) => format!("{:?}", appearance.maps[0]),
        (None, None) => format!("{:?}", entry.color),
    };
    let worn: Vec<String> = arrived
        .iter()
        .map(|layer| format!("{:?}@{:?}x{}", layer.image, layer.region, layer.alpha))
        .collect();
    Some(AssetRef::Thumb(format!(
        "makeup/{under}/{}",
        worn.join("/")
    )))
}

/// `entry`'s colour map with the makeup laid over it, source-over, lowest
/// layer first. A head without a colour map is painted in its own colour.
pub(super) fn composite(entry: &Entry, images: &HashMap<AssetRef, Arc<Image>>) -> Image {
    let base = entry
        .texture
        .as_ref()
        .or_else(|| entry.appearance.as_ref()?.maps[0].as_ref())
        .and_then(|reference| images.get(reference));
    let (width, height) = base.map_or((FALLBACK_SIZE, FALLBACK_SIZE), |b| (b.width, b.height));
    let pixels = match base {
        Some(base) => base.pixels.clone(),
        None => {
            let [r, g, b] = entry.color.map(|channel| {
                (scene::linear_to_srgb(channel) * 255.0)
                    .round()
                    .clamp(0.0, 255.0) as u8
            });
            [r, g, b, u8::MAX].repeat((width * height) as usize)
        }
    };
    let mut image = Image {
        width,
        height,
        pixels,
    };
    for layer in &entry.makeup {
        if let Some(source) = images.get(&layer.image) {
            lay(&mut image, source, layer);
        }
    }
    image
}

fn lay(onto: &mut Image, source: &Image, layer: &Makeup) {
    let [u0, v0, u1, v1] = layer.region;
    let (w, h) = (onto.width as f32, onto.height as f32);
    let x0 = (u0 * w).floor().max(0.0) as u32;
    let x1 = ((u1 * w).ceil() as u32).min(onto.width);
    let y0 = (v0 * h).floor().max(0.0) as u32;
    let y1 = ((v1 * h).ceil() as u32).min(onto.height);
    for y in y0..y1 {
        for x in x0..x1 {
            let u = ((x as f32 + 0.5) / w - u0) / (u1 - u0);
            let v = ((y as f32 + 0.5) / h - v0) / (v1 - v0);
            if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                continue;
            }
            let sx = ((u * source.width as f32) as u32).min(source.width - 1);
            let sy = ((v * source.height as f32) as u32).min(source.height - 1);
            let from = &source.pixels[((sy * source.width + sx) * 4) as usize..][..4];
            let alpha = f32::from(from[3]) / 255.0 * layer.alpha;
            let out = &mut onto.pixels[((y * onto.width + x) * 4) as usize..][..4];
            for channel in 0..3 {
                out[channel] = (f32::from(from[channel]) * alpha
                    + f32::from(out[channel]) * (1.0 - alpha))
                    .round() as u8;
            }
            out[3] = out[3].max((alpha * 255.0).round() as u8);
        }
    }
}

#[cfg(test)]
mod tests {
    use rbx_dom::{Content, Vector2Data};

    use super::*;

    fn decal(min: [f32; 2], max: [f32; 2]) -> (WeakDom, rbx_dom::Ref) {
        let mut dom = WeakDom::new();
        let head = dom.new_instance("MeshPart", "Head", None);
        let decal = dom.new_instance("Decal", "Lip", Some(head));
        dom.get_mut(decal).unwrap().properties_mut().insert(
            "Texture".into(),
            Variant::Content(Content::Uri("rbxassetid://5".into())),
        );
        let transfer = dom.new_instance(TRANSFER, TRANSFER, Some(decal));
        let props = &mut dom.get_mut(transfer).unwrap().properties_mut();
        props.insert(
            "UVMinBound".into(),
            Variant::Vector2(Vector2Data {
                x: min[0],
                y: min[1],
            }),
        );
        props.insert(
            "UVMaxBound".into(),
            Variant::Vector2(Vector2Data {
                x: max[0],
                y: max[1],
            }),
        );
        (dom, head)
    }

    #[test]
    fn the_cage_bounds_become_a_top_down_rectangle_in_their_own_tile() {
        let (dom, head) = decal([3.25, 0.5], [3.75, 0.75]);
        let found = of(&dom, dom.get(head).unwrap());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].region, [0.25, 0.25, 0.75, 0.5]);
    }

    #[test]
    fn a_plain_decal_is_not_makeup() {
        let (mut dom, head) = decal([0.; 2], [1.; 2]);
        let transfer = dom
            .get(dom.get(head).unwrap().children()[0])
            .unwrap()
            .children()[0];
        dom.remove(transfer);
        assert!(of(&dom, dom.get(head).unwrap()).is_empty());
    }

    #[test]
    fn lay_paints_only_inside_the_region() {
        let mut head = Image {
            width: 4,
            height: 4,
            pixels: [0, 0, 0, 255].repeat(16),
        };
        let red = Image {
            width: 1,
            height: 1,
            pixels: vec![255, 0, 0, 255],
        };
        let layer = Makeup {
            image: parsed(),
            region: [0.5, 0.0, 1.0, 0.5],
            alpha: 1.0,
        };
        lay(&mut head, &red, &layer);
        let at = |x: usize, y: usize| head.pixels[(y * 4 + x) * 4];
        assert_eq!((at(3, 0), at(2, 1)), (255, 255));
        assert_eq!((at(0, 0), at(3, 3), at(2, 2)), (0, 0, 0));
    }

    fn parsed() -> AssetRef {
        let (dom, head) = decal([0.; 2], [1.; 2]);
        of(&dom, dom.get(head).unwrap()).remove(0).image
    }
}
