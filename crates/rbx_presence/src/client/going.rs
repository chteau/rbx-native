//! A new host going public: its tunnel, the wait until that answers, then
//! the announcement. That takes from a few seconds to most of a minute, so
//! it runs on its own thread while the host's own client already talks to
//! the room on the loopback port, where Studio on this computer finds it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use super::{reachable, Hosting, Tunnel, TunnelRefused};
use crate::announce;

/// The tunnel and the address announced through it.
pub(super) type Public = (String, Box<dyn Tunnel>);

/// The thread taking a host public; dropping it calls the thread off.
pub(super) struct Going {
    done: Receiver<Result<Public, TunnelRefused>>,
    stop: Arc<AtomicBool>,
}

/// A tunnel that lets the thread that opened it end once it's dropped.
struct Held {
    tunnel: Box<dyn Tunnel>,
    _release: mpsc::Sender<()>,
}

impl Tunnel for Held {
    fn alive(&mut self) -> bool {
        self.tunnel.alive()
    }
}

/// Where the address is announced.
pub(super) struct Topic {
    pub(super) ntfy: String,
    pub(super) key: String,
    pub(super) room: String,
}

impl Going {
    /// `id`: the host's, announced with its address (see [`announce`]).
    pub(super) fn start(hosting: &Hosting, port: u16, id: String, topic: Topic) -> Going {
        let (send, done) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (open, called_off) = (hosting.0.clone(), stop.clone());
        thread::spawn(move || {
            let wanted = || !called_off.load(Ordering::Relaxed);
            // The tunnel's process ends with the thread that started it
            // (PR_SET_PDEATHSIG on Linux), so this thread lives as long as
            // the tunnel does.
            let (held, released) = mpsc::channel::<()>();
            let went = go_public(open(port), &id, &topic, &wanted).map(|(public, tunnel)| {
                let tunnel: Box<dyn Tunnel> = Box::new(Held {
                    tunnel,
                    _release: held,
                });
                (public, tunnel)
            });
            // Dropped while announcing: take the address back so nobody goes
            // to a tunnel that closes with this thread.
            if let Err(mpsc::SendError(Ok((public, _)))) = send.send(went) {
                goodbye(&topic, &public, true);
            }
            let _ = released.recv();
        });
        Going { done, stop }
    }

    /// The outcome once the thread is done.
    pub(super) fn poll(&self) -> Option<Result<Public, TunnelRefused>> {
        match self.done.try_recv() {
            Ok(went) => Some(went),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("going public stopped".into())),
        }
    }
}

impl Drop for Going {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn go_public(
    opened: Result<Public, TunnelRefused>,
    id: &str,
    topic: &Topic,
    wanted: &dyn Fn() -> bool,
) -> Result<Public, TunnelRefused> {
    let (public, tunnel) = opened?;
    if !announce::allowed_url(&public) {
        return Err(format!("the tunnel gave an unusable address: {public}").into());
    }
    // Collaborators look the address up the moment it is announced; one
    // asked before the record exists caches "no such host" for many
    // minutes. Announce only once public DNS has it, and once the tunnel
    // actually reaches this host: cloudflared prints its address before
    // Cloudflare's edge routes it, and a Studio whose first request gets
    // the edge's 530 backs off before trying again.
    let ready = wait_until_resolvable(&public, Duration::from_secs(45), wanted)
        && wait_until_answers(&public, Duration::from_secs(20), wanted);
    if !ready || !wanted() {
        return Err("presence stopped".into());
    }
    announce_now(topic, &public, id, wanted)?;
    Ok((public, tunnel))
}

/// Says the room has no host, unless (`check`) a newer host announced
/// since (this editor's own next worker, after a presence restart), which
/// a goodbye would hide.
pub(super) fn goodbye(topic: &Topic, public: &str, check: bool) {
    if check {
        if let Ok(Some(newest)) = announce::find(&topic.ntfy, &topic.key, &topic.room) {
            if newest.url != public {
                return;
            }
        }
    }
    let _ = announce::publish(&topic.ntfy, &topic.key, &topic.room, "", "");
}

/// Announces `public` as the room's host, trying again until it goes out
/// or `wanted` says to stop: a dropped post would otherwise cost a new
/// tunnel and its start-up before collaborators hear of this host, while
/// Studio on this computer is served from the loopback port all along.
/// Retries back off, and wait out a 429 for at least as long as ntfy asked:
/// posting on through its rate limit only keeps the budget empty.
fn announce_now(
    topic: &Topic,
    public: &str,
    id: &str,
    wanted: &dyn Fn() -> bool,
) -> Result<(), String> {
    let mut backoff = ANNOUNCE_RETRY_START;
    loop {
        let posted = announce::publish(&topic.ntfy, &topic.key, &topic.room, public, id);
        let Err(refused) = posted else {
            return Ok(());
        };
        let pause = refused.retry_after.unwrap_or_default().max(backoff);
        let until = Instant::now() + pause;
        while Instant::now() < until {
            if !wanted() {
                return Err(refused.why);
            }
            thread::sleep(Duration::from_millis(200));
        }
        backoff = (backoff * 2).min(ANNOUNCE_RETRY_MAX);
    }
}

/// Waits (at most `limit`) until `url`'s host has an address in public DNS,
/// asking its zone's own name servers (see [`crate::dns`]) so no resolver
/// caches an early "no such host". Loopback addresses return at once. Where
/// those servers can't be asked (UDP blocked), it waits a few seconds, about
/// as long as a new quick tunnel's name takes to appear. False when `wanted`
/// says to stop.
fn wait_until_resolvable(url: &str, limit: Duration, wanted: &dyn Fn() -> bool) -> bool {
    let Some(host) = url
        .strip_prefix("https://")
        .map(|rest| rest.split(['/', ':']).next().unwrap_or(rest))
    else {
        return true;
    };
    let servers = crate::dns::name_servers(host);
    let end = Instant::now() + limit;
    let blind = Instant::now() + UNASKED_WAIT;
    while Instant::now() < end {
        if !wanted() {
            return false;
        }
        let answers: Vec<Option<bool>> = servers
            .iter()
            .take(2)
            .map(|&server| crate::dns::has_address(host, server))
            .collect();
        if answers.contains(&Some(true)) {
            return true;
        }
        if answers.iter().all(Option::is_none) && Instant::now() >= blind {
            return true;
        }
        thread::sleep(Duration::from_millis(500));
    }
    true
}

/// How long [`wait_until_resolvable`] waits when no name server answers.
const UNASKED_WAIT: Duration = Duration::from_secs(5);

/// The first wait before posting an announcement again, doubling up to
/// [`ANNOUNCE_RETRY_MAX`].
const ANNOUNCE_RETRY_START: Duration = Duration::from_secs(1);
const ANNOUNCE_RETRY_MAX: Duration = Duration::from_secs(120);

/// Waits (at most `limit`) until this host's relay answers its health check
/// through `url`, its public address. False when `wanted` says to stop.
fn wait_until_answers(url: &str, limit: Duration, wanted: &dyn Fn() -> bool) -> bool {
    let end = Instant::now() + limit;
    while Instant::now() < end && !reachable(url) {
        if !wanted() {
            return false;
        }
        thread::sleep(Duration::from_millis(500));
    }
    true
}
