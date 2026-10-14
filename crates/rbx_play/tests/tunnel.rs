use std::path::PathBuf;

use rbx_play::tunnel::{scrape_url, TunnelProvider};

fn cloudflared() -> TunnelProvider {
    TunnelProvider::Cloudflared {
        binary: PathBuf::from("cloudflared"),
    }
}

#[test]
fn scrape_url_finds_cloudflared_url_in_log_line() {
    let line = "2026-10-14T10:00:00Z INF |  https://abc-def.trycloudflare.com  |";
    assert_eq!(
        scrape_url(line, &cloudflared().url_pattern()).as_deref(),
        Some("https://abc-def.trycloudflare.com")
    );
}

#[test]
fn scrape_url_returns_none_without_url() {
    let line = "2026-10-14T10:00:00Z INF Starting tunnel connection";
    assert_eq!(scrape_url(line, &cloudflared().url_pattern()), None);
}

#[test]
fn scrape_url_returns_none_for_invalid_regex() {
    let line = "2026-10-14T10:00:00Z INF |  https://abc-def.trycloudflare.com  |";
    assert_eq!(scrape_url(line, "(["), None);
}

#[test]
fn cloudflared_command_line_points_at_loopback_port() {
    let (program, args) = cloudflared().command_line(4242);
    assert_eq!(program, PathBuf::from("cloudflared"));
    assert_eq!(
        args,
        vec![
            "tunnel".to_owned(),
            "--no-autoupdate".to_owned(),
            "--url".to_owned(),
            "http://127.0.0.1:4242".to_owned(),
        ]
    );
}

#[test]
fn custom_command_line_replaces_every_port_placeholder() {
    let provider = TunnelProvider::Custom {
        program: PathBuf::from("sh"),
        args: vec![
            "-c".to_owned(),
            "echo https://x{port}.example.test; echo {port}".to_owned(),
        ],
        url_pattern: "https://[a-z0-9]+\\.example\\.test".to_owned(),
    };
    let (program, args) = provider.command_line(7);
    assert_eq!(program, PathBuf::from("sh"));
    assert_eq!(
        args,
        vec![
            "-c".to_owned(),
            "echo https://x7.example.test; echo 7".to_owned(),
        ]
    );
}

#[cfg(unix)]
mod unix_start {
    use std::path::PathBuf;
    use std::process::Command;
    use std::thread;
    use std::time::{Duration, Instant};

    use rbx_play::tunnel::{TunnelError, TunnelProvider};

    fn echo_provider(script: &str) -> TunnelProvider {
        TunnelProvider::Custom {
            program: PathBuf::from("sh"),
            args: vec!["-c".to_owned(), script.to_owned()],
            url_pattern: "https://[a-z0-9]+\\.example\\.test".to_owned(),
        }
    }

    fn process_alive(pid: u32) -> bool {
        Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    #[test]
    fn start_returns_scraped_public_url() {
        let provider = TunnelProvider::Custom {
            program: PathBuf::from("sh"),
            args: vec![
                "-c".to_owned(),
                "echo starting; echo url=https://x{port}.example.test >&2; sleep 30".to_owned(),
            ],
            url_pattern: "https://[a-z0-9]+\\.example\\.test".to_owned(),
        };
        let tunnel = provider
            .start(7, Duration::from_secs(5))
            .expect("tunnel should start");
        assert_eq!(tunnel.public_url, "https://x7.example.test");
    }

    #[test]
    fn start_times_out_when_no_url_is_printed() {
        let provider = echo_provider("echo nothing to see; sleep 30");
        let result = provider.start(7, Duration::from_millis(300));
        assert!(
            matches!(result, Err(TunnelError::Timeout)),
            "expected Timeout, got {:?}",
            result.err()
        );
    }

    #[test]
    fn start_reports_exit_with_last_output() {
        let provider = echo_provider("echo oops; exit 3");
        match provider.start(7, Duration::from_secs(5)) {
            Err(TunnelError::Exited(text)) => {
                assert!(text.contains("oops"), "exit text was {text:?}");
            }
            other => panic!("expected Exited, got {:?}", other.err()),
        }
    }

    #[test]
    fn start_reports_spawn_error_for_missing_program() {
        let provider = TunnelProvider::Custom {
            program: PathBuf::from("/nonexistent/rbx-play-no-such-tunnel-binary"),
            args: Vec::new(),
            url_pattern: "https://[a-z0-9]+\\.example\\.test".to_owned(),
        };
        assert!(matches!(
            provider.start(7, Duration::from_secs(5)),
            Err(TunnelError::Spawn(_))
        ));
    }

    #[test]
    fn dropping_tunnel_kills_child() {
        let provider = echo_provider("echo https://x7.example.test; sleep 30");
        let tunnel = provider
            .start(7, Duration::from_secs(5))
            .expect("tunnel should start");
        let pid = tunnel.child.id();
        assert!(process_alive(pid), "child should be running before drop");

        drop(tunnel);

        let deadline = Instant::now() + Duration::from_secs(2);
        while process_alive(pid) {
            assert!(
                Instant::now() < deadline,
                "child {pid} still alive 2 s after drop"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn tunnel_survives_logging_after_the_url() {
        // cloudflared keeps logging once its URL is out; a closed pipe
        // would kill it with SIGPIPE on the next line.
        let mut tunnel = echo_provider(
            "echo https://abc.example.test; sleep 0.2; i=0; \
             while [ $i -lt 30 ]; do echo log line $i >&2 || exit 9; sleep 0.02; i=$((i+1)); done; sleep 30",
        )
        .start(1, Duration::from_secs(5))
        .expect("tunnel starts");
        thread::sleep(Duration::from_millis(800));
        // try_wait, not kill -0: an unreaped dead child is a zombie kill -0 still finds.
        assert!(
            matches!(tunnel.child.try_wait(), Ok(None)),
            "tunnel died after its URL"
        );
    }
}
