//! One `ParticleEmitter2D`'s live particles, kept in the parent's own turned
//! axes so a locked particle follows its parent for free.

use super::super::emitter::{seed_of, PER_EMITTER_CAP};
use super::super::rng::Rng;
use super::super::sequence::{eval_color, eval_number_spread};
use super::{FlatFrame, FlipbookMode, GuiEmitter};

struct Particle {
    /// Pixels from the parent's centre, along its turned axes.
    position: [f32; 2],
    velocity: [f32; 2],
    age: f32,
    lifetime: f32,
    /// Degrees, before the parent's own turn.
    rotation: f32,
    rot_speed: f32,
    /// Where in its keypoints' envelopes this particle sits, in `[-1, 1]`.
    spread: f32,
    /// The parent as it was at birth, which an unlocked particle keeps.
    born: FlatFrame,
    framerate: f32,
    first_frame: f32,
    seed: u64,
}

/// One particle as drawn: its centre on screen, its full width and height,
/// its turn, colour (linear) and opacity, and the flipbook frames it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FlatParticle {
    pub(crate) center: [f32; 2],
    pub(crate) extent: [f32; 2],
    /// Degrees clockwise.
    pub(crate) rotation: f32,
    pub(crate) color: [f32; 3],
    pub(crate) alpha: f32,
    /// The frame shown, the next one, and how far towards it.
    pub(crate) frames: (u32, u32, f32),
}

/// A particle system's state between frames.
pub(crate) struct FlatSim {
    particles: Vec<Particle>,
    rng: Rng,
    /// Rate-driven particles owed since the last spawn, see the 3D sim's.
    owed: f32,
    /// `Emit` calls not spawned yet: they need the parent's frame.
    pending: u32,
}

impl FlatSim {
    pub(crate) fn new(seed: u64) -> Self {
        FlatSim {
            particles: Vec::new(),
            rng: Rng::new(seed),
            owed: 0.0,
            pending: 0,
        }
    }

    /// Seeded from the emitter's referent, so the same place draws the same
    /// particles run after run.
    pub(crate) fn seeded(referent: rbx_dom::Ref) -> Self {
        Self::new(seed_of(referent.value(), 0))
    }

    /// `Emit(count)`: spawned at the next [`FlatSim::step`].
    pub(crate) fn emit(&mut self, count: u32) {
        self.pending = self.pending.saturating_add(count);
    }

    /// `Clear()`.
    pub(crate) fn clear(&mut self) {
        self.particles.clear();
        self.pending = 0;
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.particles.is_empty()
    }

    /// Advances by `dt` seconds of wall clock (`TimeScale` is applied here),
    /// then spawns what `Rate` and any `Emit` owe, against `frame`.
    pub(crate) fn step(&mut self, emitter: &GuiEmitter, frame: FlatFrame, dt: f32) {
        let dt = dt.max(0.0) * emitter.time_scale;
        // Measured in Studio: speed halves `Drag` times a second (Drag 2
        // travels half as far as Drag 1, Drag 0.5 twice as far).
        let damping = 0.5f32.powf(dt * emitter.drag);
        for particle in &mut self.particles {
            let scale = particle.born.scale;
            for axis in 0..2 {
                particle.velocity[axis] =
                    particle.velocity[axis] * damping + emitter.acceleration[axis] * scale * dt;
                particle.position[axis] += particle.velocity[axis] * dt;
            }
            particle.rotation += particle.rot_speed * dt;
            particle.age += dt;
        }
        self.particles
            .retain(|particle| particle.age < particle.lifetime);

        let mut due = std::mem::take(&mut self.pending);
        if emitter.enabled && emitter.rate > 0.0 {
            self.owed += emitter.rate * dt;
            let whole = self.owed.floor();
            self.owed -= whole;
            due = due.saturating_add(whole as u32);
        }
        for _ in 0..due {
            if self.particles.len() >= PER_EMITTER_CAP as usize {
                break;
            }
            self.spawn(emitter, frame);
        }
    }

    fn spawn(&mut self, emitter: &GuiEmitter, frame: FlatFrame) {
        let lifetime = self
            .rng
            .range(emitter.lifetime.0, emitter.lifetime.1)
            .max(0.0);
        let rng = &mut self.rng;
        let fill = match emitter.bounding_box {
            true => 1.0,
            false => 0.0,
        };
        let position = [0, 1].map(|axis| {
            let extent = frame.size[axis];
            let area = extent * (emitter.spread[axis] + fill);
            emitter.offset[axis] * extent + rng.range(-0.5, 0.5) * area
        });
        let angle =
            (emitter.angle + rng.range(-emitter.spread_angle, emitter.spread_angle)).to_radians();
        let speed = rng.range(emitter.speed.0, emitter.speed.1) * frame.scale;
        let book = emitter.flipbook;
        let particle = Particle {
            position,
            velocity: [angle.sin() * speed, -angle.cos() * speed],
            age: 0.0,
            lifetime,
            rotation: rng.range(emitter.rotation.0, emitter.rotation.1),
            rot_speed: rng.range(emitter.rot_speed.0, emitter.rot_speed.1),
            spread: rng.range(-1.0, 1.0),
            born: frame,
            framerate: rng.range(book.framerate.0, book.framerate.1).max(0.0),
            first_frame: match book.start_random {
                true => rng.range(0.0, book.frames() as f32).floor(),
                false => 0.0,
            },
            seed: (rng.range(0.0, 1.0) * u32::MAX as f32) as u64,
        };
        if lifetime > 0.0 {
            self.particles.push(particle);
        }
    }

    /// Every live particle as drawn, oldest first, against `frame`, the
    /// parent where it is now.
    pub(crate) fn particles<'a>(
        &'a self,
        emitter: &'a GuiEmitter,
        frame: FlatFrame,
    ) -> impl Iterator<Item = FlatParticle> + 'a {
        self.particles.iter().map(move |particle| {
            let t = particle.age / particle.lifetime;
            let parent = match emitter.locked {
                true => frame,
                false => particle.born,
            };
            let size = eval_number_spread(&emitter.size, t, particle.spread).max(0.0)
                * 2.0
                * particle.born.scale;
            let squash = eval_number_spread(&emitter.squash, t, particle.spread);
            FlatParticle {
                center: parent.to_screen(particle.position),
                extent: [size * 2f32.powf(-squash), size * 2f32.powf(squash)],
                rotation: particle.rotation + parent.rotation,
                color: eval_color(&emitter.color, t),
                alpha: 1.0
                    - eval_number_spread(&emitter.transparency, t, -particle.spread)
                        .clamp(0.0, 1.0),
                frames: frames(emitter, particle),
            }
        })
    }
}

/// Which flipbook frame a particle shows, the one after it, and how far
/// between the two it is. Measured in Studio: `Loop` and `PingPong` play at
/// the particle's own framerate from its first frame, `OneShot` spreads
/// every frame over the particle's life whatever the framerate, and
/// `Random` jumps to a random frame at each tick.
fn frames(emitter: &GuiEmitter, particle: &Particle) -> (u32, u32, f32) {
    let book = emitter.flipbook;
    let count = book.frames();
    if count <= 1 {
        return (0, 0, 0.0);
    }
    let played = particle.first_frame + particle.age * particle.framerate;
    let (current, next, between) = match book.mode {
        FlipbookMode::Loop => {
            let at = played.floor() as u64;
            (at % count as u64, (at + 1) % count as u64, played.fract())
        }
        FlipbookMode::OneShot => {
            let last = (count - 1) as f32;
            let at = (particle.first_frame + particle.age / particle.lifetime * count as f32)
                .clamp(0.0, last);
            let current = at.floor();
            (
                current as u64,
                (current + 1.0).min(last) as u64,
                at - current,
            )
        }
        FlipbookMode::PingPong => {
            let period = 2 * (count as u64 - 1);
            let bounce = |step: u64| {
                let step = step % period;
                match step < count as u64 {
                    true => step,
                    false => period - step,
                }
            };
            let at = played.floor() as u64;
            (bounce(at), bounce(at + 1), played.fract())
        }
        FlipbookMode::Random => {
            let at = played.floor() as u64;
            let pick = |step: u64| {
                Rng::new(particle.seed ^ step.wrapping_mul(0x9E37_79B9_7F4A_7C15))
                    .range(0.0, count as f32)
                    .min(count as f32 - 1.0) as u64
            };
            (pick(at), pick(at + 1), played.fract())
        }
    };
    let between = match book.blend_frames {
        true => between,
        false => 0.0,
    };
    (current as u32, next as u32, between)
}
