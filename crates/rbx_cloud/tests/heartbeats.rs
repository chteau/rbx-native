//! Presence pulses against an injected transport: no network.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rbx_cloud::heartbeats::{
    pulse_body, send_pulse, PulseError, Pulser, PulserEvent, Reply, SessionCookie, StudioLocation,
    PULSE_INTERVAL,
};
use serde_json::{json, Value};

const NO_PLACE: StudioLocation = StudioLocation { place: None };

fn reply(status: u16, csrf: Option<&str>) -> Result<Reply, String> {
    Ok(Reply {
        status,
        csrf: csrf.map(str::to_owned),
        body: String::new(),
    })
}

#[test]
fn pulse_body_without_place_has_empty_studio_location() {
    let body = pulse_body("s1", NO_PLACE, 42);
    assert_eq!(
        body,
        json!({
            "clientSideTimestampEpochMs": 42,
            "sessionInfo": { "sessionId": "s1" },
            "locationInfo": { "studioLocationInfo": {} },
        })
    );
}

#[test]
fn pulse_body_with_place_carries_both_ids() {
    let body = pulse_body(
        "s1",
        StudioLocation {
            place: Some((10, 20)),
        },
        42,
    );
    assert_eq!(
        body["locationInfo"]["studioLocationInfo"],
        json!({ "placeId": 10, "universeId": 20 })
    );
}

#[test]
fn send_pulse_success_is_one_call() {
    let mut calls = 0;
    let mut csrf = String::new();
    let mut post = |_: &Value, _: &str| {
        calls += 1;
        reply(200, None)
    };
    let result = send_pulse(&json!({}), false, &mut csrf, &mut post);
    assert_eq!(result, Ok(()));
    assert_eq!(calls, 1);
}

#[test]
fn send_pulse_retries_once_with_new_csrf_token() {
    let mut seen: Vec<String> = Vec::new();
    let mut csrf = String::new();
    let mut post = |_: &Value, token: &str| {
        seen.push(token.to_owned());
        if seen.len() == 1 {
            reply(403, Some("t1"))
        } else {
            reply(200, None)
        }
    };
    let result = send_pulse(&json!({}), false, &mut csrf, &mut post);
    assert_eq!(result, Ok(()));
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[1], "t1");
    assert_eq!(csrf, "t1");
}

#[test]
fn send_pulse_403_without_new_token_is_unauthorized_without_place() {
    let mut csrf = String::new();
    let mut post = |_: &Value, _: &str| reply(403, None);
    let result = send_pulse(&json!({}), false, &mut csrf, &mut post);
    assert_eq!(result, Err(PulseError::Unauthorized));
}

#[test]
fn send_pulse_403_without_new_token_is_not_editable_with_place() {
    let mut csrf = String::new();
    let mut post = |_: &Value, _: &str| reply(403, None);
    let result = send_pulse(&json!({}), true, &mut csrf, &mut post);
    assert_eq!(result, Err(PulseError::PlaceNotEditable));
}

#[test]
fn send_pulse_repeated_csrf_token_does_not_loop() {
    let mut calls = 0;
    let mut csrf = String::new();
    let mut post = |_: &Value, _: &str| {
        calls += 1;
        reply(403, Some("t1"))
    };
    let result = send_pulse(&json!({}), false, &mut csrf, &mut post);
    assert!(result.is_err());
    assert!(calls <= 2, "made {calls} calls");
}

#[test]
fn send_pulse_500_is_other() {
    let mut csrf = String::new();
    let mut post = |_: &Value, _: &str| reply(500, None);
    let result = send_pulse(&json!({}), false, &mut csrf, &mut post);
    assert!(matches!(result, Err(PulseError::Other(_))), "{result:?}");
}

#[test]
fn send_pulse_transport_error_is_other() {
    let mut csrf = String::new();
    let mut post = |_: &Value, _: &str| -> Result<Reply, String> { Err("reset".to_owned()) };
    let result = send_pulse(&json!({}), false, &mut csrf, &mut post);
    assert!(matches!(result, Err(PulseError::Other(_))), "{result:?}");
}

#[test]
fn pulser_pulses_repeatedly_at_interval() {
    let post = Box::new(move |_: &Value, _: &str| -> Result<Reply, String> { reply(200, None) });
    let (_pulser, events) = Pulser::start_with(NO_PLACE, Duration::from_millis(50), post);
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut pulsed = 0;
    while pulsed < 3 {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match events.recv_timeout(remaining) {
            Ok(PulserEvent::Pulsed) => pulsed += 1,
            Ok(other) => panic!("unexpected event {other:?}"),
            Err(_) => break,
        }
    }
    assert!(pulsed >= 3, "only {pulsed} pulses within 1 s");
}

#[test]
fn pulser_set_location_pulses_with_new_place() {
    let bodies: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&bodies);
    let post = Box::new(move |body: &Value, _: &str| -> Result<Reply, String> {
        recorded.lock().unwrap().push(body.clone());
        reply(200, None)
    });
    let (pulser, _events) = Pulser::start_with(NO_PLACE, PULSE_INTERVAL, post);
    pulser.set_location(StudioLocation {
        place: Some((10, 20)),
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let found = bodies
            .lock()
            .unwrap()
            .iter()
            .any(|b| b["locationInfo"]["studioLocationInfo"]["placeId"] == 10);
        if found {
            break;
        }
        assert!(Instant::now() < deadline, "no pulse carried placeId 10");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn pulser_signs_out_and_stops_on_unauthorized() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let post = Box::new(move |_: &Value, _: &str| -> Result<Reply, String> {
        counter.fetch_add(1, Ordering::SeqCst);
        reply(403, None)
    });
    let (_pulser, events) = Pulser::start_with(NO_PLACE, Duration::from_millis(50), post);
    let first = events.recv_timeout(Duration::from_secs(1));
    assert_eq!(first, Ok(PulserEvent::SignedOut));
    let after = calls.load(Ordering::SeqCst);
    thread::sleep(Duration::from_millis(200));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        after,
        "pulsed after SignedOut"
    );
}

#[test]
fn session_cookie_debug_hides_secret() {
    let cookie = SessionCookie::new(".ROBLOSECURITY=abc ");
    assert!(!format!("{cookie:?}").contains("abc"));
}
