#[cfg(target_os = "windows")]
mod windows_impl {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use std::sync::Mutex;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        SetInformationJobObject,
    };

    pub struct JobObject {
        handle: Mutex<HANDLE>,
    }

    impl JobObject {
        pub fn new() -> Result<Self, String> {
            unsafe {
                let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if handle.is_null() {
                    return Err("CreateJobObjectW failed".to_string());
                }

                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

                let ok = SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 {
                    CloseHandle(handle);
                    return Err("SetInformationJobObject failed".to_string());
                }

                Ok(Self {
                    handle: Mutex::new(handle),
                })
            }
        }

        pub fn assign_process(&self, child: &mut Child) -> Result<(), String> {
            unsafe {
                let job = *self.handle.lock().unwrap();
                let proc = child.as_raw_handle() as HANDLE;
                if proc.is_null() {
                    return Err(format!("invalid handle for pid {}", child.id()));
                }
                if AssignProcessToJobObject(job, proc) == 0 {
                    return Err(format!(
                        "AssignProcessToJobObject failed for pid {}",
                        child.id()
                    ));
                }
                Ok(())
            }
        }
    }

    impl Drop for JobObject {
        fn drop(&mut self) {
            unsafe {
                let h = *self.handle.lock().unwrap();
                if h != std::ptr::null_mut() {
                    CloseHandle(h);
                }
            }
        }
    }

    unsafe impl Send for JobObject {}
    unsafe impl Sync for JobObject {}
}

#[cfg(target_os = "windows")]
pub use windows_impl::JobObject;

#[cfg(not(target_os = "windows"))]
mod other_impl {
    pub struct JobObject;

    impl JobObject {
        pub fn new() -> Result<Self, String> {
            Ok(Self)
        }

        pub fn assign_process(&self, _child: &mut std::process::Child) -> Result<(), String> {
            Ok(())
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub use other_impl::JobObject;
