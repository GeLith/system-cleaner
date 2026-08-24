//! 进程管理 —— 对齐 Electron 版 system/process.js
//! 通过 PowerShell (Get-Process) 与 taskkill 实现

use crate::system::exec::run_async;
use serde_json::{json, Value};

/// 列举运行中进程 —— 对齐 process.js#8-26 listProcesses()
/// 返回 [{ pid, name, path, workingSet }], path 为 null 表示无权限/系统进程
pub async fn list_processes() -> Result<Vec<Value>, String> {
    let res = run_async(
        "powershell",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-Process | Select-Object Id,ProcessName,Path,WorkingSet64 | ConvertTo-Json -Compress",
        ],
        None,
    ).await?;

    if res.trim().is_empty() {
        return Ok(Vec::new());
    }

    // 解析 JSON 输出
    let parsed: Value = serde_json::from_str(&res).map_err(|e| format!("JSON parse failed: {}", e))?;
    let arr = if parsed.is_array() {
        parsed.as_array().unwrap()
    } else if parsed.is_object() {
        // 单个进程时返回对象而非数组
        &vec![parsed]
    } else {
        return Ok(Vec::new());
    };

    let mut processes = Vec::with_capacity(arr.len());
    for p in arr {
        let pid = p.get("Id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let name = p.get("ProcessName").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let path = p.get("Path").and_then(|v| v.as_str()).map(|s| s.to_string());
        let working_set = p.get("WorkingSet64").and_then(|v| v.as_u64()).unwrap_or(0);

        processes.push(json!({
            "pid": pid,
            "name": name,
            "path": path,
            "workingSet": working_set,
        }));
    }
    Ok(processes)
}

/// 杀死进程 —— 对齐 process.js#28-31 killProcess()
/// 返回 { ok, code, message }
pub async fn kill_process(pid: u32) -> Result<Value, String> {
    let res = run_async("taskkill", &["/PID", &pid.to_string(), "/F"], None).await?;
    let ok = res.contains("SUCCESS") || res.contains("成功");
    Ok(json!({
        "ok": ok,
        "code": if ok { 0 } else { 1 },
        "message": res,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_processes() {
        let res = list_processes().await;
        assert!(res.is_ok());
        let procs = res.unwrap();
        // 至少有当前进程
        assert!(!procs.is_empty());
        for p in procs {
            assert!(p.get("pid").is_some());
            assert!(p.get("name").is_some());
            assert!(p.get("workingSet").is_some());
        }
    }
}