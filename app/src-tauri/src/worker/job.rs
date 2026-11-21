//! Windows Job Object: worker children die with the app (even on a crash or a forced exit).

use std::os::windows::io::AsRawHandle;
use std::process::Child;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

/// Process-wide job handle (never closed: the OS closes it when the app dies, which kills the members).
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
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if ok == 0 {
            0
        } else {
            h as usize
        }
    });
    (h != 0).then_some(h as HANDLE)
}

/// Best effort: a failure only means the child is not auto-killed.
pub fn assign(child: &Child) {
    if let Some(j) = job() {
        let ok = unsafe { AssignProcessToJobObject(j, child.as_raw_handle() as HANDLE) };
        if ok == 0 {
            eprintln!("haru: could not assign worker to job object");
        }
    }
}
