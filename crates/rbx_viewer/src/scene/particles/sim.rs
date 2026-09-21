//! Advances one emitter's particles by a time step, and prepares them for
//! rendering. Pure CPU state — no GPU handle anywhere in this file — so the
//! renderer only ever reads [`Simulation::particles`] once a frame.

use glam::{Quat, Vec3};

use super::emitter::Emitter;
use super::rng::Rng;
use super::sequence::{eval_color, eval_number};

/// A fixed step used only to pre-warm an emitter to its steady state before the
/// first real frame (see [`Simulation::prewarm`]). Small enough that spawning
/// and drag both stay stable, and a clean 1/60 s keeps it exactly `dt`-sized
/// for a 60 Hz window.
const PREWARM_STEP: f32 = 1.0 / 60.0;

struct Particle {
    position: Vec3,
    velocity: Vec3,
    age: f32,
    lifetime: f32,
    rotation: f32,
    rot_speed: f32,
}

/// One alive particle, resolved to whatever the renderer needs to draw it —
/// everything except the emitter-wide constants (texture, light emission,
/// z-offset), which the renderer already has one copy of per emitter.
pub(crate) struct RenderParticle {
    pub(crate) position: Vec3,
    pub(crate) size: f32,
    pub(crate) rotation: f32,
    pub(crate) color: [f32; 3],
    pub(crate) alpha: f32,
}

/// One emitter's live particles and its own spawn clock.
pub(crate) struct Simulation {
    particles: Vec<Particle>,
    rng: Rng,
    /// Fractional particles owed since the last spawn: `Rate` is continuous but
    /// particles are not, so the remainder carries over instead of rounding it
    /// away every step (which would silently slow down a low-rate emitter).
    owed: f32,
}

impl Simulation {
    pub(crate) fn new(seed: u64) -> Self {
        Simulation {
            particles: Vec::new(),
            rng: Rng::new(seed),
            owed: 0.0,
        }
    }

    /// Runs enough fixed `dt` steps to reach the steady state a continuously
    /// running emitter settles into — every particle a spawn could have
    /// produced since t=0 that has not yet expired. `Lifetime.1` (the longest a
    /// particle can live) is exactly that long: anything spawned before now
    /// minus that has already expired, whatever spawned after is already
    /// covered by a shorter run.
    pub(crate) fn prewarm(&mut self, emitter: &Emitter) {
        let seconds = emitter.lifetime.1.max(0.0);
        let steps = (seconds / PREWARM_STEP).ceil().max(0.0) as u32;
        for _ in 0..steps {
            self.step(emitter, PREWARM_STEP);
        }
    }

    /// Advances every alive particle by `dt`, drops the ones that expired, then
    /// spawns whatever the emitter owes for this step.
    pub(crate) fn step(&mut self, emitter: &Emitter, dt: f32) {
        if dt <= 0.0 {
            return;
        }

        let half_life_factor = if emitter.drag > 0.0 {
            0.5f32.powf(dt / emitter.drag)
        } else {
            1.0
        };
        for particle in &mut self.particles {
            particle.velocity = particle.velocity * half_life_factor + emitter.acceleration * dt;
            particle.position += particle.velocity * dt;
            particle.rotation += particle.rot_speed * dt;
            particle.age += dt;
        }
        self.particles
            .retain(|particle| particle.age < particle.lifetime);

        if emitter.rate <= 0.0 || emitter.cap == 0 {
            return;
        }
        self.owed += emitter.rate * dt;
        while self.owed >= 1.0 && (self.particles.len() as u32) < emitter.cap {
            self.owed -= 1.0;
            self.spawn(emitter);
        }
    }

    fn spawn(&mut self, emitter: &Emitter) {
        let local = Vec3::new(
            self.rng.range(-0.5, 0.5),
            self.rng.range(-0.5, 0.5),
            self.rng.range(-0.5, 0.5),
        );
        let position = emitter.volume.transform_point3(local);

        let direction = spread_direction(
            emitter.direction,
            self.rng
                .range(-emitter.spread_degrees.0, emitter.spread_degrees.0),
            self.rng
                .range(-emitter.spread_degrees.1, emitter.spread_degrees.1),
        );
        let speed = self.rng.range(emitter.speed.0, emitter.speed.1);
        let lifetime = self
            .rng
            .range(emitter.lifetime.0, emitter.lifetime.1)
            .max(0.0);
        if lifetime <= 0.0 {
            return;
        }

        self.particles.push(Particle {
            position,
            velocity: direction * speed,
            age: 0.0,
            lifetime,
            rotation: self
                .rng
                .range(emitter.rotation_degrees.0, emitter.rotation_degrees.1)
                .to_radians(),
            rot_speed: self
                .rng
                .range(emitter.rot_speed_degrees.0, emitter.rot_speed_degrees.1)
                .to_radians(),
        });
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.particles.len()
    }

    /// Snapshots every alive particle's current appearance, reading `Size`,
    /// `Transparency` and `Color` at its own age-over-lifetime fraction.
    pub(crate) fn particles<'a>(
        &'a self,
        emitter: &'a Emitter,
    ) -> impl Iterator<Item = RenderParticle> + 'a {
        self.particles.iter().map(move |particle| {
            let t = particle.age / particle.lifetime;
            RenderParticle {
                position: particle.position,
                size: eval_number(&emitter.size, t).max(0.0),
                rotation: particle.rotation,
                color: eval_color(&emitter.color, t),
                alpha: 1.0 - eval_number(&emitter.transparency, t).clamp(0.0, 1.0),
            }
        })
    }
}

/// Tilts `axis` by up to `degrees_a`/`degrees_b` around its two perpendiculars —
/// an ellipse-shaped cone approximating `SpreadAngle`'s independent X/Z limits.
/// Composing two small rotations is not exactly Roblox's own cone (which
/// samples the ellipse's interior, not just this one diagonal), but it keeps
/// every direction within the same angular bound, which is what the emission
/// cone is actually for — see the module's tests.
fn spread_direction(axis: Vec3, degrees_a: f32, degrees_b: f32) -> Vec3 {
    if degrees_a == 0.0 && degrees_b == 0.0 {
        return axis;
    }
    let (a, b) = perpendiculars(axis);
    let tilt = Quat::from_axis_angle(b, degrees_a.to_radians())
        * Quat::from_axis_angle(a, degrees_b.to_radians());
    tilt * axis
}

/// Any two vectors orthonormal to `axis` and to each other. Which pair (there
/// are infinitely many) is unobservable from `SpreadAngle` alone, since nothing
/// else in a static viewer ties the spread ellipse to a specific part face.
fn perpendiculars(axis: Vec3) -> (Vec3, Vec3) {
    let aside = if axis.y.abs() < 0.99 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let a = aside.cross(axis).normalize_or_zero();
    let b = axis.cross(a);
    (a, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Mat4;
    use rbx_assets::AssetRef;
    use rbx_dom::{NumberSequence, NumberSequenceKeypoint};

    fn flat(value: f32) -> NumberSequence {
        NumberSequence {
            keypoints: vec![
                NumberSequenceKeypoint {
                    time: 0.0,
                    value,
                    envelope: 0.0,
                },
                NumberSequenceKeypoint {
                    time: 1.0,
                    value,
                    envelope: 0.0,
                },
            ],
        }
    }

    fn emitter(rate: f32, lifetime: (f32, f32)) -> Emitter {
        Emitter {
            rate,
            lifetime,
            speed: (5.0, 5.0),
            spread_degrees: (0.0, 0.0),
            direction: Vec3::Y,
            acceleration: Vec3::ZERO,
            drag: 0.0,
            size: flat(1.0),
            transparency: flat(0.0),
            color: rbx_dom::ColorSequence {
                keypoints: vec![rbx_dom::ColorSequenceKeypoint {
                    time: 0.0,
                    color: rbx_dom::Color3Data {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                    },
                    envelope: 0.0,
                }],
            },
            texture: AssetRef::Empty,
            light_emission: 0.0,
            rotation_degrees: (0.0, 0.0),
            rot_speed_degrees: (0.0, 0.0),
            z_offset: 0.0,
            gain: 1.0,
            time_scale: 1.0,
            cap: super::super::emitter::PER_EMITTER_CAP,
            seed: 1,
            referent: rbx_dom::Ref::new(1),
            slot: 0,
            volume: Mat4::IDENTITY,
        }
    }

    #[test]
    fn a_step_spawns_rate_times_dt_particles() {
        let e = emitter(10.0, (100.0, 100.0));
        let mut sim = Simulation::new(1);
        sim.step(&e, 0.5);
        assert_eq!(sim.len(), 5);
    }

    #[test]
    fn fractional_spawns_carry_over_instead_of_being_dropped() {
        let e = emitter(1.0, (100.0, 100.0));
        let mut sim = Simulation::new(1);
        // 0.9 owed four times is 3.6: three whole particles have spawned.
        for _ in 0..4 {
            sim.step(&e, 0.9);
        }
        assert_eq!(sim.len(), 3);
    }

    #[test]
    fn a_particle_disappears_once_its_age_reaches_its_lifetime() {
        let mut e = emitter(1.0, (1.0, 1.0));
        let mut sim = Simulation::new(1);
        sim.step(&e, 1.0); // owed reaches exactly 1: spawns one, at age 0
        assert_eq!(sim.len(), 1);
        e.rate = 0.0; // isolate ageing from the next step's own spawn
        sim.step(&e, 1.0); // ages it past its 1s lifetime
        assert_eq!(sim.len(), 0);
    }

    #[test]
    fn disabling_spawn_via_zero_rate_still_ages_existing_particles() {
        let mut e = emitter(1.0, (10.0, 10.0));
        let mut sim = Simulation::new(1);
        sim.step(&e, 1.0);
        assert_eq!(sim.len(), 1);
        e.rate = 0.0;
        sim.step(&e, 1.0);
        assert_eq!(sim.len(), 1, "existing particles keep ageing");
    }

    #[test]
    fn a_zero_cap_spawns_nothing() {
        let mut e = emitter(100.0, (10.0, 10.0));
        e.cap = 0;
        let mut sim = Simulation::new(1);
        sim.step(&e, 1.0);
        assert_eq!(sim.len(), 0);
    }

    #[test]
    fn prewarm_reaches_the_rate_times_mean_lifetime_steady_state() {
        let e = emitter(20.0, (2.0, 4.0));
        let mut sim = Simulation::new(1);
        sim.prewarm(&e);
        // Continuous spawning over a mean lifetime of 3s at 20/s settles near 60
        // alive particles; generous tolerance since Lifetime is itself random.
        let expected = e.rate * (e.lifetime.0 + e.lifetime.1) / 2.0;
        assert!(
            (sim.len() as f32 - expected).abs() < expected * 0.25,
            "got {}, expected close to {expected}",
            sim.len()
        );
    }

    #[test]
    fn spread_direction_stays_within_the_combined_cone_angle() {
        let axis = Vec3::Y;
        for degrees_a in [0.0, 10.0, 45.0] {
            for degrees_b in [0.0, 20.0, 30.0] {
                let direction = spread_direction(axis, degrees_a, degrees_b);
                assert!((direction.length() - 1.0).abs() < 1e-4);
                let bound = (degrees_a.abs() + degrees_b.abs()).to_radians().cos() - 1e-3;
                assert!(
                    direction.dot(axis) >= bound,
                    "direction {direction:?} strayed past the {degrees_a}/{degrees_b} cone"
                );
            }
        }
    }

    #[test]
    fn zero_spread_never_deviates_from_the_emission_axis() {
        assert_eq!(spread_direction(Vec3::Y, 0.0, 0.0), Vec3::Y);
    }

    #[test]
    fn render_snapshot_reads_appearance_at_age_over_lifetime() {
        let mut e = emitter(1.0, (2.0, 2.0));
        e.transparency = NumberSequence {
            keypoints: vec![
                NumberSequenceKeypoint {
                    time: 0.0,
                    value: 0.0,
                    envelope: 0.0,
                },
                NumberSequenceKeypoint {
                    time: 1.0,
                    value: 1.0,
                    envelope: 0.0,
                },
            ],
        };
        let mut sim = Simulation::new(1);
        sim.step(&e, 1.0); // owed reaches exactly 1: spawns one, at age 0
        e.rate = 0.0; // isolate the snapshot from the next step's own spawn
        sim.step(&e, 1.0); // now half-way through its 2s lifetime
        let snapshot: Vec<_> = sim.particles(&e).collect();
        assert_eq!(snapshot.len(), 1);
        assert!((snapshot[0].alpha - 0.5).abs() < 0.1);
    }
}
