//! Windows network shaper built on WinDivert, entirely in user space.
//!
//! Packet capture needs the WinDivert driver, which needs administrator
//! rights. Rather than elevating the whole editor, the elevated helper is the
//! editor itself relaunched with [`HELPER_FLAG`] through `runas`; the parent
//! talks to it over a named pipe. The helper opens a NETWORK-layer handle
//! filtered on the Roblox client's local UDP ports, holds each packet in a
//! delay line, then reinjects it.
//!
//! WinDivert is LGPLv3, so it is not shipped: `WinDivert.dll` and
//! `WinDivert64.sys` are loaded at runtime from beside the executable.
//!
//! This file holds the pure parts (filter text, error wording, parsing); the
//! Windows-only modules below do the system calls.
#[cfg(windows)]
mod ffi;
#[cfg(windows)]
mod helper;
#[cfg(windows)]
mod parent;
#[cfg(windows)]
mod udp;

#[cfg(windows)]
pub use helper::run_helper;
#[cfg(windows)]
pub use parent::Shaper;

/// Command-line flag that turns the relaunched editor into the elevated helper.
#[cfg_attr(not(windows), allow(dead_code))]
pub const HELPER_FLAG: &str = "--netsim-helper";
/// Pipe name prefix; the parent appends a unique suffix.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) const PIPE_PREFIX: &str = r"\\.\pipe\rbx-netsim-";

/// WinDivert filter matching UDP traffic to and from the given local ports.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn ports_filter(ports: &[u16]) -> String {
    if ports.is_empty() {
        return "false".to_string();
    }
    let clauses: Vec<String> = ports
        .iter()
        .map(|p| format!("(outbound and udp.SrcPort == {p}) or (inbound and udp.DstPort == {p})"))
        .collect();
    format!("udp and ({})", clauses.join(" or "))
}

/// Plain-words explanation of a `WinDivertOpen` `GetLastError` code.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn error_text(code: u32) -> String {
    match code {
        2 => "WinDivert.dll or WinDivert64.sys is missing: download WinDivert 2.2 from https://github.com/basil00/WinDivert/releases (LGPLv3) and put both files from its x64 folder next to rbxstudio.exe".to_string(),
        5 => "WinDivert needs administrator rights".to_string(),
        87 => "WinDivert rejected the packet filter (bad filter)".to_string(),
        577 => "Windows rejected the WinDivert driver signature (Secure Boot)".to_string(),
        654 => "another program's older WinDivert driver is loaded: close it or reboot".to_string(),
        1060 => "the WinDivert driver is not installed".to_string(),
        1275 => "Windows blocked the WinDivert driver (Memory Integrity / vulnerable driver blocklist)".to_string(),
        1753 => "the Base Filtering Engine service is off: WinDivert needs it running".to_string(),
        other => format!("WinDivert failed (error {other})"),
    }
}

/// PIDs from `tasklist /FI "IMAGENAME eq ..." /NH /FO CSV` output.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn parse_tasklist_pids(csv: &str) -> Vec<u32> {
    csv.lines()
        .filter_map(|line| {
            let row = line.trim().strip_prefix('"')?;
            row.split("\",\"").nth(1)?.trim_matches('"').parse().ok()
        })
        .collect()
}

/// Port from a `MIB_UDPROW` `dwLocalPort`, which holds it in network byte
/// order in the low 16 bits.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn local_port(dw_local_port: u32) -> u16 {
    u16::from_be(dw_local_port as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_for_no_ports_matches_nothing() {
        assert_eq!(ports_filter(&[]), "false");
    }

    #[test]
    fn filter_for_one_port() {
        assert_eq!(
            ports_filter(&[5]),
            "udp and ((outbound and udp.SrcPort == 5) or (inbound and udp.DstPort == 5))"
        );
    }

    #[test]
    fn filter_for_two_ports() {
        assert_eq!(
            ports_filter(&[1, 2]),
            "udp and ((outbound and udp.SrcPort == 1) or (inbound and udp.DstPort == 1) or (outbound and udp.SrcPort == 2) or (inbound and udp.DstPort == 2))"
        );
    }

    #[test]
    fn error_text_known_and_unknown() {
        assert!(error_text(2).contains("WinDivert64.sys is missing"));
        assert!(error_text(5).contains("administrator"));
        assert!(error_text(87).contains("filter"));
        assert!(error_text(577).contains("Secure Boot"));
        assert!(error_text(654).contains("reboot"));
        assert!(error_text(1060).contains("not installed"));
        assert!(error_text(1275).contains("Memory Integrity"));
        assert!(error_text(1753).contains("Base Filtering Engine"));
        assert_eq!(error_text(9), "WinDivert failed (error 9)");
    }

    #[test]
    fn tasklist_two_rows() {
        let csv = "\"RobloxPlayerBeta.exe\",\"1234\",\"Console\",\"1\",\"300,000 K\"\r\n\
                   \"RobloxPlayerBeta.exe\",\"77\",\"Console\",\"1\",\"12 K\"\r\n";
        assert_eq!(parse_tasklist_pids(csv), vec![1234, 77]);
    }

    #[test]
    fn tasklist_info_line_and_empty() {
        let info = "INFO: No tasks are running which match the specified criteria.\r\n";
        assert!(parse_tasklist_pids(info).is_empty());
        assert!(parse_tasklist_pids("").is_empty());
    }

    #[test]
    fn local_port_swaps_network_order() {
        assert_eq!(local_port(0x0000_39E2), u16::from_be(0x39E2));
        assert_eq!(
            local_port(0xFFFF_0000 | u32::from(0x1234u16.to_be())),
            0x1234
        );
    }

    #[test]
    fn constants() {
        assert_eq!(HELPER_FLAG, "--netsim-helper");
        assert!(PIPE_PREFIX.starts_with(r"\\.\pipe\"));
    }
}
