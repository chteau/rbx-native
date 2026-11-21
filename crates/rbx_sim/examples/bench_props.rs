//! Frame times for a busy place: `n` unanchored mesh props (1200 by default)
//! dropped on a baseplate, settling, then resting.
//!
//! cargo run --release -p rbx_sim --example bench_props -- [n] [--seconds N]
//!
//! Prints the mean and worst frame per simulated second. Once the props
//! rest, a frame should cost a small fraction of the 16.7 ms a 60 Hz frame
//! has.

use std::time::{Duration, Instant};

use rbx_sim::{conformance, physics, Session, FRAME_HZ};

fn main() {
    let mut n = 1200;
    let mut seconds = 8.0;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seconds" => {
                seconds = args
                    .next()
                    .and_then(|s| s.parse().ok())
                    .expect("--seconds N")
            }
            other => n = other.parse().expect("usage: bench_props [n] [--seconds N]"),
        }
    }
    let (dom, props) = conformance::prop_field(n);
    let (vertices, triangles) = conformance::rock();
    physics::share_mesh(&props, vertices, triangles);
    let mut session = Session::new(dom).expect("session");
    session.start();
    let per_second = FRAME_HZ as usize;
    let mut frames = Vec::with_capacity(per_second);
    for frame in 0..(seconds * FRAME_HZ) as usize {
        let started = Instant::now();
        session.step();
        frames.push(started.elapsed());
        if frames.len() == per_second {
            let total: Duration = frames.iter().sum();
            let worst = frames.iter().max().unwrap();
            println!(
                "second {:2}: mean {:6.2} ms, worst {:6.2} ms",
                frame / per_second,
                total.as_secs_f64() * 1000.0 / per_second as f64,
                worst.as_secs_f64() * 1000.0
            );
            frames.clear();
        }
    }
}
