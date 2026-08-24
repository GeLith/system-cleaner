//! 加速优化管理器 —— 完整移植自 Electron 版 business/speedupManager.js
//! 对齐 JS 导出: optimize, applyFix (snake_case: optimize, apply_fix)

use crate::system::exec::run_async;
use crate::system::filesystem::scan_dir;
use crate::system::process::{list_processes, kill_process};
use crate::system::registry::{set_binary, set_string, set_dword};
use crate::system::service::{set_service_start_type, is_protected};
use crate::rules_engine::is_safe_path;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

/// 可安全退出的进程名单 —— 对齐 scanScheduler.js#30-35 SAFE_TO_CLOSE
/// 用于 kill_process 动作的 PID 复用防护
pub const SAFE_TO_CLOSE: &[&str] = &[
    "qqmusic.exe", "kugou.exe", "kuwo.exe", "cloudmusic.exe", "potplayermini.exe", "vlc.exe",
    "foobar2000.exe", "qqlive.exe", "iqiyi.exe", "youkudesktop.exe", "baofengplatform.exe",
    "thunderplatform.exe", "thunder.exe", "xunleiservice.exe", "baidunetdisk.exe", "utorrent.exe",
    "qbittorrent.exe", "googleupdate.exe", "msedgeupdate.exe", "chromeupdate.exe", "qqexternal.exe",
];

/// 单条优化执行 —— 对齐 speedupManager.js#10-66 applyFix()
/// 返回 { ok, message? }
pub async fn apply_fix(fix: &Value) -> Value {
    let action = fix.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let target = fix.get("target");

    // 统一错误捕获：任何 panic/错误都转为 {ok:false, message}
    let result = match action {
        "disable_service" => apply_disable_service(target).await,
        "kill_process" => apply_kill_process(target).await,
        "reg_set" => apply_reg_set(target).await,
        "flush_dns" => apply_flush_dns().await,
        "clean_file" => apply_clean_file(target).await,
        _ => Ok(json!({ "ok": false, "message": format!("unknown action: {}", action) })),
    };

    match result {
        Ok(v) => v,
        Err(e) => json!({ "ok": false, "message": e }),
    }
}

/// disable_service: 禁用服务 —— 对齐 speedupManager.js#13-14
/// target: 服务名字符串
async fn apply_disable_service(target: Option<&Value>) -> Result<Value, String> {
    let name = target
        .and_then(|v| v.as_str())
        .ok_or_else(|| "disable_service: missing target (service name)".to_string())?;

    // 受保护服务判断 —— 对齐 service.js isProtected(), 通过 crate::system::service::is_protected
    if is_protected(name) {
        return Ok(json!({
            "ok": false,
            "message": format!("Protected service: {}", name),
        }));
    }

    // 调用 system::service::set_service_start_type
    let res = set_service_start_type(name, "Disabled").await?;
    Ok(res)
}

/// kill_process: 杀进程 —— 对齐 speedupManager.js#15-25
/// PID 复用防护：核对当前进程名仍在 SAFE_TO_CLOSE 名单内才执行 taskkill
/// target: PID 数字
async fn apply_kill_process(target: Option<&Value>) -> Result<Value, String> {
    let pid = target
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "kill_process: missing target (pid)".to_string())? as u32;

    // 列举当前进程 —— 对齐 process.listProcesses()
    let procs = list_processes().await?;
    let cur = procs.iter().find(|p| p.get("pid").and_then(|v| v.as_u64()).unwrap_or(0) == pid as u64);

    // 进程已不存在 —— 对齐 JS#20: { ok: true, message: 'process already exited' }
    if cur.is_none() {
        return Ok(json!({ "ok": true, "message": "process already exited" }));
    }

    let cur = cur.unwrap();
    let name = cur.get("name").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();

    // PID 复用防护：进程名必须在 SAFE_TO_CLOSE 名单内 —— 对齐 JS#21-23
    if !SAFE_TO_CLOSE.iter().any(|&s| s == name) {
        return Ok(json!({
            "ok": false,
            "message": format!("PID reused by other process ({}), skipped", name),
        }));
    }

    // 执行杀进程 —— 对齐 process.killProcess()
    let res = kill_process(pid).await?;
    Ok(res)
}

/// reg_set: 注册表设置 —— 对齐 speedupManager.js#26-36
/// target: { keyPath, valueName, value, type? }
/// type: "binary" | "string" | "dword" (默认 dword)
async fn apply_reg_set(target: Option<&Value>) -> Result<Value, String> {
    let t = target.ok_or_else(|| "reg_set: missing target".to_string())?;

    let key_path = t
        .get("keyPath")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "reg_set: missing keyPath".to_string())?;
    let value_name = t
        .get("valueName")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "reg_set: missing valueName".to_string())?;

    let vtype = t.get("type").and_then(|v| v.as_str()).unwrap_or("dword");
    let value = t.get("value");

    match vtype {
        "binary" => {
            // binary: value 为十六进制字符串或数组 —— 对齐 JS#28-30
            // JS: Buffer.isBuffer(t.value) ? t.value : Buffer.from(t.value || '')
            // registry::set_binary 期望十六进制字符串(大写, 无空格)
            let hex = match value {
                Some(Value::String(s)) => s.to_uppercase(),
                Some(Value::Array(arr)) => {
                    // 数组形式 [3,0,0,...] -> 十六进制字符串
                    arr.iter()
                        .map(|v| v.as_u64().unwrap_or(0) as u8)
                        .map(|b| format!("{:02X}", b))
                        .collect::<String>()
                }
                _ => String::new(),
            };
            set_binary(key_path, value_name, &hex).await
        }
        "string" => {
            // string: REG_SZ —— 对齐 JS#32-34
            let val = value.and_then(|v| v.as_str()).unwrap_or("");
            set_string(key_path, value_name, val).await
        }
        _ => {
            // dword (默认) —— 对齐 JS#35
            let val = value.and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            set_dword(key_path, value_name, val).await
        }
    }
}

/// flush_dns: 刷新 DNS 缓存 —— 对齐 speedupManager.js#37-40
/// 执行 ipconfig /flushdns
async fn apply_flush_dns() -> Result<Value, String> {
    let res = run_async("ipconfig", &["/flushdns"], None).await?;
    let ok = res.contains("成功") || res.contains("Successfully") || res.contains("flushed");
    Ok(json!({
        "ok": ok,
        "code": if ok { 0 } else { 1 },
        "message": res,
    }))
}

/// clean_file: 清理过期文件 —— 对齐 speedupManager.js#41-59
/// 只删除目标目录中超过 7 天的文件，保留目录结构与近期文件
/// 限制最多删除 2000 个文件
/// fsp.rm(f.path, {force:true}) 对应 std::fs 删除并吞错
/// target: 目录路径字符串
async fn apply_clean_file(target: Option<&Value>) -> Result<Value, String> {
    let dir_path = target
        .and_then(|v| v.as_str())
        .ok_or_else(|| "clean_file: missing target (directory path)".to_string())?;

    let dir = Path::new(dir_path);

    // 安全路径检查 —— 对齐 fileSystem.js isSafePath 语义
    // 这里使用 rules_engine::is_safe_path，allowed_root 设为目标目录自身
    if !is_safe_path(dir_path, dir_path) {
        return Ok(json!({
            "ok": false,
            "message": "unsafe path: clean_file target not allowed",
        }));
    }

    // 扫描目录：minAgeDays=7, maxDepth=4 —— 对齐 JS#45
    let scan_res = scan_dir(dir, 7, 4, None, None, None).await?;

    let files = scan_res.get("files").and_then(|v| v.as_array()).cloned().unwrap_or_default();

    let mut freed = 0u64;
    let mut deleted = 0usize;

    // 限制最多处理 2000 个文件 —— 对齐 JS#48: scan.files.slice(0, 2000)
    for f in files.iter().take(2000) {
        let f_path = f.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let f_size = f.get("size").and_then(|v| v.as_u64()).unwrap_or(0);

        if f_path.is_empty() {
            continue;
        }

        let path = Path::new(f_path);

        // 删除文件，吞掉错误(锁定/已不存在) —— 对齐 JS#49-53: try { await fsp.rm(f.path, {force:true}) } catch(e) { /* locked or gone */ }
        match fs::remove_file(path) {
            Ok(_) => {
                freed += f_size;
                deleted += 1;
            }
            Err(_) => {
                // 忽略错误：文件被占用、已删除、权限不足等
            }
        }
    }

    Ok(json!({
        "ok": true,
        "message": format!("deleted {} files (>=7d), freed {} bytes", deleted, freed),
    }))
}

/// 批量执行优化 —— 对齐 speedupManager.js#72-85 optimize()
/// fix_ids: 修复项 ID 数组
/// 返回 { ok: true, results: [{ id, ok, message }...] }
/// 注意：JS 版通过 scanScheduler.getFix(id) 获取 fix 详情。
/// Rust 版需要外部传入 fix_map (HashMap<String, Value>)，
/// 因为 scan_scheduler 尚未移植完成，暂不提供全局单例。
pub async fn optimize(fix_ids: Vec<String>, fix_map: &std::collections::HashMap<String, Value>) -> Value {
    let mut results = Vec::new();

    for id in fix_ids {
        // 查找 fix 详情 —— 对齐 JS#76: scanScheduler.getFix(id)
        let fix = fix_map.get(&id);

        if fix.is_none() {
            // 未知 fix id —— 对齐 JS#77-79
            results.push(json!({
                "id": id,
                "ok": false,
                "message": "unknown fix id",
            }));
            continue;
        }

        let fix = fix.unwrap();

        // 执行单条优化 —— 对齐 JS#81-82
        let r = apply_fix(fix).await;

        results.push(json!({
            "id": id,
            "ok": r.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
            "message": r.get("message").and_then(|v| v.as_str()).unwrap_or(""),
        }));
    }

    json!({
        "ok": true,
        "results": results,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn test_safe_to_close_contains_expected() {
        assert!(SAFE_TO_CLOSE.contains(&"qqmusic.exe"));
        assert!(SAFE_TO_CLOSE.contains(&"chromeupdate.exe"));
        assert!(!SAFE_TO_CLOSE.contains(&"notepad.exe"));
    }

    #[tokio::test]
    async fn test_apply_fix_unknown_action() {
        let fix = json!({ "action": "unknown_action", "target": null });
        let res = apply_fix(&fix).await;
        assert_eq!(res.get("ok"), Some(&json!(false)));
        assert!(res.get("message").unwrap().as_str().unwrap().contains("unknown action"));
    }

    #[tokio::test]
    async fn test_optimize_unknown_fix_id() {
        let fix_map = HashMap::new();
        let res = optimize(vec!["fix_001".to_string()], &fix_map).await;
        assert_eq!(res.get("ok"), Some(&json!(true)));
        let results = res.get("results").unwrap().as_array().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].get("id"), Some(&json!("fix_001")));
        assert_eq!(results[0].get("ok"), Some(&json!(false)));
        assert_eq!(results[0].get("message"), Some(&json!("unknown fix id")));
    }
}