//! The Properties-panel fast path for a `ParticleEmitter` edit: swapping the
//! static definitions under the running simulations, keeping every texture
//! [`Particles::new`] uploaded and every particle already in the air.

use std::collections::HashMap;

use rbx_assets::AssetRef;
use rbx_dom::Ref;

use super::{Live, Particles};
use crate::scene::{Emitter, Simulation};

impl Particles {
    /// Replaces the emitter set with `emitters` — the scene's freshly
    /// re-planned list (see `Scene::replan_effect`) — keeping the running
    /// simulation of every emitter still present, so a `Rate` or `Color`
    /// edit changes what the next frame spawns rather than restarting the
    /// effect, and pre-warming only an emitter that just appeared (`Enabled`
    /// flipped on) exactly as [`Particles::new`] would have.
    ///
    /// `false` when an emitter names a texture this pass never tried to
    /// download (a `Texture` edit to a new image): only a full reload fetches
    /// it. A texture that was tried and failed keeps dropping its emitter,
    /// same as at load, rather than forcing a reload that would only fail the
    /// same way again.
    pub(in crate::renderer) fn replace(&mut self, emitters: &[Emitter]) -> bool {
        if !self.enabled {
            return true;
        }
        if emitters
            .iter()
            .any(|emitter| !self.slots.contains_key(&emitter.texture))
        {
            return false;
        }
        let previous = std::mem::take(&mut self.live);
        self.live = carry_over(previous, emitters, |texture| {
            self.slots.get(texture).copied().flatten()
        });
        true
    }
}

/// [`Particles::replace`]'s pure half: every emitter in `emitters` whose
/// texture `slot` resolves, paired with its own simulation out of `previous`
/// where its referent was already live, and with a fresh pre-warmed one
/// otherwise.
fn carry_over(
    previous: Vec<Live>,
    emitters: &[Emitter],
    slot: impl Fn(&AssetRef) -> Option<usize>,
) -> Vec<Live> {
    let mut simulations: HashMap<Ref, Simulation> = previous
        .into_iter()
        .map(|live| (live.emitter.referent, live.simulation))
        .collect();
    emitters
        .iter()
        .filter_map(|emitter| {
            let texture = slot(&emitter.texture)?;
            let simulation = simulations.remove(&emitter.referent).unwrap_or_else(|| {
                let mut simulation = Simulation::new(emitter.seed);
                simulation.prewarm(emitter);
                simulation
            });
            Some(Live {
                emitter: emitter.clone(),
                simulation,
                texture,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use rbx_assets::AssetRef;
    use rbx_dom::Ref;

    use super::super::tests::emitter;
    use super::*;

    fn busy_emitter(referent: u32) -> Emitter {
        Emitter {
            rate: 50.0,
            cap: 100,
            referent: Ref::new(referent),
            ..emitter(AssetRef::Id(1))
        }
    }

    // A `Rate` edit must not restart the effect: the particles already in the
    // air belong to the same emitter and keep flying under the new definition.
    #[test]
    fn a_surviving_emitter_keeps_its_particles_in_the_air() {
        let emitter = busy_emitter(7);
        let mut simulation = Simulation::new(emitter.seed);
        simulation.step(&emitter, 1.0);
        let flying = simulation.len();
        assert!(flying > 0);
        let previous = vec![Live {
            emitter: emitter.clone(),
            simulation,
            texture: 3,
        }];

        let edited = Emitter {
            rate: 1.0,
            ..emitter
        };
        let live = carry_over(previous, &[edited], |_| Some(3));

        assert_eq!(live.len(), 1);
        assert_eq!(live[0].simulation.len(), flying);
        assert_eq!(live[0].emitter.rate, 1.0);
        assert_eq!(live[0].texture, 3);
    }

    #[test]
    fn a_new_emitter_starts_pre_warmed_like_at_load() {
        let live = carry_over(Vec::new(), &[busy_emitter(1)], |_| Some(0));

        assert_eq!(live.len(), 1);
        assert!(live[0].simulation.len() > 0);
    }

    #[test]
    fn an_emitter_whose_texture_failed_is_dropped_like_at_load() {
        let live = carry_over(Vec::new(), &[busy_emitter(1)], |_| None);

        assert!(live.is_empty());
    }

    // The referent, not the list position, is what identifies an emitter: a
    // disabled neighbour vanishing from the list must not hand its
    // simulation to the emitter after it.
    #[test]
    fn simulations_follow_referents_not_positions() {
        let first = busy_emitter(1);
        let second = busy_emitter(2);
        let mut kept = Simulation::new(second.seed);
        kept.step(&second, 1.0);
        let flying = kept.len();
        let previous = vec![
            Live {
                emitter: first,
                simulation: Simulation::new(1),
                texture: 0,
            },
            Live {
                emitter: second.clone(),
                simulation: kept,
                texture: 0,
            },
        ];

        let live = carry_over(previous, &[second], |_| Some(0));

        assert_eq!(live.len(), 1);
        assert_eq!(live[0].emitter.referent, Ref::new(2));
        assert_eq!(live[0].simulation.len(), flying);
    }
}
