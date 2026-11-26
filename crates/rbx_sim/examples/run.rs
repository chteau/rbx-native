//! Runs a place headless and prints what its scripts printed, then where
//! every unanchored part ended up.
//!
//! ```text
//! cargo run -p rbx_sim --example run -- place.rbxl --seconds 5
//! ```

use std::path::PathBuf;

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_sim::{LineKind, Simulation};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut place = None;
    let mut seconds = 5.0;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seconds" => {
                seconds = args
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(|| usage());
            }
            _ if place.is_none() => place = Some(PathBuf::from(arg)),
            _ => usage(),
        }
    }
    let place = place.unwrap_or_else(|| usage());
    let dom = rbx_sim::load::read_place(&place).unwrap_or_else(|err| fail(&err));
    let mut sim = Simulation::new(dom).unwrap_or_else(|err| fail(&err));
    let started = std::time::Instant::now();
    sim.start();
    print_output(&mut sim);
    let frames = (seconds * rbx_sim::FRAME_HZ).round() as u64;
    for _ in 0..frames {
        sim.step();
        print_output(&mut sim);
    }
    let wall = started.elapsed().as_secs_f64();
    println!(
        "-- {:.2}s of game time ({} frames) in {wall:.2}s; {} parts in {} assemblies",
        sim.time(),
        sim.frame(),
        sim.world().part_count(),
        sim.world().assembly_count()
    );
    dump(&sim.dom());
}

fn print_output(sim: &mut Simulation) {
    let time = sim.time();
    let output = sim.take_output();
    for (line, kind) in output.lines().iter().zip(output.kinds()) {
        let tag = match kind {
            LineKind::Print => "",
            LineKind::Warn => "warn: ",
            LineKind::Error => "error: ",
        };
        println!("[{time:7.3}] {tag}{line}");
    }
}

fn dump(dom: &WeakDom) {
    let db = rbx_reflection::ReflectionDatabase::shared();
    let Some(workspace) = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "Workspace"))
    else {
        return;
    };
    let mut stack: Vec<(Ref, String)> = vec![(workspace, "Workspace".into())];
    while let Some((at, path)) = stack.pop() {
        let Some(instance) = dom.get(at) else {
            continue;
        };
        for child in instance.children().iter().rev() {
            if let Some(c) = dom.get(*child) {
                stack.push((*child, format!("{path}.{}", c.name())));
            }
        }
        if !db.is_subclass_of(instance.class(), "BasePart") {
            continue;
        }
        let read = |name| db.stored_or_default(instance, name).map(|(_, v)| v.clone());
        if matches!(read("Anchored"), Some(Variant::Bool(true))) {
            continue;
        }
        let (Some(Variant::CFrame(cf)), velocity) =
            (read("CFrame"), read("AssemblyLinearVelocity"))
        else {
            continue;
        };
        let v = match velocity {
            Some(Variant::Vector3(v)) => v,
            _ => rbx_dom::Vector3Data {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        };
        let p = cf.position;
        println!(
            "{path}: position ({:.3}, {:.3}, {:.3}) velocity ({:.3}, {:.3}, {:.3})",
            p.x, p.y, p.z, v.x, v.y, v.z
        );
    }
}

fn usage() -> ! {
    eprintln!("usage: run <place.rbxl|.rbxlx> [--seconds N]");
    std::process::exit(2);
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}
