//! 注册表操作 —— 对齐 Electron 版 system/registry.js
//! 通过 reg.exe 子进程实现, 复用 exec::run_async 保证解码/超时语义一致

use crate::system::exec::run_async;
use serde_json::{json, Value};

/// 常用 Run 键路径 —— 对齐 registry.js#3-8 RUN_KEYS
pub const HKCU_RUN: &str = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
pub const HKLM_RUN: &str = "HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
pub const HKCU_RUNONCE: &str = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce";
pub const HKLM_RUNONCE: &str = "HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce";

/// RUN_KEYS 映射表, 供上层按名称引用
pub fn run_keys() -> Value {
    json!({
        "hkcu_run": HKCU_RUN,
        "hklm_run": HKLM_RUN,
        "hkcu_runonce": HKCU_RUNONCE,
        "hklm_runonce": HKLM_RUNONCE,
    })
}

const ROOT_MAP: &[(&str, &str)] = &[
    ("HKLM", "HKEY_LOCAL_MACHINE"),
    ("HKCU", "HKEY_CURRENT_USER"),
    ("HKCR", "HKEY_CLASSES_ROOT"),
    ("HKU", "HKEY_USERS"),
    ("HKCC", "HKEY_CURRENT_CONFIG"),
];

/// 规范化根键名: HKLM -> HKEY_LOCAL_MACHINE 等 —— 对齐 registry.js#35-39 normalizeRoot()
fn normalize_root(key_path: &str) -> String {
    for (short, long) in ROOT_MAP {
        if let Some(rest) = key_path.strip_prefix(&format!("{}\\", short)) {
            return format!("{}\\{}", long, rest);
        }
    }
    key_path.to_string()
}

/// 解析 reg query 单行输出: "    name    REG_SZ    value"
/// 手工解析避免引入 regex 依赖 —— 对齐 JS 版 /^\s+(.+?)\s+(REG_\w+)\s*(.*)$/
fn parse_value_line(line: &str) -> Option<(String, String, String)> {
    let line = line.trim_start();
    if line.is_empty() {
        return None;
    }
    // 找到第一个空白序列分隔 name 和 type
    let mut parts = line.split_whitespace();
    let name = parts.next()?.to_string();
    let vtype = parts.next()?.to_string();
    if !vtype.starts_with("REG_") {
        return None;
    }
    // 剩余部分作为 value(可能包含空格), 需要从原行中定位
    // 简单做法: 在原行中找到 vtype 后的内容
    let vtype_pos = line.find(&vtype)?;
    let value = line[vtype_pos + vtype.len()..].trim().to_string();
    Some((name, vtype, value))
}

/// 查询单个值 —— 对齐 registry.js#14-25 queryValue()
/// 返回 { name, type, value } 或 null(不存在/报错)
pub async fn query_value(key_path: &str, value_name: &str) -> Result<Option<Value>, String> {
    let res = run_async("reg", &["query", key_path, "/v", value_name], None).await?;
    // reg query 返回 code=1 表示未找到
    let lines: Vec<&str> = res.lines().collect();
    for line in lines {
        if let Some((name, vtype, value)) = parse_value_line(line) {
            if name == value_name {
                return Ok(Some(json!({ "name": name, "type": vtype, "value": value })));
            }
        }
    }
    Ok(None)
}

/// 列举直接子键 —— 对齐 registry.js#44-57 querySubKeys()
/// 返回子键相对路径数组
pub async fn query_sub_keys(key_path: &str) -> Result<Vec<String>, String> {
    let res = run_async("reg", &["query", key_path], None).await?;
    let full_root = normalize_root(key_path);
    let mut keys = Vec::new();
    for line in res.lines() {
        let t = line.trim();
        if t.starts_with("HKEY_") && t != full_root && t.starts_with(&format!("{}\\", full_root)) {
            keys.push(t[full_root.len() + 1..].to_string());
        }
    }
    Ok(keys)
}

/// 列举键下所有值 —— 对齐 registry.js#62-72 listValues()
/// 返回 [{ name, type, value }]
pub async fn list_values(key_path: &str) -> Result<Vec<Value>, String> {
    let res = run_async("reg", &["query", key_path], None).await?;
    let mut values = Vec::new();
    for line in res.lines() {
        if let Some((name, vtype, value)) = parse_value_line(line) {
            values.push(json!({ "name": name, "type": vtype, "value": value }));
        }
    }
    Ok(values)
}

/// 删除值 —— 对齐 registry.js#74-77 deleteValue()
/// 返回 { ok, code, message }
pub async fn delete_value(key_path: &str, value_name: &str) -> Result<Value, String> {
    let res = run_async("reg", &["delete", key_path, "/v", value_name, "/f"], None).await?;
    Ok(json!({
        "ok": true,
        "code": 0,
        "message": res,
    }))
}

/// 确保键存在(创建) —— 对齐 registry.js#79-82 ensureKey()
pub async fn ensure_key(key_path: &str) -> Result<Value, String> {
    let res = run_async("reg", &["add", key_path, "/f"], None).await?;
    Ok(json!({
        "ok": true,
        "code": 0,
        "message": res,
    }))
}

/// 设置 DWORD 值 —— 对齐 registry.js#84-87 setDword()
pub async fn set_dword(key_path: &str, value_name: &str, value: u32) -> Result<Value, String> {
    let res = run_async("reg", &["add", key_path, "/v", value_name, "/t", "REG_DWORD", "/d", &value.to_string(), "/f"], None).await?;
    Ok(json!({
        "ok": true,
        "code": 0,
        "message": res,
    }))
}

/// 设置字符串值(REG_SZ) —— 对齐 registry.js#89-92 setString()
pub async fn set_string(key_path: &str, value_name: &str, value: &str) -> Result<Value, String> {
    let res = run_async("reg", &["add", key_path, "/v", value_name, "/t", "REG_SZ", "/d", value, "/f"], None).await?;
    Ok(json!({
        "ok": true,
        "code": 0,
        "message": res,
    }))
}

/// 设置二进制值(REG_BINARY) —— 对齐 registry.js#94-98 setBinary()
/// buffer 以十六进制字符串传入(大写, 无空格)
pub async fn set_binary(key_path: &str, value_name: &str, hex: &str) -> Result<Value, String> {
    let res = run_async("reg", &["add", key_path, "/v", value_name, "/t", "REG_BINARY", "/d", hex, "/f"], None).await?;
    Ok(json!({
        "ok": true,
        "code": 0,
        "message": res,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_values_hkcu_run() {
        let res = list_values(HKCU_RUN).await;
        assert!(res.is_ok());
        // 只验证结构, 不依赖具体环境
        let vals = res.unwrap();
        for v in vals {
            assert!(v.get("name").is_some());
            assert!(v.get("type").is_some());
            assert!(v.get("value").is_some());
        }
    }
}