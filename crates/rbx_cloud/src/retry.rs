//! Retrying a request the service refused for a reason of its own rather
//! than a reason of ours.
//!
//! The Open Cloud asset endpoints answer 1000 requests a minute per key
//! *owner* (`x-ratelimit-limit: 1000, 1000;w=60` on
//! `apis.roblox.com/asset-delivery-api/v1/assetId/<id>`), shared across every
//! key that owner holds; past that they answer HTTP 429 with `retry-after: 5`.
//! Roblox's own guidance is to expect it: "Always ensure your application
//! handles HTTP 429 rate limit responses ... check the `retry-after` response
//! header as a guideline for when to retry. If there is no `retry-after`
//! response header present, implement an exponential backoff retry strategy."
//! (creator-docs `content/en-us/cloud/reference/rate-limits.md`, which also
//! warns that undocumented limits apply for DDoS protection.)
//!
//! Without this a place whose images are private — every one of them a keyed
//! request — leaves a scattering of them undecoded whenever a load crosses the
//! quota, and which ones varies run to run.

use std::time::Duration;

use crate::client::RawResponse;
use crate::error::CloudError;

/// Attempts in total, the first included. Three retries at the backoff below
/// cover a `retry-after: 5` twice over, which is what a load that merely
/// crossed the per-minute quota needs; anything still refusing after that is
/// an outage, and the caller's own "ask again later" path is the better
/// answer than a worker thread asleep for a minute.
const ATTEMPTS: u32 = 4;

/// Wait before the first retry; doubled for each one after it (0.5s, 1s, 2s).
const FIRST_BACKOFF: Duration = Duration::from_millis(500);

/// A `Retry-After` longer than this is not worth holding an asset worker for.
const MAX_WAIT: Duration = Duration::from_secs(8);

/// Runs `attempt` until it gives an answer worth keeping, or until
/// [`ATTEMPTS`] is spent.
///
/// Only for requests that are safe to repeat: this crate applies it to GETs,
/// never to the publish POSTs, where a repeat is a second version.
pub(crate) fn idempotent(
    what: &str,
    attempt: impl FnMut() -> Result<RawResponse, CloudError>,
) -> Result<RawResponse, CloudError> {
    drive(what, attempt, std::thread::sleep)
}

/// [`idempotent`] with the sleeping split out, so a test can run the whole
/// policy in no time at all and check what it would have waited.
fn drive(
    what: &str,
    mut attempt: impl FnMut() -> Result<RawResponse, CloudError>,
    mut sleep: impl FnMut(Duration),
) -> Result<RawResponse, CloudError> {
    for retry in 1..ATTEMPTS {
        let outcome = attempt();
        match backoff(&outcome, retry) {
            Some(delay) => sleep(delay),
            None => return outcome,
        }
    }

    let last = attempt();
    // One line, on the way out: a retried request that succeeds is not news,
    // and a per-attempt line would bury the one that matters under five.
    if let Some(reason) = refusal(&last) {
        eprintln!("warning: {what} gave up after {ATTEMPTS} attempts ({reason})");
    }
    last
}

/// How long to wait before retry number `retry` (1 for the first), or `None`
/// if this outcome is the answer and asking again cannot improve it.
fn backoff(outcome: &Result<RawResponse, CloudError>, retry: u32) -> Option<Duration> {
    let exponential = FIRST_BACKOFF * 2u32.pow(retry - 1);
    let delay = match outcome {
        // A connection that dropped mid-burst says nothing about the asset.
        Err(CloudError::Transport(_)) => exponential,
        // A rate limit, or the load balancer having a moment. `error_for_status`
        // has not run yet, so these are still raw statuses here; the service's
        // own `retry-after` beats our guess, but only upwards — a second refusal
        // must back off further, not repeat the same wait. A 500 is deliberately
        // absent: it is as likely to be this exact request every time.
        Ok(response) if matches!(response.status, 429 | 502..=504) => retry_after(response)
            .unwrap_or(exponential)
            .max(exponential),
        _ => return None,
    };
    (delay <= MAX_WAIT).then_some(delay)
}

fn retry_after(response: &RawResponse) -> Option<Duration> {
    response
        .headers
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
}

/// What to say about a last attempt that still failed, or `None` if it did
/// not — the status alone, never the body, which can hold a signed CDN URL.
fn refusal(outcome: &Result<RawResponse, CloudError>) -> Option<String> {
    match outcome {
        Err(err) => Some(err.to_string()),
        Ok(response) if !(200..400).contains(&response.status) => {
            Some(format!("HTTP {}", response.status))
        }
        Ok(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    fn response(status: u16, retry_after: Option<&str>) -> RawResponse {
        let mut headers = ureq::http::HeaderMap::new();
        if let Some(value) = retry_after {
            headers.insert("retry-after", value.parse().unwrap());
        }
        RawResponse {
            status,
            headers,
            body: Vec::new(),
        }
    }

    /// Answers the given outcomes in order, recording every wait the policy
    /// asked for instead of taking it.
    fn run(
        outcomes: Vec<Result<RawResponse, CloudError>>,
    ) -> (Result<RawResponse, CloudError>, Vec<Duration>) {
        let queue = RefCell::new(outcomes.into_iter());
        let waited = RefCell::new(Vec::new());
        let result = drive(
            "test request",
            || queue.borrow_mut().next().expect("one attempt too many"),
            |delay| waited.borrow_mut().push(delay),
        );
        (result, waited.into_inner())
    }

    #[test]
    fn a_rate_limit_is_retried_until_it_clears() {
        let (result, waited) = run(vec![
            Ok(response(429, None)),
            Ok(response(429, None)),
            Ok(response(200, None)),
        ]);

        assert_eq!(result.unwrap().status, 200);
        assert_eq!(
            waited,
            [Duration::from_millis(500), Duration::from_millis(1000)]
        );
    }

    #[test]
    fn retry_after_is_honoured_over_the_backoff() {
        let (result, waited) = run(vec![Ok(response(429, Some("5"))), Ok(response(200, None))]);

        assert_eq!(result.unwrap().status, 200);
        assert_eq!(waited, [Duration::from_secs(5)]);
    }

    // The header is a floor, not a ceiling: a second rate limit after a
    // `retry-after: 1` must still back off further rather than hammer.
    #[test]
    fn a_short_retry_after_never_shortens_the_backoff() {
        let (_, waited) = run(vec![
            Ok(response(429, Some("0"))),
            Ok(response(429, Some("0"))),
            Ok(response(429, Some("0"))),
            Ok(response(429, Some("0"))),
        ]);

        assert_eq!(
            waited,
            [
                Duration::from_millis(500),
                Duration::from_millis(1000),
                Duration::from_millis(2000)
            ]
        );
    }

    #[test]
    fn a_retry_after_we_will_not_wait_out_is_surfaced_at_once() {
        let (result, waited) = run(vec![Ok(response(429, Some("90")))]);

        assert_eq!(result.unwrap().status, 429);
        assert!(waited.is_empty(), "{waited:?}");
    }

    #[test]
    fn a_gateway_error_is_retried_but_a_500_is_not() {
        let (result, waited) = run(vec![Ok(response(503, None)), Ok(response(200, None))]);
        assert_eq!(result.unwrap().status, 200);
        assert_eq!(waited, [Duration::from_millis(500)]);

        let (result, waited) = run(vec![Ok(response(500, None))]);
        assert_eq!(result.unwrap().status, 500);
        assert!(waited.is_empty());
    }

    #[test]
    fn a_dropped_connection_is_retried() {
        let (result, waited) = run(vec![
            Err(CloudError::Transport("connection reset".to_string())),
            Ok(response(200, None)),
        ]);

        assert_eq!(result.unwrap().status, 200);
        assert_eq!(waited, [Duration::from_millis(500)]);
    }

    // The statuses the asset path reads itself: a 404 is the answer, and a
    // 401 is what sends `asset()` to the keyed route. Retrying either would
    // cost three round trips and change nothing.
    #[test]
    fn an_answer_about_the_asset_is_never_retried() {
        for status in [200, 302, 401, 403, 404, 409, 410] {
            let (result, waited) = run(vec![Ok(response(status, None))]);
            assert_eq!(result.unwrap().status, status);
            assert!(waited.is_empty(), "{status} was retried");
        }
    }

    #[test]
    fn a_rate_limit_that_never_clears_stops_after_the_attempt_cap() {
        // Exactly ATTEMPTS outcomes: `run` panics on one attempt too many.
        let (result, waited) = run(vec![
            Ok(response(429, None)),
            Ok(response(429, None)),
            Ok(response(429, None)),
            Ok(response(429, None)),
        ]);

        assert_eq!(result.unwrap().status, 429);
        assert_eq!(waited.len() as u32, ATTEMPTS - 1);
    }

    #[test]
    fn only_a_failed_last_attempt_is_worth_a_line() {
        assert!(refusal(&Ok(response(200, None))).is_none());
        assert!(refusal(&Ok(response(302, None))).is_none());
        assert_eq!(refusal(&Ok(response(429, None))).unwrap(), "HTTP 429");
        assert!(refusal(&Err(CloudError::Transport("reset".to_string())))
            .unwrap()
            .contains("reset"));
    }
}
