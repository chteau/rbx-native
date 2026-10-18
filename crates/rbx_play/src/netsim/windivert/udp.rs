//! Finding the Roblox client's UDP ports without opening the client process.

use std::ffi::c_void;

use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, NO_ERROR};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedUdpTable, MIB_UDP6ROW_OWNER_PID, MIB_UDP6TABLE_OWNER_PID, MIB_UDPROW_OWNER_PID,
    MIB_UDPTABLE_OWNER_PID, UDP_TABLE_OWNER_PID,
};
use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};

pub(super) fn client_pids() -> Vec<u32> {
    let out = crate::launch::command_for("tasklist")
        .args([
            "/FI",
            "IMAGENAME eq RobloxPlayerBeta.exe",
            "/NH",
            "/FO",
            "CSV",
        ])
        .output();
    match out {
        Ok(o) => super::parse_tasklist_pids(&String::from_utf8_lossy(&o.stdout)),
        Err(_) => Vec::new(),
    }
}

/// Raw table bytes for one address family; empty on failure. Backed by u64s
/// so the table structs are suitably aligned.
fn table(af: u32) -> Vec<u64> {
    let mut size = 0u32;
    let mut buf: Vec<u64> = Vec::new();
    for _ in 0..8 {
        let ptr = if buf.is_empty() {
            std::ptr::null_mut::<c_void>()
        } else {
            buf.as_mut_ptr().cast()
        };
        // SAFETY: `ptr` is null or points at `size` writable bytes.
        let rc = unsafe { GetExtendedUdpTable(ptr, &mut size, 0, af, UDP_TABLE_OWNER_PID, 0) };
        if rc == NO_ERROR {
            return buf;
        }
        if rc != ERROR_INSUFFICIENT_BUFFER {
            return Vec::new();
        }
        buf = vec![0u64; (size as usize).div_ceil(8) + 8];
        size = (buf.len() * 8) as u32;
    }
    Vec::new()
}

pub(super) fn ports_of(pids: &[u32]) -> Vec<u16> {
    let mut ports = Vec::new();
    let v4 = table(AF_INET as u32);
    if !v4.is_empty() {
        // SAFETY: a successful call filled a MIB_UDPTABLE_OWNER_PID of
        // `dwNumEntries` rows in an 8-byte-aligned buffer.
        unsafe {
            let t = v4.as_ptr().cast::<MIB_UDPTABLE_OWNER_PID>();
            let rows = std::slice::from_raw_parts(
                std::ptr::addr_of!((*t).table).cast::<MIB_UDPROW_OWNER_PID>(),
                (*t).dwNumEntries as usize,
            );
            for r in rows.iter().filter(|r| pids.contains(&r.dwOwningPid)) {
                ports.push(super::local_port(r.dwLocalPort));
            }
        }
    }
    let v6 = table(AF_INET6 as u32);
    if !v6.is_empty() {
        // SAFETY: as above, for the IPv6 table.
        unsafe {
            let t = v6.as_ptr().cast::<MIB_UDP6TABLE_OWNER_PID>();
            let rows = std::slice::from_raw_parts(
                std::ptr::addr_of!((*t).table).cast::<MIB_UDP6ROW_OWNER_PID>(),
                (*t).dwNumEntries as usize,
            );
            for r in rows.iter().filter(|r| pids.contains(&r.dwOwningPid)) {
                ports.push(super::local_port(r.dwLocalPort));
            }
        }
    }
    ports.retain(|p| *p != 0);
    ports.sort_unstable();
    ports.dedup();
    ports
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_pids_no_ports() {
        assert!(ports_of(&[]).is_empty());
    }
}
