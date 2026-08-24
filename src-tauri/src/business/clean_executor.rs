//! 清理执行器 —— 对齐 Electron 版 business/cleanExecutor.js
//! 导出: execute_clean, is_white_listed, load_white_list

use crate::rules_engine::is_safe_path;
use crate::store;
use crate::system::filesystem::{empty_recycle_bin, get_dir_size, is_file_locked};
use crate::system::paths::program_files_x86;
use crate::types::send;
use once_cell::sync::Lazy;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::AppHandle;

/// 白名单集合: SHA-256 前缀(64 hex chars) —— 对齐 cleanExecutor.js#11-12
static WHITE_LIST: Lazy<Mutex<Option<HashSet<String>>>> = Lazy::new(|| Mutex::new(None));
/// 白名单桶索引: 前 2 字符 -> Vec<prefix> —— 对齐 cleanExecutor.js#37-43
static WHITE_LIST_INDEX: Lazy<Mutex<Option<HashMap<String, Vec<String>>>>> = Lazy::new(|| Mutex::new(None));

/// 加载 WhiteList.dat —— 对齐 cleanExecutor.js#18-45 loadWhiteList()
/// 返回白名单条目数
pub async fn load_white_list() -> usize {
    // 已加载则直接返回
    if WHITE_LIST.lock().unwrap().is_some() {
        return WHITE_LIST.lock().unwrap().as_ref().unwrap().len();
    }

    let mut set = HashSet::new();
    let candidates = vec![
        PathBuf::from(r"D:\360Safe\sweeper\WhiteList.dat"),
        program_files_x86().join("360").join("360Safe").join("sweeper").join("WhiteList.dat"),
    ];

    for file in candidates {
        if !file.exists() {
            continue;
        }
        match fs::read_to_string(&file) {
            Ok(s) => {
                let hex: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
                // 每 80 字符一条, 取前 64 字符 —— 对齐 cleanExecutor.js#30-31
                for i in (0..hex.len()).step_by(80) {
                    if i + 64 <= hex.len() {
                        set.insert(hex[i..i + 64].to_uppercase());
                    }
                }
                break; // 成功读取一个即停止
            }
            Err(_) => continue,
        }
    }

    // 建立桶索引: 前 2 字符 -> Vec<prefix> —— 对齐 cleanExecutor.js#38-43
    let mut index: HashMap<String, Vec<String>> = HashMap::new();
    for prefix in &set {
        let key = &prefix[0..2];
        index.entry(key.to_string()).or_default().push(prefix.clone());
    }

    *WHITE_LIST.lock().unwrap() = Some(set);
    *WHITE_LIST_INDEX.lock().unwrap() = Some(index);

    WHITE_LIST.lock().unwrap().as_ref().unwrap().len()
}

/// 计算文件 SHA-256 (大写 hex) —— 对齐 cleanExecutor.js#47-55 sha256Hex()
/// 读取失败返回 None
async fn sha256_hex(file_path: &Path) -> Option<String> {
    let file = match fs::File::open(file_path) {
        Ok(f) => f,
        Err(_) => return None,
    };
    let mut hasher = Sha256::new();
    let mut reader = std::io::BufReader::new(file);
    match std::io::copy(&mut reader, &mut hasher) {
        Ok(_) => Some(format!("{:X}", hasher.finalize())),
        Err(_) => None,
    }
}

/// 判断文件是否在白名单中 —— 对齐 cleanExecutor.js#57-68 isWhiteListed()
pub async fn is_white_listed(file_path: &Path) -> bool {
    // 确保白名单已加载
    if WHITE_LIST.lock().unwrap().is_none() {
        load_white_list().await;
    }

    // 只需判空: 提取后立即放锁, MutexGuard 不可跨 .await(否则 future 非 Send)
    let wl_loaded_nonempty = {
        let wl_guard = WHITE_LIST.lock().unwrap();
        matches!(wl_guard.as_ref(), Some(s) if !s.is_empty())
    };
    if !wl_loaded_nonempty {
        return false;
    }

    let hash = match sha256_hex(file_path).await {
        Some(h) => h,
        None => return false,
    };

    let index = WHITE_LIST_INDEX.lock().unwrap();
    let index = match index.as_ref() {
        Some(idx) => idx,
        None => return false,
    };

    let bucket_key = &hash[0..2];
    let bucket = match index.get(bucket_key) {
        Some(b) => b,
        None => return false,
    };

    // 对齐 cleanExecutor.js#64-65: 前缀匹配
    for prefix in bucket {
        if hash.starts_with(prefix) {
            return true;
        }
    }
    false
}

/// 提取盘符字母 —— 对齐 cleanExecutor.js#70-73 extractDrive()
fn extract_drive(path: &str) -> Option<char> {
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        Some(bytes[0].to_ascii_uppercase() as char)
    } else {
        None
    }
}

/// 今天日期字符串 YYYY-MM-DD —— 对齐 cleanExecutor.js#75-80 todayStr()
/// 使用标准库实现的日期计算 (基于 Unix 时间戳), 避免额外依赖
fn today_str() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let mut days = now / 86400; // 天数 since epoch
    
    // 计算年份 (考虑闰年)
    let mut year = 1970;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days >= days_in_year {
            days -= days_in_year;
            year += 1;
        } else {
            break;
        }
    }
    
    // 计算月份和日期
    let month_days = [
        31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31
    ];
    let mut month = 1;
    for &md in &month_days {
        let dim = if month == 2 && is_leap_year(year) { 29 } else { md };
        if days >= dim {
            days -= dim;
            month += 1;
        } else {
            break;
        }
    }
    let day = days + 1; // 1-based
    
    format!("{:04}-{:02}-{:02}", year, month, day)
}

fn is_leap_year(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// 本地回退: 获取组定义 —— 对齐 scanScheduler.getGroupDef()
/// 并行代理移植 scan_scheduler 时会提供真实实现
async fn get_group_def_local(_group_id: &str) -> Option<serde_json::Value> {
    None // 暂时返回 None, 由 scan_scheduler 实现后替换
}

/// 本地回退: 获取扫描文件列表 —— 对齐 scanScheduler.getScanFiles()
/// 并行代理移植 scan_scheduler 时会提供真实实现
async fn get_scan_files_local(_group_id: &str, _item_id: &str) -> Option<serde_json::Value> {
    None // 暂时返回 None, 由 scan_scheduler 实现后替换
}

/// 清理执行入口 —— 对齐 cleanExecutor.js#88-149 executeClean()
/// items: Vec<CleanItemRef> = { group_id, item_id, path, size }
/// 返回 { ok, freed_size, count, failed: [{path, reason}] }
pub async fn execute_clean(app: AppHandle, items: Vec<serde_json::Value>) -> serde_json::Value {
    let list = items;
    let mut result = serde_json::json!({
        "ok": true,
        "freedSize": 0u64,
        "count": 0usize,
        "failed": Vec::<serde_json::Value>::new(),
    });
    let total = list.len();
    let mut current = 0usize;

    for ref_item in list {
        current += 1;

        let group_id = ref_item.get("groupId").and_then(|v| v.as_str()).unwrap_or("");
        let item_id = ref_item.get("itemId").and_then(|v| v.as_str()).unwrap_or("");
        let path_str = ref_item.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let ref_size = ref_item.get("size").and_then(|v| v.as_u64()).unwrap_or(0);

        // 获取组定义 —— 对齐 cleanExecutor.js#95
        let def = get_group_def_local(group_id).await;
        // 获取扫描文件列表 —— 对齐 cleanExecutor.js#96
        let files = get_scan_files_local(group_id, item_id).await;

        let mut freed = 0u64;
        let mut deleted = 0usize;

        // 回收站清空分支 —— 对齐 cleanExecutor.js#99-111
        if let Some(def_val) = &def {
            if def_val.get("action").and_then(|v| v.as_str()) == Some("recycle_bin") {
                if let Some(drive) = extract_drive(path_str) {
                    let drive_str = drive.to_string();
                    match empty_recycle_bin(&drive_str).await {
                        Ok(r) => {
                            if r.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                                freed = ref_size;
                                deleted = 1;
                            } else {
                                result["failed"].as_array_mut().unwrap().push(serde_json::json!({
                                    "path": path_str,
                                    "reason": r.get("message").and_then(|v| v.as_str()).unwrap_or("recycle-bin-failed")
                                }));
                            }
                        }
                        Err(e) => {
                            result["failed"].as_array_mut().unwrap().push(serde_json::json!({
                                "path": path_str,
                                "reason": e
                            }));
                        }
                    }
                } else {
                    result["failed"].as_array_mut().unwrap().push(serde_json::json!({
                        "path": path_str,
                        "reason": "invalid-drive"
                    }));
                }
            }
        }

        // 普通文件/目录删除分支 —— 对齐 cleanExecutor.js#112-137
        if freed == 0 && deleted == 0 {
            let targets: Vec<String> = if let Some(files_val) = &files {
                if let Some(files_arr) = files_val.as_array() {
                    if !files_arr.is_empty() {
                        files_arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()
                    } else {
                        vec![path_str.to_string()]
                    }
                } else {
                    vec![path_str.to_string()]
                }
            } else {
                vec![path_str.to_string()]
            };

            let allowed_root = def.as_ref()
                .and_then(|d| d.get("allowedRoot"))
                .and_then(|v| v.as_str())
                .unwrap_or(path_str);

            for t in &targets {
                // 安全路径检查 —— 对齐 cleanExecutor.js#116-118
                if !is_safe_path(t.as_str(), allowed_root) {
                    result["failed"].as_array_mut().unwrap().push(serde_json::json!({
                        "path": t,
                        "reason": "unsafe-path"
                    }));
                    continue;
                }

                let path_buf = PathBuf::from(&t);

                // 文件锁定检查 —— 对齐 cleanExecutor.js#120-122
                if is_file_locked(&path_buf).await {
                    result["failed"].as_array_mut().unwrap().push(serde_json::json!({
                        "path": t,
                        "reason": "file-locked"
                    }));
                    continue;
                }

                // 白名单检查 —— 对齐 cleanExecutor.js#124-126
                if is_white_listed(&path_buf).await {
                    result["failed"].as_array_mut().unwrap().push(serde_json::json!({
                        "path": t,
                        "reason": "whitelisted"
                    }));
                    continue;
                }

                // 删除操作 —— 对齐 cleanExecutor.js#128-136
                match fs::metadata(&path_buf) {
                    Ok(meta) => {
                        let size = if meta.is_dir() {
                            get_dir_size(&path_buf).await.unwrap_or(0)
                        } else {
                            meta.len()
                        };

                        // 尝试删除: 先 remove_dir_all, 失败再 remove_file
                        let del_result = fs::remove_dir_all(&path_buf)
                            .or_else(|_| fs::remove_file(&path_buf));

                        match del_result {
                            Ok(_) => {
                                freed += size;
                                deleted += 1;
                            }
                            Err(e) => {
                                result["failed"].as_array_mut().unwrap().push(serde_json::json!({
                                    "path": t,
                                    "reason": e.to_string()
                                }));
                            }
                        }
                    }
                    Err(e) => {
                        result["failed"].as_array_mut().unwrap().push(serde_json::json!({
                            "path": t,
                            "reason": e.to_string()
                        }));
                    }
                }
            }
        }

        // 累计结果 —— 对齐 cleanExecutor.js#139-140
        result["freedSize"] = serde_json::json!(result["freedSize"].as_u64().unwrap_or(0) + freed);
        result["count"] = serde_json::json!(result["count"].as_u64().unwrap_or(0) + deleted as u64);

        // 进度推送 —— 对齐 cleanExecutor.js#141-143
        send(&app, "clean:exec-progress", serde_json::json!({
            "current": current,
            "total": total,
            "freedSize": result["freedSize"]
        }));
    }

    // 统计落库 —— 对齐 cleanExecutor.js#145-147
    let freed_size = result["freedSize"].as_u64().unwrap_or(0);
    if freed_size > 0 {
        let size_mb = (freed_size as f64 / 1024.0 / 1024.0 * 100.0).round() / 100.0;
        store::append_history(today_str(), size_mb);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_extract_drive() {
        assert_eq!(extract_drive("C:\\Temp\\test.txt"), Some('C'));
        assert_eq!(extract_drive("D:/folder"), Some('D'));
        assert_eq!(extract_drive("\\\\server\\share"), None);
        assert_eq!(extract_drive("relative/path"), None);
    }

    #[tokio::test]
    async fn test_today_str() {
        let s = today_str();
        assert!(s.len() == 10);
        assert!(s.chars().nth(4) == Some('-'));
        assert!(s.chars().nth(7) == Some('-'));
    }

    #[tokio::test]
    async fn test_sha256_hex() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("test.txt");
        let mut f = File::create(&file).unwrap();
        f.write_all(b"hello world").unwrap();
        drop(f);

        let hash = sha256_hex(&file).await.unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(hash, hash.to_uppercase());
    }
}