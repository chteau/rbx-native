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
}
