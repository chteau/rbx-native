//! Live smoke test against real Roblox endpoints. Ignored by default since it
//! needs a real (test) API key and hits the network:
//!
//! ```sh
//! RBX_API_KEY=... cargo test -p rbx_cloud --test live -- --ignored
//! ```

const TEST_UNIVERSE_ID: u64 = 6053515322;
const TEST_PLACE_ID: u64 = 17675488706;

#[test]
#[ignore]
fn introspect_universe_and_download_place() {
    let Some(key) = rbx_cloud::ApiKey::from_env_or_config() else {
        eprintln!("no API key configured (RBX_API_KEY unset), skipping");
        return;
    };
    let client = rbx_cloud::Client::new(Some(key));

    let info = client.introspect().expect("introspect should succeed");
    assert!(info.enabled);
    assert!(!info.expired);

    let universe = client
        .universe(TEST_UNIVERSE_ID)
        .expect("universe lookup should succeed");
    assert_eq!(universe.root_place_id, TEST_PLACE_ID);

    let bytes = client
        .download_place(TEST_PLACE_ID)
        .expect("place download should succeed");
    assert!(bytes.starts_with(b"<roblox"));

    // A private place reached by its link alone, as Home's "Add by place ID
    // or URL" does it.
    let link = format!("https://www.roblox.com/games/{TEST_PLACE_ID}/x");
    let place_id = rbx_cloud::place_id_from_link(&link).unwrap();
    let experience = client
        .experience_of_place(place_id)
        .expect("place lookup should succeed")
        .expect("the test place exists");
    assert_eq!(experience.universe_id, TEST_UNIVERSE_ID);
    assert_eq!(experience.root_place_id, TEST_PLACE_ID);
}

/// The personal listing, without any group's games: what Home waits on.
#[test]
#[ignore]
fn listing_is_quick_and_group_games_come_one_group_at_a_time() {
    let Some(key) = rbx_cloud::ApiKey::from_env_or_config() else {
        eprintln!("no API key configured (RBX_API_KEY unset), skipping");
        return;
    };
    let client = rbx_cloud::Client::new(Some(key));
    let started = std::time::Instant::now();
    let listing = client.list_experiences().expect("listing should succeed");
    eprintln!(
        "listed {} experiences and {} groups in {:?}",
        listing.experiences.len(),
        listing.groups.len(),
        started.elapsed()
    );
    if let Some(group) = listing.groups.first() {
        let started = std::time::Instant::now();
        let games = client.group_experiences(group.id).expect("group listing");
        eprintln!(
            "group {} has {} games ({:?})",
            group.id,
            games.len(),
            started.elapsed()
        );
        assert!(games
            .iter()
            .all(|g| g.owner == rbx_cloud::Owner::Group(group.id)));
    }
}
