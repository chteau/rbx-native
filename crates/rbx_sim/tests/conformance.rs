//! rbx_sim against the goldens recorded from real Roblox. A test whose
//! golden names a `known_gap` is reported, not failed; an unrecorded one
//! is skipped.

use rbx_sim::conformance::{self, TESTS};

#[test]
fn matches_the_recorded_goldens() {
    let ours = conformance::run_in_sim(1200.0).expect("the suite runs in rbx_sim");
    let mut failures = Vec::new();
    for test in TESTS {
        let Some(golden) = conformance::golden(test.name).filter(|g| g["status"] == "recorded")
        else {
            continue;
        };
        let mine = ours.get(test.name).cloned().unwrap_or_default();
        let rows = conformance::compare(test, &golden, &mine);
        let off: Vec<String> = rows
            .iter()
            .filter(|r| !r.pass)
            .map(|r| {
                format!(
                    "{}.{}: roblox {} ours {}",
                    r.test, r.field, r.golden, r.ours
                )
            })
            .collect();
        if off.is_empty() {
            continue;
        }
        match golden["known_gap"].as_str() {
            Some(gap) => eprintln!("known gap in {}: {gap}", test.name),
            None => failures.extend(off),
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
