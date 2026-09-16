//! The Properties-panel fast path for the effects a [`Scene`] keeps beside its
//! parts — `ParticleEmitter`s, `Beam`s and `Trail`s — which the renderer draws
//! from static definitions rather than from the instance buffers
//! `Scene::resync_part` writes into, so an edit to one is a re-plan of its
//! list, not a rewrite of one GPU slot.

use rbx_dom::WeakDom;
use rbx_reflection::ReflectionDatabase;

use super::{beam, particles, trail, Scene};

/// Which of the three effect lists an edited instance's class lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EffectKind {
    Particles,
    Beams,
    Trails,
}

impl EffectKind {
    /// `None` for anything that is not one of the three — the caller's cue
    /// that this fast path does not apply. An `Attachment` is deliberately
    /// not here even though moving one moves a beam's or trail's endpoint:
    /// an attachment can be the endpoint of any number of effects of either
    /// kind at once, and a full reload is the only path that re-reads them all.
    pub(crate) fn of(database: &ReflectionDatabase, class: &str) -> Option<Self> {
        [
            ("ParticleEmitter", Self::Particles),
            ("Beam", Self::Beams),
            ("Trail", Self::Trails),
        ]
        .into_iter()
        .find(|(ancestor, _)| database.is_subclass_of(class, ancestor))
        .map(|(_, kind)| kind)
    }
}

impl Scene {
    /// Re-reads every effect of `kind` from `dom`, exactly as
    /// [`Scene::from_dom`] did — for an edit on an emitter/beam/trail, or on
    /// the part or attachment one hangs off, which `Headless::apply_changes`
    /// turns into a re-plan of that kind's list.
    ///
    /// The whole list rather than the one edited instance: each `plan` is a
    /// filtered walk that drops a disabled emitter or an unplaceable beam
    /// outright, so flipping `Enabled` is a change of list membership, and
    /// `ParticleEmitter`'s whole-scene particle budget
    /// (`particles::TOTAL_CAP`) is spent in DOM order across all of them.
    /// The walk is CPU-only and touches no asset or GPU state, which is the
    /// entire saving over a full reload; matching the result back to the
    /// renderer's running state is `renderer::Renderer::patch_effect`'s job.
    pub(crate) fn replan_effect(
        &mut self,
        dom: &WeakDom,
        database: &ReflectionDatabase,
        kind: EffectKind,
    ) {
        match kind {
            EffectKind::Particles => {
                let placements = self.placements();
                self.emitters = particles::plan(dom, database, &placements);
            }
            EffectKind::Beams => self.beams = beam::plan(dom, database),
            EffectKind::Trails => self.trails = trail::plan(dom, database),
        }
    }
}

#[cfg(test)]
mod tests {
    use rbx_dom::{CFrameData, Instance, Ref, Variant, Vector3Data, WeakDom};
    use rbx_reflection::ReflectionDatabase;

    use super::EffectKind;
    use crate::scene::Scene;

    const IDENTITY_ROTATION: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    const BASE: u32 = 1;
    const EMITTER: u32 = 2;
    const ATTACHMENT0: u32 = 4;
    const ATTACHMENT1: u32 = 6;
    const BEAM: u32 = 7;
    const TRAIL: u32 = 8;

    fn cframe_at(x: f32, y: f32, z: f32) -> Variant {
        Variant::CFrame(CFrameData {
            position: Vector3Data { x, y, z },
            rotation: IDENTITY_ROTATION,
        })
    }

    fn insert(
        dom: &mut WeakDom,
        id: u32,
        class: &str,
        parent: Option<Ref>,
        properties: Vec<(&str, Variant)>,
    ) -> Ref {
        let referent = Ref::new(id);
        let mut instance = Instance::new(referent, class, class);
        for (name, value) in properties {
            instance.properties_mut().insert(name.to_string(), value);
        }
        dom.insert(instance);
        dom.set_parent(referent, parent);
        referent
    }

    fn part(dom: &mut WeakDom, id: u32, workspace: Ref, x: f32) -> Ref {
        insert(
            dom,
            id,
            "Part",
            Some(workspace),
            vec![
                (
                    "size",
                    Variant::Vector3(Vector3Data {
                        x: 4.0,
                        y: 1.0,
                        z: 4.0,
                    }),
                ),
                ("CFrame", cframe_at(x, 0.0, 0.0)),
            ],
        );
        Ref::new(id)
    }

    /// A part carrying an emitter, and two more parts whose attachments a beam
    /// and a trail both span.
    fn effects_place() -> WeakDom {
        let mut dom = WeakDom::new();
        let workspace = insert(&mut dom, 100, "Workspace", None, vec![]);
        let base = part(&mut dom, BASE, workspace, 0.0);
        insert(
            &mut dom,
            EMITTER,
            "ParticleEmitter",
            Some(base),
            vec![
                ("Enabled", Variant::Bool(true)),
                ("Rate", Variant::Float32(10.0)),
            ],
        );
        let a = part(&mut dom, 3, workspace, -8.0);
        insert(
            &mut dom,
            ATTACHMENT0,
            "Attachment",
            Some(a),
            vec![("CFrame", cframe_at(0.0, 0.0, 0.0))],
        );
        let b = part(&mut dom, 5, workspace, 8.0);
        insert(
            &mut dom,
            ATTACHMENT1,
            "Attachment",
            Some(b),
            vec![("CFrame", cframe_at(0.0, 0.0, 0.0))],
        );
        let endpoints = || {
            vec![
                ("Attachment0", Variant::Ref(Ref::new(ATTACHMENT0))),
                ("Attachment1", Variant::Ref(Ref::new(ATTACHMENT1))),
            ]
        };
        insert(&mut dom, BEAM, "Beam", Some(a), endpoints());
        insert(&mut dom, TRAIL, "Trail", Some(a), endpoints());
        dom
    }

    #[test]
    fn effect_kind_covers_exactly_the_three_effect_classes() {
        let database = ReflectionDatabase::embedded();
        assert_eq!(
            EffectKind::of(&database, "ParticleEmitter"),
            Some(EffectKind::Particles)
        );
        assert_eq!(EffectKind::of(&database, "Beam"), Some(EffectKind::Beams));
        assert_eq!(EffectKind::of(&database, "Trail"), Some(EffectKind::Trails));
        for other in ["Part", "Attachment", "Fire", "Folder"] {
            assert_eq!(EffectKind::of(&database, other), None, "{other}");
        }
    }

    // `Enabled` is a membership change for emitters (see `particles::plan`),
    // so a re-plan has to be able to both drop one and bring it back.
    #[test]
    fn replanning_particles_drops_a_disabled_emitter_and_brings_it_back() {
        let mut dom = effects_place();
        let database = ReflectionDatabase::embedded();
        let mut scene = Scene::from_dom(&dom, &database).unwrap();
        assert_eq!(scene.particle_emitters().len(), 1);
        assert_eq!(scene.particle_emitters()[0].referent, Ref::new(EMITTER));

        dom.set_property(Ref::new(EMITTER), "Enabled", Variant::Bool(false))
            .unwrap();
        scene.replan_effect(&dom, &database, EffectKind::Particles);
        assert!(scene.particle_emitters().is_empty());

        dom.set_property(Ref::new(EMITTER), "Enabled", Variant::Bool(true))
            .unwrap();
        scene.replan_effect(&dom, &database, EffectKind::Particles);
        assert_eq!(scene.particle_emitters().len(), 1);
    }

    #[test]
    fn replanning_one_kind_picks_up_its_edit_and_leaves_the_others_alone() {
        let mut dom = effects_place();
        let database = ReflectionDatabase::embedded();
        let mut scene = Scene::from_dom(&dom, &database).unwrap();
        assert_eq!(scene.beams()[0].width0, 1.0);

        dom.set_property(Ref::new(BEAM), "Width0", Variant::Float32(3.0))
            .unwrap();
        dom.set_property(Ref::new(EMITTER), "Rate", Variant::Float32(99.0))
            .unwrap();
        scene.replan_effect(&dom, &database, EffectKind::Beams);

        assert_eq!(scene.beams()[0].width0, 3.0);
        assert_eq!(
            scene.particle_emitters()[0].rate,
            10.0,
            "a beam re-plan must not re-read emitters"
        );
    }

    // A disabled trail is still listed (its history keeps drawing — see
    // `scene::trail::Trail::enabled`), so the re-plan carries the flag rather
    // than dropping the trail the way it drops a disabled emitter.
    #[test]
    fn replanning_trails_keeps_a_disabled_trail_but_flags_it() {
        let mut dom = effects_place();
        let database = ReflectionDatabase::embedded();
        let mut scene = Scene::from_dom(&dom, &database).unwrap();
        assert!(scene.trails()[0].enabled);

        dom.set_property(Ref::new(TRAIL), "Enabled", Variant::Bool(false))
            .unwrap();
        scene.replan_effect(&dom, &database, EffectKind::Trails);

        assert_eq!(scene.trails().len(), 1);
        assert!(!scene.trails()[0].enabled);
        assert_eq!(scene.trails()[0].referent, Ref::new(TRAIL));
    }
}
