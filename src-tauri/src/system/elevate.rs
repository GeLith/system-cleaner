//! UAC 提权执行 —— 启动项删除(HKLM 值/系统计划任务/HKCR 机装键)在未提权被拒后,
//! 走一次 ShellExecuteW("runas") 弹 UAC 的重试通道, 同步等待退出码。
//! 不经 PowerShell 中转, 规避多层引号转义问题。

use std::ffi::c_void;

type HANDLE = *mut c_void;
type HWND = *mut c_void;
type PCWSTR = *const u16;
type HINSTANCE = *mut c_void;

const SEE_MASK_NOCLOSEPROCESS: u32 = 0x0000_0040;
const SW_HIDE: i32 = 0;
const WAIT_TIMEOUT_MS: u32 = 180_000; // 留足用户点 UAC 的时间

#[repr(C)]
struct SHELLEXECUTEINFOW {
    cb_size: u32,
    f_mask: u32,
    hwnd: HWND,
    lp_verb: PCWSTR,
    lp_file: PCWSTR,
    lp_parameters: PCWSTR,
    lp_directory: PCWSTR,
    n_show: i32,
    h_inst_app: HINSTANCE,
    lp_id_list: *mut c_void,
    lp_class: PCWSTR,
    hkey_class: HANDLE,
    dw_hot_key: u32,
    icon_or_monitor: HANDLE,
    h_process: HANDLE,
}

#[link(name = "shell32")]
extern "system" {
    fn ShellExecuteExW(sei: *mut SHELLEXECUTEINFOW) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn WaitForSingleObject(handle: HANDLE, ms: u32) -> u32;
    fn GetExitCodeProcess(handle: HANDLE, code: *mut u32) -> i32;
    fn CloseHandle(handle: HANDLE) -> i32;
    fn GetLastError() -> u32;
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn run_elevated_sync(file: &str, parameters: &str) -> Result<u32, String> {
    unsafe {
        let verb = wide("runas");
        let f = wide(file);
        let p = wide(parameters);
        let mut sei: SHELLEXECUTEINFOW = std::mem::zeroed();
        sei.cb_size = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        sei.f_mask = SEE_MASK_NOCLOSEPROCESS;
        sei.lp_verb = verb.as_ptr();
        sei.lp_file = f.as_ptr();
        sei.lp_parameters = p.as_ptr();
        sei.n_show = SW_HIDE;

        if ShellExecuteExW(&mut sei) == 0 {
            let err = GetLastError();
            return Err(if err == 1223 {
                "用户取消了管理员授权".to_string()
            } else {
                format!("提权启动失败 (Win32 error {})", err)
            });
        }
        if sei.h_process.is_null() {
            return Err("提权进程句柄不可用".to_string());
        }
        WaitForSingleObject(sei.h_process, WAIT_TIMEOUT_MS);
        let mut code: u32 = 1;
        GetExitCodeProcess(sei.h_process, &mut code);
        CloseHandle(sei.h_process);
        Ok(code)
    }
}

/// 以管理员身份执行 file parameters, 阻塞等待其退出并返回退出码(0=成功)。
/// 用户在 UAC 取消时返回 Err。
pub async fn run_elevated(file: &str, parameters: &str) -> Result<u32, String> {
    let f = file.to_string();
    let p = parameters.to_string();
    tauri::async_runtime::spawn_blocking(move || run_elevated_sync(&f, &p))
        .await
        .unwrap_or_else(|e| Err(format!("elevation join error: {}", e)))
}

/// 判定一条命令错误信息是否为权限拒绝(用于决定是否走提权重试)
pub fn is_access_denied(msg: &str) -> bool {
    let m = msg.to_lowercase();
    m.contains("拒绝访问")
        || m.contains("access is denied")
        || m.contains("(os error 5)")
        || m.contains("denied")
}
