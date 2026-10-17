//! 进程存在性和启动身份。
//!
//! macOS 用 proc_pidinfo；Windows 用 OpenProcess / GetProcessTimes。
//! 不读写文件，不持有凭据。

#[cfg(target_os = "windows")]
mod windows_process {
    use std::mem::MaybeUninit;

    #[repr(C)]
    struct FileTime {
        dw_low_date_time: u32,
        dw_high_date_time: u32,
    }

    type Handle = *mut std::ffi::c_void;

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const PROCESS_QUERY_INFORMATION: u32 = 0x0400;

    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
        fn GetProcessTimes(
            handle: Handle,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
    }

    fn open_query_handle(pid: u32) -> Result<Handle, String> {
        // SAFETY: OpenProcess returns NULL or a handle that CloseHandle must release.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if !handle.is_null() {
            return Ok(handle);
        }
        let handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION, 0, pid) };
        if handle.is_null() {
            return Err(format!("进程 {pid} 已不存在。"));
        }
        Ok(handle)
    }

    pub fn process_exists(pid: u32) -> bool {
        match open_query_handle(pid) {
            Ok(handle) => {
                unsafe { CloseHandle(handle) };
                true
            }
            Err(_) => false,
        }
    }

    pub fn process_start_identity(pid: u32) -> Result<String, String> {
        let handle = open_query_handle(pid)?;
        let mut creation = MaybeUninit::<FileTime>::zeroed();
        let mut exit = MaybeUninit::<FileTime>::zeroed();
        let mut kernel = MaybeUninit::<FileTime>::zeroed();
        let mut user = MaybeUninit::<FileTime>::zeroed();
        // SAFETY: handle is a valid process handle; buffers match GetProcessTimes.
        let ok = unsafe {
            GetProcessTimes(
                handle,
                creation.as_mut_ptr(),
                exit.as_mut_ptr(),
                kernel.as_mut_ptr(),
                user.as_mut_ptr(),
            )
        };
        unsafe { CloseHandle(handle) };
        if ok == 0 {
            return Err(format!("进程 {pid} 已不存在。"));
        }
        let creation = unsafe { creation.assume_init() };
        let ticks =
            ((creation.dw_high_date_time as u64) << 32) | u64::from(creation.dw_low_date_time);
        Ok(ticks.to_string())
    }
}

pub fn command_without_console(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[cfg_attr(not(target_os = "windows"), allow(unused_mut))]
    let mut command = std::process::Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Stop a verified instance PID. Windows uses taskkill so child renderer
/// processes go with the supervisor; Unix stop_instance still sends SIGTERM.
#[cfg(target_os = "windows")]
pub fn terminate_pid(pid: u32) -> Result<(), String> {
    let output = command_without_console("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .output()
        .map_err(|error| format!("无法停止 Codex 实例：{error}"))?;
    if output.status.success() || !process_exists(pid) {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr);
    let detail = detail.trim();
    if detail.is_empty() {
        Err("无法停止 Codex 实例。".to_string())
    } else {
        Err(format!("无法停止 Codex 实例：{detail}"))
    }
}

pub fn process_exists(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // SAFETY: signal 0 performs existence/permission checking only.
        let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
        if result == 0 {
            return true;
        }
        return std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH);
    }
    #[cfg(target_os = "windows")]
    {
        return windows_process::process_exists(pid);
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        let _ = pid;
        true
    }
}

pub fn process_start_identity(pid: u32) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
        let size: libc::c_int = std::mem::size_of::<libc::proc_bsdinfo>()
            .try_into()
            .map_err(|_| "进程信息结构大小无效。".to_string())?;
        // SAFETY: info points to a writable buffer with the exact size required by PROC_PIDTBSDINFO.
        let result = unsafe {
            libc::proc_pidinfo(
                pid as libc::c_int,
                libc::PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr().cast(),
                size,
            )
        };
        if result != size {
            return Err(format!("进程 {pid} 已不存在。"));
        }
        let info = unsafe { info.assume_init() };
        return Ok(format!(
            "{}.{:06}",
            info.pbi_start_tvsec, info.pbi_start_tvusec
        ));
    }

    #[cfg(target_os = "windows")]
    {
        return windows_process::process_start_identity(pid);
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = pid;
        Err("当前平台尚不支持进程启动身份核验。".to_string())
    }
}
