//! The play route (`GET /v1/rooms/<room>/play?pid=<16 hex>`, see
//! [`crate::play`]): forwards each sealed message to the end it names, or
//! to every other end for 0, with the sender's end in place of the
//! address. Nothing is kept or read.

use std::net::{Shutdown, TcpStream};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::time::Instant;

use tungstenite::Message;

use super::http::{refuse, Request};
use super::{lock, upgrade, Refusal, Shared};
use crate::play::{MESSAGE_BYTES, TICK};

/// Messages a second an end may send, and how many at once: a player's
/// input at 60 Hz plus a host's frames for several players, with room.
const RATE_PER_SECOND: f64 = 600.0;
const RATE_BURST: f64 = 1200.0;

/// One open play socket.
pub(super) struct Outbox {
    pid: u64,
    connection: u64,
    to: Sender<Vec<u8>>,
}

pub(super) fn serve(shared: &Arc<Shared>, mut stream: TcpStream, request: &Request) {
    let Some(pid) = request
        .query("pid")
        .and_then(|hex| u64::from_str_radix(hex, 16).ok())
        .filter(|pid| *pid != 0)
    else {
        return refuse(&mut stream, Refusal::new(400, "schema"));
    };
    let Some(mut ws) = upgrade(stream, request, TICK, Some(MESSAGE_BYTES)) else {
        return;
    };
    let connection = shared.next_outbox.fetch_add(1, Ordering::SeqCst);
    let (to, inbox) = mpsc::channel();
    {
        // A reconnecting end replaces its old socket.
        let mut outboxes = lock(&shared.play);
        outboxes.retain(|o| o.pid != pid);
        outboxes.push(Outbox {
            pid,
            connection,
            to,
        });
    }
    let mut tokens = RATE_BURST;
    let mut refilled = Instant::now();
    'serve: loop {
        if shared.stop.load(Ordering::SeqCst) {
            let _ = ws.close(None);
            break;
        }
        while let Ok(message) = inbox.try_recv() {
            if crate::tick::send(&mut ws, Message::binary(message)).is_err() {
                break 'serve;
            }
        }
        if crate::tick::flush(&mut ws).is_err() {
            break;
        }
        match ws.read() {
            Ok(Message::Binary(data)) if data.len() > 8 => {
                let now = Instant::now();
                tokens = (tokens + now.duration_since(refilled).as_secs_f64() * RATE_PER_SECOND)
                    .min(RATE_BURST);
                refilled = now;
                if tokens < 1.0 {
                    continue;
                }
                tokens -= 1.0;
                let target = u64::from_be_bytes(data[..8].try_into().unwrap_or_default());
                let mut forwarded = pid.to_be_bytes().to_vec();
                forwarded.extend_from_slice(&data[8..]);
                for outbox in lock(&shared.play).iter() {
                    let wanted = if target == 0 {
                        outbox.pid != pid
                    } else {
                        outbox.pid == target
                    };
                    if wanted {
                        let _ = outbox.to.send(forwarded.clone());
                    }
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(e) if crate::tick::nothing_read(&e, TICK) => {}
            Err(_) => break,
        }
    }
    lock(&shared.play).retain(|o| o.connection != connection);
    let _ = ws.get_mut().shutdown(Shutdown::Both);
}
