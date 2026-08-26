//! 服务管理 —— 对齐 Electron 版 system/service.js
//! 通过 sc.exe 子进程实现, 复用 exec::run_async

use crate::system::exec::{run_async, decode};
use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::sync::Arc;
use std::thread;

/// 受保护服务列表 —— 对齐 service.js#3-5 PROTECTED_SERVICES
const PROTECTED_SERVICES: &[&str] = &[
    "winlogon", "lsass", "csrss", "services", "smss", "svchost",
    "explorer", "dwm", "wininit", "spoolsv",
];

/// 判断是否为受保护服务 —— 对齐 service.js#7-9 isProtected()
pub fn is_protected(name: &str) -> bool {
    let lower = name.to_lowercase();
    PROTECTED_SERVICES.iter().any(|&s| s == lower)
}

/// 返回受保护服务列表(供上层枚举)
pub fn protected_services() -> Value {
    json!(PROTECTED_SERVICES)
}

/// 查询单个服务状态 —— 对齐 service.js#11-23 queryService()
/// 返回 { name, state, stateCode, running } 或 null
pub async fn query_service(name: &str) -> Result<Option<Value>, String> {
    let res = run_async("sc", &["query", name], None).await?;
    if res.contains("FAILED") || res.contains("1060") {
        // 服务不存在
        return Ok(None);
    }
    let out = res;
    // STATE        : 4  RUNNING
    let state = extract_field(&out, "STATE").and_then(|s| {
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() >= 2 {
            Some((parts[0].to_string(), parts[1].to_string()))
        } else {
            None
        }
    });
    // SERVICE_NAME: name
    let svc_name = extract_field(&out, "SERVICE_NAME").unwrap_or_else(|| name.to_string());

    let (state_code, state_str, running) = state.map(|(code, s)| {
        let code = code.parse::<i32>().unwrap_or(0);
        (code, s.clone(), s == "RUNNING")
    }).unwrap_or((0, String::new(), false));

    Ok(Some(json!({
        "name": svc_name,
        "state": state_str,
        "stateCode": state_code,
        "running": running,
    })))
}

/// 设置服务启动类型 —— 对齐 service.js#29-36 setServiceStartType()
/// type: "Auto" | "Manual" | "Disabled"
/// 返回 { ok, code, message }
pub async fn set_service_start_type(name: &str, start_type: &str) -> Result<Value, String> {
    if is_protected(name) {
        return Ok(json!({
            "ok": false,
            "code": -1,
            "message": format!("Protected service: {}", name),
        }));
    }
    let arg = match start_type {
        "Auto" => "auto",
        "Manual" => "demand",
        "Disabled" => "disabled",
        _ => return Ok(json!({ "ok": false, "code": -1, "message": "Invalid start type" })),
    };
    let res = run_async("sc", &["config", name, "start=", arg], None).await?;
    let ok = !res.contains("FAILED") && !res.contains("[SC] OpenService FAILED");
    Ok(json!({
        "ok": ok,
        "code": if ok { 0 } else { 1 },
        "message": res,
    }))
}

const START_TYPE_MAP: &[(i32, &str)] = &[
    (0, "BOOT_START"),
    (1, "SYSTEM_START"),
    (2, "AUTO_START"),
    (3, "DEMAND_START"),
    (4, "DISABLED"),
];

/// 列举所有服务及启动类型 —— 对齐 service.js#44-76 listServices()
/// 并发度 8, 返回 [{ name, startType, startTypeCode, binaryPath }]
/// 使用线程池模拟并发, 避免引入 futures 依赖
pub async fn list_services() -> Result<Vec<Value>, String> {
    // 先获取所有服务名
    let res = run_async("sc", &["query", "type=", "service", "state=", "all"], None).await?;
    let mut names = Vec::new();
    for line in res.lines() {
        if let Some(name) = extract_field(line, "SERVICE_NAME") {
            names.push(name);
        }
    }

    // 并发查询每个服务的 qc 信息
    let concurrency = 8;
    let mut services = Vec::with_capacity(names.len());

    // 使用简单的线程池模式: 分批启动线程, 等待完成
    let names_arc = Arc::new(names);
    let mut idx = 0;

    while idx < names_arc.len() {
        let batch_end = (idx + concurrency).min(names_arc.len());
        let batch = names_arc[idx..batch_end].to_vec();
        idx = batch_end;

        let mut handles = Vec::new();
        for name in batch {
            let handle = thread::spawn(move || {
                // 在线程中阻塞调用 run_async 的同步版本
                // 这里我们需要同步版本的 sc 查询
                let qc = std::process::Command::new("sc")
                    .args(["qc", &name])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .creation_flags(0x08000000)
                    .output();
                (name, qc)
            });
            handles.push(handle);
        }

        for handle in handles {
            let (name, qc_result) = handle.join().unwrap();
            let mut start_type = None;
            let mut start_type_code = None;
            let mut binary_path = None;

            if let Ok(qc) = qc_result {
                if qc.status.success() {
                    let qc_out = crate::system::exec::decode(&qc.stdout);
                    if let Some(st) = extract_field(&qc_out, "START_TYPE") {
                        let parts: Vec<&str> = st.split_whitespace().collect();
                        if parts.len() >= 2 {
                            start_type_code = parts[0].parse::<i32>().ok();
                            start_type = Some(parts[1].to_string());
                        }
                    }
                    if let Some(bp) = extract_field(&qc_out, "BINARY_PATH_NAME") {
                        binary_path = Some(bp.trim().to_string());
                    }
                }
            }

            // 映射 start_type_code 到名称
            let start_type = start_type.or_else(|| {
                start_type_code.and_then(|code| {
                    START_TYPE_MAP.iter().find(|(c, _)| *c == code).map(|(_, n)| n.to_string())
                })
            });

            services.push(json!({
                "name": name,
                "startType": start_type,
                "startTypeCode": start_type_code,
                "binaryPath": binary_path,
            }));
        }
    }

    Ok(services)
}

/// 从 sc 输出中提取字段值(冒号后的内容)
fn extract_field(output: &str, field: &str) -> Option<String> {
    let prefix = format!("{}:", field);
    for line in output.lines() {
        let trimmed = line.trim();
        // 快路径: 字段名紧跟冒号
        if let Some(rest) = trimmed.strip_prefix(prefix.as_str()) {
            return Some(rest.trim().to_string());
        }
        // sc query 输出的字段名与冒号之间有对齐空格: "STATE        : 4  RUNNING"
        if trimmed.starts_with(field) {
            if let Some(pos) = trimmed.find(':') {
                return Some(trimmed[pos + 1..].trim().to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_protected() {
        assert!(is_protected("winlogon"));
        assert!(is_protected("WINLOGON"));
        assert!(!is_protected("myservice"));
    }

    #[test]
    fn test_extract_field() {
        let out = "SERVICE_NAME: wuauserv\nSTATE        : 4  RUNNING";
        assert_eq!(extract_field(out, "SERVICE_NAME"), Some("wuauserv".to_string()));
        assert_eq!(extract_field(out, "STATE"), Some("4  RUNNING".to_string()));
    }
}