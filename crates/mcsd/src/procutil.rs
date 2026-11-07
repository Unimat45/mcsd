//! Platform-specific process plumbing.
//!
//! * children get their own process group so Ctrl+C in the daemon's terminal
//!   does not hit them directly; the daemon stops them gracefully instead.
//! * children must not outlive a crashed daemon (an orphan would hold the
//!   world lock and the port): a kill-on-close Job Object on Windows,
//!   PR_SET_PDEATHSIG on Linux, and PID-based cleanup on next boot elsewhere.

use std::time::Duration;
use tokio::process::Command;

pub fn configure(cmd: &mut Command) {
    #[cfg(windows)]
    {
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }
    #[cfg(target_os = "linux")]
    unsafe {
        cmd.pre_exec(|| {
            // SIGTERM (not SIGKILL) so the JVM shutdown hook can save worlds.
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
}

/// For short helper processes (e.g. `java -version`): just hide the window.
pub fn configure_helper(cmd: &mut Command) {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// Tie a freshly spawned child to the daemon's lifetime (Windows only).
pub fn attach(child: &tokio::process::Child) {
    #[cfg(windows)]
    win::attach(child);
    #[cfg(not(windows))]
    let _ = child;
}

#[cfg(windows)]
mod win {
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
    };

    // Stored as usize because raw handles are not Send/Sync.
    static JOB: OnceLock<usize> = OnceLock::new();

    fn job() -> Option<HANDLE> {
        let h = *JOB.get_or_init(|| unsafe {
            let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if h.is_null() {
                return 0;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                h,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of_val(&info) as u32,
            );
            if ok == 0 {
                CloseHandle(h);
                return 0;
            }
            // Intentionally never closed: the handle dies with the process,
            // which is exactly what kills the children.
            h as usize
        });
        (h != 0).then_some(h as HANDLE)
    }

    pub fn attach(child: &tokio::process::Child) {
        if let (Some(job), Some(process)) = (job(), child.raw_handle()) {
            unsafe {
                if AssignProcessToJobObject(job, process as HANDLE) == 0 {
                    tracing::warn!("could not assign child to job object; it may outlive a daemon crash");
                }
            }
        }
    }
}

/// After a daemon crash, a previous server process may still be running
/// (possible on macOS/BSD). If `pid` is still a java process, ask it to
/// terminate (JVM shutdown hooks save the world), then kill it.
pub async fn reap_orphan(pid: u32, server_dir: &std::path::Path) -> bool {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let spid = Pid::from_u32(pid);
    let Ok(mut sys) = System::new() else { return false };
    let refresh = ProcessRefreshKind::nothing().with_cwd(UpdateKind::Always);
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[spid]), true, refresh);
    let Some(p) = sys.process(spid) else { return false };
    let is_java = p.name().to_string_lossy().to_ascii_lowercase().contains("java");
    // Only trust the PID if it still looks like *our* server (PIDs get reused).
    let same_dir = match p.cwd() {
        Some(cwd) => same_path(cwd, server_dir),
        None => true, // cwd not readable (permissions): fall back to the name check
    };
    if !is_java || !same_dir {
        return false;
    }
    tracing::warn!(pid, dir = %server_dir.display(), "terminating orphaned server process from a previous daemon run");
    if p.kill_with(sysinfo::Signal::Term).is_none() {
        p.kill();
    }
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[spid]), true, ProcessRefreshKind::nothing());
        if sys.process(spid).is_none() {
            return true;
        }
    }
    if let Some(p) = sys.process(spid) {
        p.kill();
    }
    true
}

fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}
