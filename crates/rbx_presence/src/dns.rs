//! Whether a new quick-tunnel name exists yet, asked of its zone's own name
//! servers. A recursive resolver asked too early caches "no such host" for
//! the zone's negative TTL (60 s for trycloudflare.com): asking 1.1.1.1
//! once a second from the moment cloudflared printed the address kept it
//! answering "no" for the whole wait, seen live. An authoritative server
//! caches nothing, so asking it is free.

use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::Duration;

/// The name servers of `host`'s zone (`host` minus its first label), looked
/// up once over DNS-over-HTTPS: that answer exists, so nothing bad gets
/// cached. Empty when the lookup fails.
pub fn name_servers(host: &str) -> Vec<SocketAddr> {
    let Some((_, zone)) = host.split_once('.') else {
        return Vec::new();
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(5)))
        .build()
        .into();
    let names: Vec<String> = agent
        .get("https://cloudflare-dns.com/dns-query")
        .query("name", zone)
        .query("type", "NS")
        .header("Accept", "application/dns-json")
        .call()
        .ok()
        .and_then(|mut resp| resp.body_mut().read_to_string().ok())
        .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
        .and_then(|json| json.get("Answer")?.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|answer| answer.get("data")?.as_str().map(str::to_owned))
        .collect();
    names
        .iter()
        .filter_map(|name| (name.trim_end_matches('.'), 53).to_socket_addrs().ok())
        .flatten()
        .filter(SocketAddr::is_ipv4)
        .collect()
}

/// Asks `server` whether `host` has an A record: `None` when it gives no
/// usable answer within a second.
pub fn has_address(host: &str, server: SocketAddr) -> Option<bool> {
    let id = (crate::now_ms() & 0xffff) as u16;
    let socket = UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    socket.set_read_timeout(Some(Duration::from_secs(1))).ok()?;
    socket.send_to(&query(id, host), server).ok()?;
    let mut reply = [0u8; 512];
    let len = socket.recv(&mut reply).ok()?;
    answered(id, &reply[..len])
}

/// A recursion-free query for `host`'s A record.
fn query(id: u16, host: &str) -> Vec<u8> {
    let mut packet = Vec::with_capacity(18 + host.len());
    packet.extend_from_slice(&id.to_be_bytes());
    packet.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in host.split('.').filter(|l| !l.is_empty()) {
        packet.push(label.len() as u8);
        packet.extend_from_slice(label.as_bytes());
    }
    packet.extend_from_slice(&[0, 0, 1, 0, 1]);
    packet
}

/// Whether `reply` (to query `id`) holds an answer; `None` for anything
/// but a reply to it that succeeded or said "no such name".
fn answered(id: u16, reply: &[u8]) -> Option<bool> {
    if reply.len() < 12 || reply[..2] != id.to_be_bytes() || reply[2] & 0x80 == 0 {
        return None;
    }
    let answers = u16::from_be_bytes([reply[6], reply[7]]);
    match reply[3] & 0x0f {
        0 => Some(answers > 0),
        3 => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_names_the_host_label_by_label() {
        let packet = query(0xabcd, "a-b.example.com");
        assert_eq!(&packet[..4], &[0xab, 0xcd, 0, 0]);
        assert_eq!(
            &packet[12..],
            b"\x03a-b\x07example\x03com\x00\x00\x01\x00\x01"
        );
    }

    #[test]
    fn replies_read_as_found_missing_or_unusable() {
        let reply = |id: u16, flags: [u8; 2], answers: u16| {
            let mut r = id.to_be_bytes().to_vec();
            r.extend_from_slice(&flags);
            r.extend_from_slice(&[0, 1]);
            r.extend_from_slice(&answers.to_be_bytes());
            r.extend_from_slice(&[0; 4]);
            r
        };
        assert_eq!(answered(7, &reply(7, [0x84, 0x00], 2)), Some(true));
        assert_eq!(answered(7, &reply(7, [0x84, 0x03], 0)), Some(false));
        assert_eq!(answered(7, &reply(7, [0x84, 0x00], 0)), Some(false));
        assert_eq!(answered(7, &reply(7, [0x84, 0x02], 0)), None);
        assert_eq!(answered(7, &reply(8, [0x84, 0x00], 2)), None);
        assert_eq!(answered(7, &reply(7, [0x04, 0x00], 2)), None);
    }
}
