//! Minimal dynamic binding to WinDivert 2.x (`WinDivert.dll`), loaded by
//! absolute path so a planted DLL in the working directory is never picked up.

use std::ffi::{c_void, OsStr};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::Arc;

use windows_sys::Win32::Foundation::{GetLastError, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

const QUEUE_LENGTH: i32 = 0;
const QUEUE_TIME: i32 = 1;
const QUEUE_SIZE: i32 = 2;
const SHUTDOWN_RECV: i32 = 1;
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_PROC_NOT_FOUND: u32 = 127;

/// `WINDIVERT_ADDRESS`. The bitfield layout (outbound = bit 17 of `bits`) is
/// inferred from the header under LSB-first MSVC allocation and has not been
/// verified on hardware.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Address {
    timestamp: i64,
    bits: u32,
    reserved2: u32,
    data: [u8; 64],
}

const _: () = assert!(std::mem::size_of::<Address>() == 80);

impl Address {
    const OUTBOUND_BIT: u32 = 17;

    pub(super) fn zeroed() -> Address {
        Address {
            timestamp: 0,
            bits: 0,
            reserved2: 0,
            data: [0; 64],
        }
    }

    pub(super) fn outbound(&self) -> bool {
        (self.bits >> Self::OUTBOUND_BIT) & 1 != 0
    }
}

type OpenFn = unsafe extern "C" fn(*const u8, i32, i16, u64) -> *mut c_void;
type RecvFn = unsafe extern "C" fn(*mut c_void, *mut c_void, u32, *mut u32, *mut Address) -> i32;
type SendFn =
    unsafe extern "C" fn(*mut c_void, *const c_void, u32, *mut u32, *const Address) -> i32;
type ShutdownFn = unsafe extern "C" fn(*mut c_void, i32) -> i32;
type CloseFn = unsafe extern "C" fn(*mut c_void) -> i32;
type SetParamFn = unsafe extern "C" fn(*mut c_void, i32, u64) -> i32;

pub(super) struct Api {
    open: OpenFn,
    recv: RecvFn,
    send: SendFn,
    shutdown: ShutdownFn,
    close: CloseFn,
    set_param: SetParamFn,
}

impl Api {
    /// Errors are Win32 codes: 2 when the DLL cannot be loaded, 127 when an
    /// export is missing.
    pub(super) fn load(dll: &Path) -> Result<Api, u32> {
        let wide: Vec<u16> = OsStr::new(dll.as_os_str())
            .encode_wide()
            .chain(Some(0))
            .collect();
        // SAFETY: `wide` is NUL-terminated and outlives the call.
        let module = unsafe { LoadLibraryW(wide.as_ptr()) };
        if module.is_null() {
            return Err(ERROR_FILE_NOT_FOUND);
        }
        macro_rules! export {
            ($name:literal, $ty:ty) => {{
                // SAFETY: the name is NUL-terminated; the signature matches windivert.h.
                match unsafe { GetProcAddress(module, concat!($name, "\0").as_ptr()) } {
                    Some(f) => unsafe {
                        std::mem::transmute::<unsafe extern "system" fn() -> isize, $ty>(f)
                    },
                    None => return Err(ERROR_PROC_NOT_FOUND),
                }
            }};
        }
        Ok(Api {
            open: export!("WinDivertOpen", OpenFn),
            recv: export!("WinDivertRecv", RecvFn),
            send: export!("WinDivertSend", SendFn),
            shutdown: export!("WinDivertShutdown", ShutdownFn),
            close: export!("WinDivertClose", CloseFn),
            set_param: export!("WinDivertSetParam", SetParamFn),
        })
    }
}

pub(super) struct Handle {
    api: Arc<Api>,
    raw: *mut c_void,
}

// SAFETY: WinDivert handles are plain kernel handles; the library documents
// concurrent Recv and Send on one handle as supported.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

impl Handle {
    pub(super) fn open(api: &Arc<Api>, filter: &str, priority: i16) -> Result<Handle, u32> {
        let mut f = filter.as_bytes().to_vec();
        f.push(0);
        // SAFETY: `f` is a NUL-terminated C string; layer 0 is NETWORK.
        let raw = unsafe { (api.open)(f.as_ptr(), 0, priority, 0) };
        if raw as isize == INVALID_HANDLE_VALUE as isize {
            // SAFETY: no preconditions.
            return Err(unsafe { GetLastError() });
        }
        let handle = Handle {
            api: Arc::clone(api),
            raw,
        };
        // Best effort: the defaults still work, just with less headroom.
        for (param, value) in [
            (QUEUE_LENGTH, 16_384u64),
            (QUEUE_TIME, 16_000),
            (QUEUE_SIZE, 33_554_432),
        ] {
            // SAFETY: `raw` is a live handle.
            unsafe { (api.set_param)(raw, param, value) };
        }
        Ok(handle)
    }

    pub(super) fn recv(&self, buf: &mut [u8]) -> Option<(usize, Address)> {
        let mut addr = Address::zeroed();
        let mut len = 0u32;
        // SAFETY: buffer pointer/length describe `buf`; out-params are valid.
        let ok = unsafe {
            (self.api.recv)(
                self.raw,
                buf.as_mut_ptr().cast(),
                buf.len() as u32,
                &mut len,
                &mut addr,
            )
        };
        (ok != 0).then_some((len as usize, addr))
    }

    pub(super) fn send(&self, packet: &[u8], addr: &Address) -> bool {
        let mut sent = 0u32;
        // SAFETY: pointer/length describe `packet`; `addr` is a valid address.
        unsafe {
            (self.api.send)(
                self.raw,
                packet.as_ptr().cast(),
                packet.len() as u32,
                &mut sent,
                addr,
            ) != 0
        }
    }

    pub(super) fn shutdown_recv(&self) {
        // SAFETY: `raw` is a live handle.
        unsafe { (self.api.shutdown)(self.raw, SHUTDOWN_RECV) };
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: `raw` is closed exactly once.
        unsafe { (self.api.close)(self.raw) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outbound_reads_bit_17() {
        let mut a = Address::zeroed();
        assert!(!a.outbound());
        a.bits = 1 << 17;
        assert!(a.outbound());
        a.bits = !(1 << 17);
        assert!(!a.outbound());
    }

    #[test]
    fn missing_dll_is_error_2() {
        let r = Api::load(Path::new(r"C:\definitely\missing\WinDivert.dll"));
        assert_eq!(r.err(), Some(2));
    }
}
