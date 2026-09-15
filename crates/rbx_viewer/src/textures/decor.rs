//! Joins a [`super::Plan`] to the images that actually downloaded.

use std::collections::HashMap;

use rbx_assets::AssetRef;

use super::sky::stars;
use super::{Body, FaceInstance, Plan, Quad, Star};
use crate::assets::Image;

/// All the face instances sharing one image, split by whether they can go in the
/// opaque pass or have to be blended after it.
pub(crate) struct Group {
    pub(crate) image: Image,
    pub(crate) opaque: Vec<FaceInstance>,
    pub(crate) blended: Vec<FaceInstance>,
}

/// One skybox panel and the image pasted on it.
pub(crate) struct Panel {
    pub(crate) image: Image,
    pub(crate) quad: Quad,
}

/// The sun or the moon, with its disc image.
pub(crate) struct Celestial {
    pub(crate) image: Image,
    pub(crate) body: Body,
}

/// The painted half of a scene: textured part faces, plus a skybox if the DOM
/// has one and every one of its six panels resolved.
#[derive(Default)]
pub(crate) struct Decor {
    pub(crate) groups: Vec<Group>,
    pub(crate) sky: Option<Vec<Panel>>,
    pub(crate) bodies: Vec<Celestial>,
    pub(crate) stars: Vec<Star>,
}

impl Decor {
    /// Joins a plan to the images that actually came back.
    ///
    /// A face whose image is missing is simply left unpainted; a skybox is all
    /// or nothing, since five panels and a hole reads as a bug rather than as a
    /// sky.
    pub(crate) fn assemble(plan: Plan, images: &HashMap<AssetRef, Image>) -> Self {
        let mut groups: Vec<Group> = Vec::new();
        let mut index: HashMap<AssetRef, usize> = HashMap::new();

        for (reference, face) in plan.faces {
            let Some(image) = images.get(&reference) else {
                continue;
            };
            let slot = *index.entry(reference).or_insert_with(|| {
                groups.push(Group {
                    image: image.clone(),
                    opaque: Vec::new(),
                    blended: Vec::new(),
                });
                groups.len() - 1
            });
            let group = &mut groups[slot];
            if face.alpha >= 1.0 && !group.image.has_alpha() {
                group.opaque.push(face);
            } else {
                group.blended.push(face);
            }
        }

        let sky = plan.sky.and_then(|panels| {
            panels
                .into_iter()
                .map(|(reference, quad)| {
                    images.get(&reference).map(|image| Panel {
                        image: image.clone(),
                        quad,
                    })
                })
                .collect::<Option<Vec<_>>>()
        });

        // Unlike the skybox, the sun and the moon are independent: one of them
        // failing to download is no reason to drop the other.
        let bodies = plan
            .bodies
            .into_iter()
            .filter_map(|(reference, body)| {
                images.get(&reference).map(|image| Celestial {
                    image: image.clone(),
                    body,
                })
            })
            .collect();

        Decor {
            groups,
            sky,
            bodies,
            // Generated rather than downloaded: a star is a direction and a
            // brightness, and Roblox ships no image for them.
            stars: stars::field(plan.stars),
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::{Mat4, Vec3};

    use super::*;
    use crate::scene::ShapeKind;
    use crate::textures::face;

    fn image(alpha: u8) -> Image {
        Image {
            width: 1,
            height: 1,
            pixels: vec![255, 255, 255, alpha],
        }
    }

    fn plan_of(faces: Vec<(AssetRef, FaceInstance)>) -> Plan {
        Plan {
            faces,
            sky: None,
            bodies: Vec::new(),
            stars: 0,
        }
    }

    fn instance(alpha: f32) -> FaceInstance {
        FaceInstance {
            referent: rbx_dom::Ref::new(1),
            kind: ShapeKind::CylinderY,
            model: Mat4::IDENTITY,
            projection: face::projection(
                face::NormalId::Top,
                ShapeKind::CylinderY,
                Vec3::splat(2.0),
                face::Mapping::Stretched,
            ),
            tint: [1.0; 3],
            alpha,
        }
    }

    #[test]
    fn instances_sharing_an_image_end_up_in_one_group() {
        let plan = plan_of(vec![
            (AssetRef::Id(1), instance(1.0)),
            (AssetRef::Id(2), instance(1.0)),
            (AssetRef::Id(1), instance(1.0)),
        ]);
        let images = HashMap::from([(AssetRef::Id(1), image(255)), (AssetRef::Id(2), image(255))]);

        let decor = Decor::assemble(plan, &images);

        assert_eq!(decor.groups.len(), 2);
        assert_eq!(decor.groups[0].opaque.len(), 2);
        assert_eq!(decor.groups[1].opaque.len(), 1);
    }

    #[test]
    fn a_translucent_instance_or_image_moves_the_whole_draw_to_the_blended_pass() {
        let plan = plan_of(vec![
            (AssetRef::Id(1), instance(0.5)),
            (AssetRef::Id(1), instance(1.0)),
            (AssetRef::Id(2), instance(1.0)),
        ]);
        let images = HashMap::from([(AssetRef::Id(1), image(255)), (AssetRef::Id(2), image(128))]);

        let decor = Decor::assemble(plan, &images);

        assert_eq!(decor.groups[0].blended.len(), 1);
        assert_eq!(decor.groups[0].opaque.len(), 1);
        // The image itself has transparent pixels, so even a fully opaque
        // instance of it has to be blended.
        assert_eq!(decor.groups[1].blended.len(), 1);
        assert!(decor.groups[1].opaque.is_empty());
    }

    #[test]
    fn an_instance_whose_image_never_downloaded_is_left_unpainted() {
        let plan = plan_of(vec![(AssetRef::Id(1), instance(1.0))]);

        let decor = Decor::assemble(plan, &HashMap::new());

        assert!(decor.groups.is_empty());
    }
}
