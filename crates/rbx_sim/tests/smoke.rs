//! Our own small games (`tests/smoke/*.rbxlx`) played headless by the smoke
//! bot (`examples/support/smoke.rs`): no script errors, the prints the game
//! makes when it works, the leaderstats it should end on. The same bot runs
//! any place: `cargo run -p rbx_sim --example smoke -- place.rbxl`.

#[path = "../examples/support/smoke.rs"]
mod smoke;

fn play(fixture: &str, expect: &[&str], stats: &[(&str, &str)]) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/smoke")
        .join(fixture);
    let dom = rbx_sim::load::read_place(&path).unwrap();
    let report = smoke::run(dom, 8.0).unwrap();
    let failures = report.failures(expect, stats);
    assert!(
        failures.is_empty(),
        "{fixture}:\n{}\n{}",
        failures.join("\n"),
        report.transcript()
    );
}

#[test]
fn obby_checkpoints_save_to_the_datastore() {
    play(
        "obby.rbxlx",
        &[
            "loaded stage true 0",
            "checkpoint 1 saved 1",
            "checkpoint 2 saved 2",
            "jumped",
        ],
        &[("Stage", "2")],
    );
}

#[test]
fn sword_swings_reach_the_server_through_a_remote() {
    play(
        "combat.rbxlx",
        &[
            "equipped Sword",
            "hit Dummy 60",
            "hit Dummy 20",
            "hit Dummy -20",
            "dummy down",
        ],
        &[("Hits", "3"), ("KOs", "1")],
    );
}

#[test]
fn the_shop_button_buys_through_a_remote_function() {
    play(
        "shop.rbxlx",
        &["shop true 70", "shop true 40", "shop true 10"],
        &[("Coins", "10"), ("Items", "3")],
    );
}

#[test]
fn a_streaming_map_runs() {
    play(
        "streaming.rbxlx",
        &[
            "streaming true",
            "persistent Tower",
            "requested stream",
            "streamed FarCoin",
        ],
        &[("Visits", "1")],
    );
}
