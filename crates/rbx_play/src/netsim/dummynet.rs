//! macOS shaping through pf's dummynet.
//!
//! Rules live in the anchor `com.apple/rbxnative`, feeding pipes 4201-4204
//! (inbound) and 4211-4214 (outbound). Apple's dummynet has no jitter, so
//! four pipes are configured with delays spread evenly over
//! `[latency, latency + jitter]` and each packet picks one by cascading pf
//! `probability` rules; if `pfctl` rejects `probability`, a single pipe at the
//! mean delay is used instead. The root helper (`helper.sh`) is started once
//! through osascript so the user sees a single password prompt.

#[cfg(target_os = "macos")]
mod mac;

#[cfg(target_os = "macos")]
pub use mac::Shaper;

pub(crate) const HELPER: &str = include_str!("dummynet/helper.sh");

/// Local UDP ports from `lsof -nP -a -p PID -iUDP -F n` output.
pub(crate) fn parse_lsof_udp(output: &str) -> Vec<u16> {
    let mut ports: Vec<u16> = output
        .lines()
        .filter_map(|l| l.strip_prefix('n'))
        .filter_map(|name| {
            let local = name.split_once("->").map_or(name, |(l, _)| l);
            let (_, port) = local.rsplit_once(':')?;
            port.parse::<u16>().ok().filter(|p| *p != 0)
        })
        .collect();
    ports.sort_unstable();
    ports.dedup();
    ports
}

/// Arguments for `osascript` that run the helper as root. The helper is
/// copied to a root-owned temp file first so a user-writable original can't
/// be swapped after the prompt.
pub(crate) fn osascript_args(dir: &str, parent_pid: u32) -> Option<[String; 2]> {
    let ok = !dir.is_empty()
        && dir
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'));
    if !ok {
        return None;
    }
    let cmd = format!(
        "f=$(/usr/bin/mktemp /tmp/rbxnative-netsim.XXXXXX) && /bin/cat {dir}/helper.sh > $f && (/bin/sh $f {dir} {parent_pid}; /bin/rm -f $f) >/dev/null 2>&1 &"
    );
    let script = format!(
        "do shell script \"{cmd}\" with administrator privileges with prompt \"rbx-native wants to simulate network conditions for Play.\""
    );
    Some(["-e".to_string(), script])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsof_samples() {
        let out = "p123\nf5\nn*:61234\nf6\nn192.168.1.5:61234->128.116.1.2:50000\n\
                   f7\nn[::1]:5353\nf8\nn[fe80::1%en0]:61240->[2001:db8::1]:443\nf9\nn*:*\n";
        assert_eq!(parse_lsof_udp(out), vec![5353, 61234, 61240]);
        assert!(parse_lsof_udp("p1\nf2\nn*:*\n").is_empty());
    }

    #[test]
    fn osascript_paths() {
        let a = osascript_args("/var/folders/ab/c_d-1/T/.tmpXyZ", 77).unwrap();
        assert_eq!(a[0], "-e");
        assert!(a[1].contains("/bin/sh $f /var/folders/ab/c_d-1/T/.tmpXyZ 77;"));
        assert!(a[1].contains("with administrator privileges"));
        assert!(osascript_args("/tmp/a b", 1).is_none());
        assert!(osascript_args("/tmp/a\"b", 1).is_none());
        assert!(osascript_args("/tmp/a'b", 1).is_none());
        assert!(osascript_args("", 1).is_none());
    }

    #[cfg(target_os = "linux")]
    mod helper_run {
        use super::super::HELPER;
        use std::fs::{self, OpenOptions};
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::fs::PermissionsExt;
        use std::process::{Command, Stdio};

        const PFCTL: &str = r#"#!/bin/sh
echo "pfctl $*" >> "$STUB_LOG"
case "$*" in
  *-E*) echo "Token : 42"; exit 0 ;;
  "-n "*) cat > /dev/null; exit "${STUB_PFCTL_N:-0}" ;;
  *"-f -"*) cat >> "$STUB_LOG" ;;
esac
exit 0
"#;
        const DNCTL: &str = "#!/bin/sh\necho \"dnctl $*\" >> \"$STUB_LOG\"\nexit 0\n";

        fn run(pfctl_n: &str) -> (Vec<String>, String) {
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path();
            let stubs = dir.join("stubs");
            fs::create_dir(&stubs).unwrap();
            for (name, body) in [("pfctl", PFCTL), ("dnctl", DNCTL)] {
                let p = stubs.join(name);
                fs::write(&p, body).unwrap();
                fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
            }
            fs::write(dir.join("helper.sh"), HELPER).unwrap();
            for f in ["cmd", "status"] {
                assert!(Command::new("mkfifo")
                    .arg(dir.join(f))
                    .status()
                    .unwrap()
                    .success());
            }
            let mut cmd = OpenOptions::new()
                .read(true)
                .write(true)
                .open(dir.join("cmd"))
                .unwrap();
            let status = OpenOptions::new()
                .read(true)
                .write(true)
                .open(dir.join("status"))
                .unwrap();
            let log = dir.join("log");
            let path = format!(
                "{}:{}",
                stubs.display(),
                std::env::var("PATH").unwrap_or_default()
            );
            let mut child = Command::new("sh")
                .arg(dir.join("helper.sh"))
                .arg(dir)
                .arg(std::process::id().to_string())
                .env("PATH", path)
                .env("STUB_LOG", &log)
                .env("STUB_PFCTL_N", pfctl_n)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let mut rd = BufReader::new(status);
            let mut next = || {
                let mut s = String::new();
                rd.read_line(&mut s).unwrap();
                s.trim().to_string()
            };
            let mut replies = Vec::new();
            if pfctl_n != "0" {
                replies.push(next());
            }
            replies.push(next());
            for line in [
                "out 30000 12000 2000",
                "ports 61234 61240",
                "in 0 0 0",
                "out 1 2",
                "stop",
            ] {
                writeln!(cmd, "{line}").unwrap();
                if line != "stop" {
                    replies.push(next());
                }
            }
            child.wait().unwrap();
            (replies, fs::read_to_string(&log).unwrap())
        }

        #[test]
        fn full_cascade() {
            let (r, log) = run("0");
            assert!(r[0].starts_with("ready "), "{r:?}");
            assert_eq!(
                &r[1..],
                ["ok out", "ok ports", "ok in", "error out malformed"]
            );
            for (pipe, d) in [(4211, 32), (4212, 35), (4213, 38), (4214, 41)] {
                assert!(
                    log.contains(&format!("dnctl pipe {pipe} config delay {d} plr 0.002000")),
                    "{log}"
                );
            }
            for (pipe, pct) in [
                (4211, "probability 25% "),
                (4212, "probability 33% "),
                (4213, "probability 50% "),
                (4214, ""),
            ] {
                let rule = format!(
                    "dummynet out quick proto udp from any port {{ 61234 61240 }} to any {pct}pipe {pipe}"
                );
                assert!(log.contains(&rule), "{rule}\n{log}");
            }
            assert!(log.contains("dummynet in quick proto udp from any to any port { 61234 61240 } probability 25% pipe 4201"));
            for n in [4201, 4202, 4203, 4204, 4211, 4212, 4213, 4214] {
                assert!(log.contains(&format!("dnctl pipe delete {n}")));
            }
            assert!(log.contains("pfctl -X 42"));
        }

        #[test]
        fn single_pipe_without_jitter_support() {
            let (r, log) = run("1");
            assert_eq!(r[0], "nojitter");
            assert!(r[1].starts_with("ready "));
            assert_eq!(
                &r[2..],
                ["ok out", "ok ports", "ok in", "error out malformed"]
            );
            assert!(log.contains("dnctl pipe 4211 config delay 36 plr 0.002000"));
            assert!(log.contains("to any pipe 4211"));
            assert!(!log.contains("pipe 4212\n"));
            assert!(!log.contains("probability 25%"));
        }
    }
}
