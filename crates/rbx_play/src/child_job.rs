//! Child processes that must not outlive the editor (cloudflared, luau-lsp).
//!
//! Windows has no parent-death signal, and a crash or a kill skips every
//! `Drop`: cloudflared was seen still running 75 s after quitting. Each such
//! child goes into one Job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`;
//! its only handle is this process's, which the kernel closes however the
//! editor ends, and that kills the job's processes. Elsewhere this does
//! nothing (Linux has `PR_SET_PDEATHSIG`, see `tunnel::end_with_parent`).
//!
//! Only these children go in, not the editor itself: a job holding the
//! editor would also take down whatever it opens through the shell (the
//! browser, Roblox, a relaunched editor).

/// Ties `child` to the editor's lifetime. Failing only logs: the child
/// still runs, it just may outlive a crash.
pub fn kill_with_editor(child: &std::process::Child) {
    #[cfg(windows)]
    if let Err(code) = windows::assign(child) {
        eprintln!(
            "rbx_play: could not tie child {} to the editor (error {code})",
            child.id()
        );
    }
    #[cfg(not(windows))]
    let _ = child;
}

#[cfg(windows)]
mod windows {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;

    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// The job's handle as an integer (`HANDLE` is a pointer, not `Sync`),
    /// or the error that kept it from existing. Never closed: that is the
    /// kernel's job when this process ends.
    fn job() -> Result<usize, u32> {
        static JOB: OnceLock<Result<usize, u32>> = OnceLock::new();
        *JOB.get_or_init(|| {
            // SAFETY: plain Win32 calls; the info struct is zeroed and sized
            // for its class, and a null job is checked first.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return Err(GetLastError());
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 {
                    return Err(GetLastError());
                }
                Ok(job as usize)
            }
        })
    }

    pub(super) fn assign(child: &std::process::Child) -> Result<(), u32> {
        let job = job()?;
        // SAFETY: both handles are live: the job is never closed and the
        // child's is owned by `child` for the call.
        let ok = unsafe { AssignProcessToJobObject(job as _, child.as_raw_handle() as _) };
        if ok == 0 {
            // SAFETY: no preconditions.
            return Err(unsafe { GetLastError() });
        }
        Ok(())
    }
}
