//! Joins a [`super::Plan`] to the images that actually downloaded.

use std::collections::HashMap;
use std::sync::Arc;

use rbx_assets::AssetRef;
use rbx_dom::Ref;

use super::sky::stars;
use super::{Body, FaceInstance, Plan, Quad, Star};
use crate::assets::Image;
use crate::scene::Placement;

/// All the face instances sharing one image, split by whether they can go in the
/// opaque pass or have to be blended after it.
///
/// The image is behind an `Arc` here and in [`Panel`]/[`Celestial`]: it is
/// the decoded asset the loader keeps across reloads (see `load::Resident`),
/// shared rather than copied into every scene built from it.
pub(crate) struct Group {
    /// The asset `image` was decoded from. Not drawn from — the renderer keeps
    /// its uploads keyed by this, so a scene rebuild (see
    /// `renderer::textured::Textured::rebuild`) can tell an image it already
    /// holds on the GPU from one it has to upload.
    pub(crate) reference: AssetRef,
    pub(crate) image: Arc<Image>,
    pub(crate) opaque: Vec<FaceInstance>,
    pub(crate) blended: Vec<FaceInstance>,
}

/// One skybox panel and the image pasted on it.
pub(crate) struct Panel {
    /// The asset `image` came from — the same role as [`Group::reference`]:
    /// six unchanged references mean an unchanged environment probe and
    /// skybox, which a rebuild then keeps rather than prefilters again.
    pub(crate) reference: AssetRef,
    pub(crate) image: Arc<Image>,
    pub(crate) quad: Quad,
}

/// The sun or the moon, with its disc image.
pub(crate) struct Celestial {
    /// The asset `image` came from — see [`Group::reference`].
    pub(crate) reference: AssetRef,
    pub(crate) image: Arc<Image>,
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
    ///
    /// `drawn` is the scene's placements as they stand — a face whose part is
    /// not among them is dropped. A plan is made while every part still draws
    /// its box (there is no other moment a streaming load could make one), and
    /// this is where a part whose real mesh has since arrived stops carrying
    /// the decals that were projected onto that box: a decal floating in the
    /// air where the box used to be is worse than none, and dropping it here
    /// leaves exactly the faces a build with every asset already resident
    /// would have planned.
    pub(crate) fn assemble(
        plan: &Plan,
        images: &HashMap<AssetRef, Arc<Image>>,
        drawn: &HashMap<Ref, Placement>,
    ) -> Self {
        let mut groups: Vec<Group> = Vec::new();
        let mut index: HashMap<AssetRef, usize> = HashMap::new();

        for planned in &plan.faces {
            if !drawn.contains_key(&planned.part) {
                continue;
            }
            let Some(image) = images.get(&planned.reference) else {
                continue;
            };
            let slot = *index.entry(planned.reference.clone()).or_insert_with(|| {
                groups.push(Group {
                    reference: planned.reference.clone(),
                    image: image.clone(),
                    opaque: Vec::new(),
                    blended: Vec::new(),
                });
                groups.len() - 1
            });
            let group = &mut groups[slot];
            if planned.face.alpha >= 1.0 && !group.image.has_alpha() {
                group.opaque.push(planned.face);
            } else {
                group.blended.push(planned.face);
            }
        }

        let sky = plan.sky.as_ref().and_then(|panels| {
            panels
                .iter()
                .map(|(reference, quad)| {
                    images.get(reference).map(|image| Panel {
                        image: image.clone(),
                        reference: reference.clone(),
                        quad: quad.clone(),
                    })
                })
                .collect::<Option<Vec<_>>>()
        });

        // Unlike the skybox, the sun and the moon are independent: one of them
        // failing to download is no reason to drop the other.
        let bodies = plan
            .bodies
            .iter()
            .filter_map(|(reference, body)| {
                images.get(reference).map(|image| Celestial {
                    image: image.clone(),
                    reference: reference.clone(),
                    body: *body,
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

    fn image(alpha: u8) -> Arc<Image> {
        Arc::new(Image {
            width: 1,
            height: 1,
            pixels: vec![255, 255, 255, alpha],
        })
    }

    /// Every planned face on the one part [`drawn`] places, so the tests
    /// below exercise grouping rather than the drawn-part filter.
    fn part() -> Ref {
        Ref::new(1)
    }

    fn plan_of(faces: Vec<(AssetRef, FaceInstance)>) -> Plan {
        Plan {
            faces: faces
                .into_iter()
                .map(|(reference, face)| crate::textures::Planned {
                    part: part(),
                    reference,
                    face,
                })
                .collect(),
            sky: None,
            bodies: Vec::new(),
            stars: 0,
        }
    }

    fn drawn() -> HashMap<Ref, Placement> {
        HashMap::from([(
            part(),
            Placement {
                kind: ShapeKind::CylinderY,
                model: Mat4::IDENTITY,
                size: Vec3::splat(2.0),
            },
        )])
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

        let decor = Decor::assemble(&plan, &images, &drawn());

        assert_eq!(decor.groups.len(), 2);
        assert_eq!(decor.groups[0].opaque.len(), 2);
        assert_eq!(decor.groups[1].opaque.len(), 1);
    }

    // A part whose real mesh has landed since the plan was made no longer
    // draws the box these faces were projected onto, so they are dropped
    // rather than left hanging in the air where it was.
    #[test]
    fn a_face_on_a_part_that_stopped_drawing_is_dropped() {
        let plan = plan_of(vec![(AssetRef::Id(1), instance(1.0))]);
        let images = HashMap::from([(AssetRef::Id(1), image(255))]);

        let decor = Decor::assemble(&plan, &images, &HashMap::new());

        assert!(decor.groups.is_empty());
    }

    #[test]
    fn a_translucent_instance_or_image_moves_the_whole_draw_to_the_blended_pass() {
        let plan = plan_of(vec![
            (AssetRef::Id(1), instance(0.5)),
            (AssetRef::Id(1), instance(1.0)),
            (AssetRef::Id(2), instance(1.0)),
        ]);
        let images = HashMap::from([(AssetRef::Id(1), image(255)), (AssetRef::Id(2), image(128))]);

        let decor = Decor::assemble(&plan, &images, &drawn());

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

        let decor = Decor::assemble(&plan, &HashMap::new(), &drawn());

        assert!(decor.groups.is_empty());
    }
}
