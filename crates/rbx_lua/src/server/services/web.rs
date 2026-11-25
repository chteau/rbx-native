//! Roblox's web APIs as a game that spams them reaches them: callers asking
//! the same thing at once share one request, each host gets a few requests
//! in flight at a time, and an HTTP 429 pauses that host (for its
//! `Retry-After`, else 1, 2, 4… seconds) and retries, the way Roblox's own
//! lookups yield longer under load instead of failing at once.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, LazyLock, Mutex};
use std::time::{Duration, Instant};

use crate::plugin::http::{self, HttpRequest, HttpResponse};

/// Requests in flight per host.
const PER_HOST: usize = 4;
/// Retries after a 429 before its answer stands.
const RETRIES: u32 = 5;
/// The longest a 429 pauses a host.
const MAX_PAUSE: Duration = Duration::from_secs(30);

type Answer = Result<HttpResponse, String>;

#[derive(Default)]
struct Shared {
    answer: Mutex<Option<Answer>>,
    ready: Condvar,
}

#[derive(Default)]
struct Host {
    busy: usize,
    paused_until: Option<Instant>,
}

#[derive(Default)]
struct State {
    in_flight: HashMap<String, Arc<Shared>>,
    hosts: HashMap<String, Host>,
}

static STATE: LazyLock<(Mutex<State>, Condvar)> = LazyLock::new(Default::default);

fn lock() -> std::sync::MutexGuard<'static, State> {
    STATE.0.lock().unwrap_or_else(|e| e.into_inner())
}

/// `request`'s answer, shared with any identical request already going.
pub(super) fn send(request: &HttpRequest) -> Answer {
    send_with(request, http::send)
}

fn send_with(request: &HttpRequest, transport: impl Fn(&HttpRequest) -> Answer) -> Answer {
    let key = format!(
        "{} {} {}",
        request.method,
        request.url,
        request.body.as_deref().unwrap_or("")
    );
    let (shared, leader) = {
        let mut state = lock();
        match state.in_flight.get(&key) {
            Some(found) => (found.clone(), false),
            None => {
                let made = Arc::new(Shared::default());
                state.in_flight.insert(key.clone(), made.clone());
                (made, true)
            }
        }
    };
    if !leader {
        let mut answer = shared.answer.lock().unwrap_or_else(|e| e.into_inner());
        while answer.is_none() {
            answer = shared.ready.wait(answer).unwrap_or_else(|e| e.into_inner());
        }
        return answer.clone().unwrap_or_else(|| Err("no answer".into()));
    }
    let host = http::host_of(&request.url).unwrap_or_default();
    let answer = paced(&host, request, &transport);
    lock().in_flight.remove(&key);
    *shared.answer.lock().unwrap_or_else(|e| e.into_inner()) = Some(answer.clone());
    shared.ready.notify_all();
    answer
}

/// One request to `host` within its budget, retried after a 429.
fn paced(host: &str, request: &HttpRequest, transport: &impl Fn(&HttpRequest) -> Answer) -> Answer {
    let mut attempt = 0;
    loop {
        take_slot(host);
        let answer = transport(request);
        let throttled = matches!(&answer, Ok(r) if r.status_code == 429);
        let mut state = lock();
        let entry = state.hosts.entry(host.to_owned()).or_default();
        entry.busy = entry.busy.saturating_sub(1);
        if throttled && attempt < RETRIES {
            let pause = answer
                .as_ref()
                .ok()
                .and_then(retry_after)
                .unwrap_or(Duration::from_secs(1 << attempt))
                .min(MAX_PAUSE);
            let until = Instant::now() + pause;
            entry.paused_until = Some(entry.paused_until.map_or(until, |u| u.max(until)));
            attempt += 1;
            drop(state);
            STATE.1.notify_all();
            continue;
        }
        drop(state);
        STATE.1.notify_all();
        return answer;
    }
}

/// Waits until `host` has a free slot and isn't paused, then takes it.
fn take_slot(host: &str) {
    let mut state = lock();
    loop {
        let entry = state.hosts.entry(host.to_owned()).or_default();
        let now = Instant::now();
        match entry.paused_until {
            Some(until) if until > now => {
                let wait = until - now;
                state = STATE
                    .1
                    .wait_timeout(state, wait)
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
                continue;
            }
            _ => entry.paused_until = None,
        }
        if entry.busy < PER_HOST {
            entry.busy += 1;
            return;
        }
        state = STATE.1.wait(state).unwrap_or_else(|e| e.into_inner());
    }
}

/// A 429's `Retry-After` in seconds.
fn retry_after(response: &HttpResponse) -> Option<Duration> {
    response
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("retry-after"))
        .and_then(|(_, value)| value.trim().parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s >= 0.0)
        .map(Duration::from_secs_f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn request(url: &str) -> HttpRequest {
        HttpRequest {
            url: url.into(),
            method: "GET".into(),
            headers: Vec::new(),
            body: None,
        }
    }

    fn response(status: u16, headers: Vec<(String, String)>) -> HttpResponse {
        HttpResponse {
            success: status == 200,
            status_code: status,
            status_message: String::new(),
            headers,
            body: "{}".into(),
        }
    }

    #[test]
    fn a_429_waits_out_retry_after_and_retries() {
        let calls = AtomicUsize::new(0);
        let started = Instant::now();
        let answer = send_with(&request("https://throttled.test/a"), |_| {
            Ok(match calls.fetch_add(1, Ordering::SeqCst) {
                0 => response(429, vec![("Retry-After".into(), "0.2".into())]),
                _ => response(200, Vec::new()),
            })
        });
        assert_eq!(answer.unwrap().status_code, 200);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(started.elapsed() >= Duration::from_millis(200));
    }

    #[test]
    fn identical_requests_at_once_share_one_answer() {
        let calls = Arc::new(AtomicUsize::new(0));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let calls = calls.clone();
                std::thread::spawn(move || {
                    send_with(&request("https://shared.test/same"), |_| {
                        calls.fetch_add(1, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(150));
                        Ok(response(200, Vec::new()))
                    })
                })
            })
            .collect();
        for t in threads {
            assert_eq!(t.join().unwrap().unwrap().status_code, 200);
        }
        assert!(calls.load(Ordering::SeqCst) < 8);
    }

    #[test]
    fn a_host_never_has_more_than_its_budget_in_flight() {
        let now = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let threads: Vec<_> = (0..12)
            .map(|i| {
                let (now, peak) = (now.clone(), peak.clone());
                std::thread::spawn(move || {
                    send_with(&request(&format!("https://budget.test/{i}")), |_| {
                        let n = now.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(n, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(30));
                        now.fetch_sub(1, Ordering::SeqCst);
                        Ok(response(200, Vec::new()))
                    })
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap().unwrap();
        }
        assert!(peak.load(Ordering::SeqCst) <= PER_HOST);
    }
}
