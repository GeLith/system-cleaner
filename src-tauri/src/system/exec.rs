//! 进程执行与输出解码 —— 对齐 Electron 版 system/exec.js
//! - run: CREATE_NO_WINDOW(0x08000000) 隐藏窗口, 捕获 stdout+stderr, GBK 回退解码
//! - decode: UTF-16LE BOM/NUL 密集 -> UTF-16LE; 否则 GBK; 含替换字符则再试 UTF-8

use std::process::{Command, Stdio};
use std::os::windows::process::CommandExt;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use encoding_rs::{GBK, UTF_8, UTF_16LE};

const CREATE_NO_WINDOW: u32 = 0x08000000;
const DEFAULT_TIMEOUT_MS: u64 = 90_000;

/// 解码命令输出字节 —— 对齐 exec.js#10-29 decode()
/// 逻辑: UTF-16LE BOM(FF FE) 或 NUL 密集(每 2 字节一个 0) -> UTF-16LE;
/// 否则尝试 GBK, 无替换字符则用 GBK; 否则尝试 UTF-8, 含替换字符则回退 GBK
pub fn decode(buf: &[u8]) -> String {
    if buf.is_empty() {
        return String::new();
    }
    // UTF-16LE BOM
    if buf.len() >= 2 && buf[0] == 0xff && buf[1] == 0xfe {
        let (s, _, _) = UTF_16LE.decode(&buf[2..]);
        return s.into_owned();
    }
    // NUL 密集检测: 采样前 4096 字节, 奇数位为 0 的比例 > 1/8 视为 UTF-16LE
    let sample = buf.len().min(4096);
    let mut nul_count = 0;
    for i in (1..sample).step_by(2) {
        if buf[i] == 0 {
            nul_count += 1;
        }
    }
    if nul_count > sample / 8 {
        let (s, _, _) = UTF_16LE.decode(buf);
        // 去除可能的 BOM
        return s.trim_start_matches('\u{FEFF}').to_string();
    }
    // 尝试 GBK
    let (gbk_cow, _, gbk_had_errors) = GBK.decode(buf);
    if !gbk_had_errors {
        return gbk_cow.into_owned();
    }
    // 尝试 UTF-8
    let (utf8_cow, _, utf8_had_errors) = UTF_8.decode(buf);
    if utf8_had_errors {
        // 含替换字符 -> 回退 GBK
        return gbk_cow.into_owned();
    }
    utf8_cow.into_owned()
}

/// 内部执行逻辑, 返回 (stdout, stderr, success)
pub fn run_inner(exe: &str, args: &[&str]) -> Result<(String, String, bool), String> {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW);

    let output = cmd.output().map_err(|e| format!("spawn failed: {}", e))?;

    let stdout = decode(&output.stdout);
    let stderr = decode(&output.stderr);

    Ok((stdout, stderr, output.status.success()))
}

/// 执行命令并捕获输出 —— 对齐 exec.js#35-70 run()
/// 返回 Ok(stdout) 当 code==0, 否则 Err(stderr.trim() 或 stdout.trim())
pub fn run(exe: &str, args: &[&str]) -> Result<String, String> {
    let (stdout, stderr, success) = run_inner(exe, args)?;
    if success {
        Ok(stdout)
    } else {
        let msg = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else {
            stdout.trim().to_string()
        };
        Err(msg)
    }
}

/// 异步变体 —— 对齐 JS 版 Promise 语义, 供 registry/service/process 复用
/// 使用 std::thread + channel 实现超时, 避免引入 tokio 依赖
pub async fn run_async(exe: &str, args: &[&str], timeout_ms: Option<u64>) -> Result<String, String> {
    let exe_owned = exe.to_string();
    let args_owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let timeout_ms = timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);

    // 为错误消息克隆一份
    let exe_for_err = exe_owned.clone();
    let args_for_err = args_owned.clone();

    // 在阻塞线程中执行, 通过 channel 返回结果
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let args_ref: Vec<&str> = args_owned.iter().map(|s| s.as_str()).collect();
        let res = run_inner(&exe_owned, &args_ref);
        let _ = tx.send(res);
    });

    // 等待结果或超时
    let timeout_dur = Duration::from_millis(timeout_ms);
    let res = match rx.recv_timeout(timeout_dur) {
        Ok(r) => r,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            return Err(format!("Command timed out: {} {}", exe_for_err, args_for_err.join(" ")));
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            return Err("Command thread died".to_string());
        }
    };

    let (stdout, stderr, success) = res?;
    if success {
        Ok(stdout)
    } else {
        let msg = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else {
            stdout.trim().to_string()
        };
        Err(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_utf8() {
        let buf = b"hello world";
        assert_eq!(decode(buf), "hello world");
    }

    #[test]
    fn test_decode_gbk() {
        // "中文" in GBK: D6 D0 CE C4
        let buf = &[0xD6, 0xD0, 0xCE, 0xC4];
        assert_eq!(decode(buf), "中文");
    }

    #[test]
    fn test_decode_utf16le_bom() {
        // "test" in UTF-16LE with BOM
        let buf = &[0xFF, 0xFE, 0x74, 0x00, 0x65, 0x00, 0x73, 0x00, 0x74, 0x00];
        assert_eq!(decode(buf), "test");
    }

    #[test]
    fn test_run_echo() {
        let res = run("cmd", &["/C", "echo hello"]);
        assert!(res.is_ok());
        assert!(res.unwrap().contains("hello"));
    }
}