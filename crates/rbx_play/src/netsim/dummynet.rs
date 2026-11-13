//! macOS shaping through pf's dummynet.
//!
//! Rules live in the anchor `com.apple/rbxnative`, feeding pipes 4201-4204
//! (inbound) and 4211-4214 (outbound). Apple's dummynet has no jitter, so
//! four pipes are configured with delays spread evenly over
//! `[latency, latency + jitter]` and each packet picks one by cascading pf
//! `probability` rules; if `pfctl` rejects `probability`, a single pipe at the
//! mean delay is used instead.
//!
//! pf needs root. The root helper (`helper.sh`) is installed once, with one
//! administrator prompt, as a launchd daemon behind a Unix socket owned by
//! the installing user (mode 0600: no other account can reach it); after
//! that Play shapes without asking. Settings › Account › Play › Network helper removes it.

#[cfg(target_os = "macos")]
mod mac;

#[cfg(target_os = "macos")]
pub use mac::{helper_installed, remove_helper, Shaper};

pub(crate) const HELPER: &str = include_str!("dummynet/helper.sh");

/// The daemon's launchd label, also the stem of its installed files.
pub(crate) const LABEL: &str = "io.github.chteau.rbx-native.netsim";
pub(crate) const HELPER_PATH: &str =
    "/Library/PrivilegedHelperTools/io.github.chteau.rbx-native.netsim";
pub(crate) const PLIST_PATH: &str =
    "/Library/LaunchDaemons/io.github.chteau.rbx-native.netsim.plist";
pub(crate) const SOCKET: &str = "/var/run/io.github.chteau.rbx-native.netsim.sock";

/// The `VERSION=` the bundled helper declares; an installed helper saying
/// another one in its `hello` line is replaced.
pub(crate) fn helper_version() -> u32 {
    HELPER
        .lines()
        .find_map(|l| l.strip_prefix("VERSION="))
        .and_then(|v| v.trim().parse().ok())
        .expect("helper.sh declares VERSION=<n>")
}

/// The daemon's launchd property list: one helper run per connection
/// (`inetdCompatibility`), on a socket only `uid` (and root) can open.
pub(crate) fn plist(uid: u32) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		<string>/bin/sh</string>
		<string>{HELPER_PATH}</string>
	</array>
	<key>inetdCompatibility</key>
	<dict>
		<key>Wait</key>
		<false/>
	</dict>
	<key>Sockets</key>
	<dict>
		<key>Listener</key>
		<dict>
			<key>SockPathName</key>
			<string>{SOCKET}</string>
			<key>SockPathMode</key>
			<integer>384</integer>
			<key>SockPathOwner</key>
			<integer>{uid}</integer>
			<key>SockType</key>
			<string>stream</string>
		</dict>
	</dict>
</dict>
</plist>
"#
    )
}

fn safe_path(dir: &str) -> bool {
    !dir.is_empty()
        && dir
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'))
}

fn is_sha256(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The root shell command that installs `DIR/helper.sh` and `DIR/daemon.plist`
/// and starts the daemon. The files are checked against the hashes taken
/// before the prompt once they sit in root-owned places, so swapping the
/// user-writable originals in the meantime gains nothing.
pub(crate) fn install_command(dir: &str, helper_sha: &str, plist_sha: &str) -> Option<String> {
    if !safe_path(dir) || !is_sha256(helper_sha) || !is_sha256(plist_sha) {
        return None;
    }
    Some(format!(
        "set -e; /bin/launchctl bootout system/{LABEL} >/dev/null 2>&1 || true; \
/bin/mkdir -p /Library/PrivilegedHelperTools; \
/usr/bin/install -m 755 -o root -g wheel {dir}/helper.sh {HELPER_PATH}; \
/usr/bin/install -m 644 -o root -g wheel {dir}/daemon.plist {PLIST_PATH}; \
if [ $(/usr/bin/shasum -a 256 {HELPER_PATH} | /usr/bin/cut -c1-64) != {helper_sha} ] || \
[ $(/usr/bin/shasum -a 256 {PLIST_PATH} | /usr/bin/cut -c1-64) != {plist_sha} ]; then \
/bin/rm -f {HELPER_PATH} {PLIST_PATH}; exit 1; fi; \
/bin/rm -f {SOCKET}; \
/bin/launchctl bootstrap system {PLIST_PATH} || {{ /bin/sleep 1; /bin/launchctl bootstrap system {PLIST_PATH}; }}"
    ))
}

/// The root shell command that stops and deletes the daemon.
pub(crate) fn uninstall_command() -> String {
    format!(
        "/bin/launchctl bootout system/{LABEL} >/dev/null 2>&1; /bin/rm -f {PLIST_PATH} {HELPER_PATH} {SOCKET}"
    )
}

/// `osascript` arguments running `command` as root behind macOS's
/// administrator prompt. `command` must hold no double quote or backslash
/// (the commands above don't).
pub(crate) fn osascript_args(command: &str, prompt: &str) -> [String; 2] {
    [
        "-e".to_string(),
        format!(
            "do shell script \"{command}\" with administrator privileges with prompt \"{prompt}\""
        ),
    ]
}

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

    const SHA_A: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const SHA_B: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

    #[test]
    fn install_command_checks_its_inputs() {
        let c = install_command("/var/folders/ab/c_d-1/T/.tmpXyZ", SHA_A, SHA_B).unwrap();
        assert!(c.contains("/var/folders/ab/c_d-1/T/.tmpXyZ/helper.sh /Library/PrivilegedHelperTools/io.github.chteau.rbx-native.netsim;"));
        assert!(c.contains(&format!("!= {SHA_A} ]")));
        assert!(c.contains(&format!("!= {SHA_B} ];")));
        assert!(c.contains("/bin/launchctl bootstrap system /Library/LaunchDaemons/io.github.chteau.rbx-native.netsim.plist"));
        assert!(
            !c.contains('"') && !c.contains('\\') && !c.contains('\n'),
            "{c}"
        );
        assert!(install_command("/tmp/a b", SHA_A, SHA_B).is_none());
        assert!(install_command("/tmp/a\"b", SHA_A, SHA_B).is_none());
        assert!(install_command("", SHA_A, SHA_B).is_none());
        assert!(install_command("/tmp/x", "abc", SHA_B).is_none());
        assert!(install_command("/tmp/x", SHA_A, &SHA_B.replace('f', "g")).is_none());
    }

    #[test]
    fn uninstall_and_prompt() {
        let u = uninstall_command();
        assert!(u.starts_with("/bin/launchctl bootout system/io.github.chteau.rbx-native.netsim "));
        assert!(u.contains(SOCKET) && u.contains(PLIST_PATH) && u.contains(HELPER_PATH));
        let a = osascript_args(&u, "Remove it?");
        assert_eq!(a[0], "-e");
        assert!(a[1].starts_with("do shell script \"/bin/launchctl"));
        assert!(a[1].ends_with("with administrator privileges with prompt \"Remove it?\""));
    }

    #[test]
    fn plist_is_owned_by_the_user() {
        let p = plist(501);
        assert!(p.contains("<key>SockPathOwner</key>\n\t\t\t<integer>501</integer>"));
        assert!(p.contains("<integer>384</integer>")); // 0600
        assert!(p.contains(&format!("<string>{SOCKET}</string>")));
        assert!(p.contains(&format!("<string>{HELPER_PATH}</string>")));
        assert!(p.contains("<key>Wait</key>\n\t\t<false/>"));
    }

    #[test]
    fn helper_declares_a_version() {
        assert!(helper_version() >= 1);
    }

    #[cfg(target_os = "linux")]
    mod helper_run {
        use super::super::HELPER;
        use std::fs;
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
            let log = dir.join("log");
            let path = format!(
                "{}:{}",
                stubs.display(),
                std::env::var("PATH").unwrap_or_default()
            );
            // As launchd runs it: the connection is stdin and stdout.
            let mut child = Command::new("sh")
                .arg(dir.join("helper.sh"))
                .env("PATH", path)
                .env("STUB_LOG", &log)
                .env("STUB_PFCTL_N", pfctl_n)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let mut cmd = child.stdin.take().unwrap();
            let mut rd = BufReader::new(child.stdout.take().unwrap());
            let mut next = || {
                let mut s = String::new();
                rd.read_line(&mut s).unwrap();
                s.trim().to_string()
            };
            let mut replies = vec![next()];
            if pfctl_n != "0" {
                replies.push(next());
            }
            replies.push(next());
            for line in [
                "out 30000 12000 2000",
                "ports 61234 61240",
                "in 0 0 0",
                "out 1 2",
            ] {
                writeln!(cmd, "{line}").unwrap();
                replies.push(next());
            }
            // A closed connection ends the session like `stop` does.
            drop(cmd);
            child.wait().unwrap();
            (replies, fs::read_to_string(&log).unwrap())
        }

        #[test]
        fn full_cascade() {
            let (r, log) = run("0");
            assert_eq!(r[0], format!("hello {}", super::super::helper_version()));
            assert!(r[1].starts_with("ready "), "{r:?}");
            assert_eq!(
                &r[2..],
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
            assert!(r[0].starts_with("hello "));
            assert_eq!(r[1], "nojitter");
            assert!(r[2].starts_with("ready "));
            assert_eq!(
                &r[3..],
                ["ok out", "ok ports", "ok in", "error out malformed"]
            );
            assert!(log.contains("dnctl pipe 4211 config delay 36 plr 0.002000"));
            assert!(log.contains("to any pipe 4211"));
            assert!(!log.contains("pipe 4212\n"));
            assert!(!log.contains("probability 25%"));
        }
    }
}
