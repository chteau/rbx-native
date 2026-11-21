//! A busy place's settled props go to sleep and stay still, so they cost
//! nothing a frame: the regression test for M18's speed work. The bench
//! with frame times is `examples/bench_props.rs`.

use rbx_dom::{Ref, Variant};
use rbx_sim::{conformance, physics, Peer, Session};

fn positions(session: &Session, props: &[Ref]) -> Vec<[f32; 3]> {
    let dom = session.sim(Peer::Server).unwrap().dom();
    props
        .iter()
        .map(|p| match dom.get(*p).unwrap().properties().get("CFrame") {
            Some(Variant::CFrame(cf)) => [cf.position.x, cf.position.y, cf.position.z],
            _ => panic!("a prop without a CFrame"),
        })
        .collect()
}

#[test]
fn settled_mesh_props_sleep_and_stay_still() {
    let (dom, props) = conformance::prop_field(256);
    let (vertices, triangles) = conformance::rock();
    physics::share_mesh(&props, vertices, triangles);
    let mut session = Session::new(dom).unwrap();
    session.start();
    for _ in 0..(5.0 * rbx_sim::FRAME_HZ) as usize {
        session.step();
    }
    let settled = positions(&session, &props);
    assert!(
        settled.iter().all(|p| p[1] > 0.0 && p[1] < 3.0),
        "every rock rests on the baseplate"
    );
    for _ in 0..60 {
        session.step();
    }
    assert_eq!(
        positions(&session, &props),
        settled,
        "asleep, nothing moves"
    );
}
