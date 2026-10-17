//! The shaper against real traffic: a UDP echo server on the host, pinged
//! from inside the shaped namespace through slirp4netns's host alias
//! (10.0.2.2), with the round trip measured before and after `apply`.
//!
//! Needs `slirp4netns` (`$RBX_SLIRP4NETNS`, else on `PATH`) and a kernel
//! that lets unprivileged user namespaces shape their network; skips with
//! the reason otherwise. The pinging runs as this same test binary, started
//! inside the namespace with `RBX_NETSIM_ECHO_CLIENT` set.
#![cfg(target_os = "linux")]

use std::net::{TcpListener, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use rbx_play::netsim::{probe, NetConditions, OneWay, Shaper};

const CLIENT: &str = "RBX_NETSIM_ECHO_CLIENT";
const PINGS: usize = 8;

/// Inside the namespace: prints `udp <min ms> <max ms>` over `PINGS` echoes
/// and `tcp <ms>` for one connect. Does nothing in a normal test run.
#[test]
fn echo_client() {
    let Ok(target) = std::env::var(CLIENT) else {
        return;
    };
    let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let (mut min, mut max) = (f64::MAX, 0.0f64);
    for i in 0..PINGS {
        let start = Instant::now();
        socket.send_to(&[i as u8], &target).unwrap();
        let mut buf = [0u8; 16];
        loop {
            let (n, _) = socket.recv_from(&mut buf).expect("echo answered");
            if n == 1 && buf[0] == i as u8 {
                break;
            }
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        (min, max) = (min.min(ms), max.max(ms));
    }
    println!("udp {min:.1} {max:.1}");
    let start = Instant::now();
    TcpStream::connect(std::env::var("RBX_NETSIM_TCP").unwrap()).unwrap();
    println!("tcp {:.1}", start.elapsed().as_secs_f64() * 1000.0);
}

/// `(udp min, udp max, tcp connect)` in ms, measured from the namespace.
fn measure(shaper: &Shaper, udp_port: u16, tcp_port: u16) -> (f64, f64, f64) {
    let wrapper = shaper.wrapper();
    let output = Command::new(&wrapper[0])
        .args(&wrapper[1..])
        .arg(std::env::current_exe().unwrap())
        .args(["echo_client", "--exact", "--nocapture", "--test-threads=1"])
        .env(CLIENT, format!("10.0.2.2:{udp_port}"))
        .env("RBX_NETSIM_TCP", format!("10.0.2.2:{tcp_port}"))
        .output()
        .unwrap();
    // libtest may print a test's output on either stream, after its name.
    let text = String::from_utf8_lossy(&output.stdout) + String::from_utf8_lossy(&output.stderr);
    let field = |prefix: &str| -> Vec<f64> {
        text.lines()
            .find_map(|line| line.split_once(prefix).map(|(_, rest)| rest))
            .unwrap_or_else(|| panic!("no `{prefix}` in {text}"))
            .split(' ')
            .map(|n| n.parse().unwrap())
            .collect()
    };
    let (udp, tcp) = (field("udp "), field("tcp ")[0]);
    eprintln!("udp {}..{} ms, tcp connect {tcp} ms", udp[0], udp[1]);
    (udp[0], udp[1], tcp)
}

fn slirp4netns() -> Option<PathBuf> {
    std::env::var_os("RBX_SLIRP4NETNS")
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from("slirp4netns")))
}

#[test]
fn applied_conditions_delay_udp_both_ways_and_leave_tcp_alone() {
    if std::env::var_os(CLIENT).is_some() {
        return;
    }
    let slirp = slirp4netns().unwrap();
    if let Err(why) = probe(&slirp) {
        eprintln!("skipped: {why}");
        return;
    }
    let echo = UdpSocket::bind("127.0.0.1:0").unwrap();
    let udp_port = echo.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut buf = [0u8; 64];
        while let Ok((n, from)) = echo.recv_from(&mut buf) {
            let _ = echo.send_to(&buf[..n], from);
        }
    });
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let tcp_port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || for _ in listener.incoming() {});

    let mut shaper = Shaper::start(&slirp, true).expect("shaper starts");
    let (min, _, _) = measure(&shaper, udp_port, tcp_port);
    assert!(min < 30.0, "unshaped round trip {min} ms");

    let mut conditions = NetConditions {
        outbound: OneWay::new(100.0, 0.0, 0.0),
        ..Default::default()
    };
    shaper.apply(&conditions).unwrap();
    let (min, max, _) = measure(&shaper, udp_port, tcp_port);
    assert!(
        (100.0..160.0).contains(&min) && max < 160.0,
        "{min}..{max} ms with outbound 100"
    );

    conditions.inbound = OneWay::new(200.0, 0.0, 0.0);
    shaper.apply(&conditions).unwrap();
    let (min, max, tcp) = measure(&shaper, udp_port, tcp_port);
    assert!(
        (300.0..360.0).contains(&min) && max < 360.0,
        "{min}..{max} ms with 100 + 200"
    );
    assert!(
        tcp < 50.0,
        "TCP connect took {tcp} ms; TCP must not be shaped"
    );

    // Jitter only ever adds: 50 ms + up to 40 ms, with nothing inbound.
    shaper
        .apply(&NetConditions {
            outbound: OneWay::new(50.0, 40.0, 0.0),
            inbound: OneWay::default(),
        })
        .unwrap();
    let (min, max, _) = measure(&shaper, udp_port, tcp_port);
    assert!(
        min >= 50.0 && max < 120.0,
        "{min}..{max} ms with 50 + 40 jitter"
    );
}
