//! 清理执行器 —— 对齐 Electron 版 business/cleanExecutor.js
//! 导出: execute_clean, is_white_listed, load_white_list

use crate::rules_engine::{is_protected_user_dir, is_safe_path};
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

/// 获取组定义 —— 读取扫描阶段登记的真实定义 (修复: 原为永远返回 None 的桩,
/// 导致回收站分支永不执行、allowedRoot 退化为目标自身、安全检查失效)
async fn get_group_def_local(group_id: &str) -> Option<serde_json::Value> {
    crate::business::scan_scheduler::get_group_def(group_id)
}

/// 获取扫描文件清单 —— 读取扫描阶段登记的真实文件列表 (修复: 原桩返回 None,
/// 导致执行器退化为对扫描根目录整体 remove_dir_all —— 下载文件夹被删的直接原因)
async fn get_scan_files_local(group_id: &str, item_id: &str) -> Option<Vec<String>> {
    crate::business::scan_scheduler::get_scan_files(group_id, item_id)
}

/// 删除后修剪根目录下的空目录 (自底向上, 永不删除根自身)
fn prune_empty_dirs(dir: &Path, root: &Path) {
    if let Ok(rd) = fs::read_dir(dir) {
        for entry in rd.flatten() {
            if entry.path().is_dir() {
                prune_empty_dirs(&entry.path(), root);
            }
        }
    }
    if dir != root {
        // 只删空目录; 忽略一切错误(非空/占用)
        let _ = fs::remove_dir(dir);
    }
}

/// 清理执行核心 (无 AppHandle, 可单测)
/// items: [{groupId, itemId, path, size}]
/// 返回 { ok, freedSize, count, skipped, failed:[{path,reason}] }
///
/// 删除语义 (v1.0.1 修复):
/// - 优先按扫描登记的真实文件清单逐个删除, 绝不整体删除扫描根目录
/// - 目录目标仅在清单中明确列出时才允许 remove_dir_all (如浏览器扩展目录)
/// - 无清单时: 文件可删; 目录一律拒绝 (root-nuke 防线)
/// - 受保护用户目录 (下载/桌面/文档等) 一律拒绝
pub async fn execute_clean_core(
    items: Vec<serde_json::Value>,
    progress: Option<&(dyn Fn(serde_json::Value) + Send + Sync)>,
) -> serde_json::Value {
    let mut result = serde_json::json!({
        "ok": true,
        "freedSize": 0u64,
        "count": 0usize,
        "skipped": 0u64,
        "failed": Vec::<serde_json::Value>::new(),
    });
    let total = items.len();

    for (idx, ref_item) in items.iter().enumerate() {
        let group_id = ref_item.get("groupId").and_then(|v| v.as_str()).unwrap_or("");
        let item_id = ref_item.get("itemId").and_then(|v| v.as_str()).unwrap_or("");
        let path_str = ref_item.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let ref_size = ref_item.get("size").and_then(|v| v.as_u64()).unwrap_or(0);

        let def = get_group_def_local(group_id).await;
        let action = def
            .as_ref()
            .and_then(|d| d.get("action"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let files = get_scan_files_local(group_id, item_id).await;

        let mut freed = 0u64;
        let mut deleted = 0usize;
        let mut skipped = 0usize;

        // 仅列出组 (如大文件): 永不自动删除
        if action == "none" {
            skipped += 1;
        } else if action == "recycle_bin" {
            // 回收站清空分支 —— def 现已可用, 分支可正常进入
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
        } else {
            // 普通删除分支
            let allowed_root = def
                .as_ref()
                .and_then(|d| d.get("allowedRoot"))
                .and_then(|v| v.as_str())
                .unwrap_or(path_str);

            // 聚合路径本身是受保护用户目录 → 拒绝 (root-nuke 防线)
            if is_protected_user_dir(path_str) {
                result["failed"].as_array_mut().unwrap().push(serde_json::json!({
                    "path": path_str,
                    "reason": "protected-dir"
                }));
            } else {
                let files_list: Option<Vec<String>> =
                    files.filter(|v| !v.is_empty());
                let targets: Vec<String> = match &files_list {
                    Some(v) => v.clone(),
                    None => vec![path_str.to_string()],
                };

                for t in &targets {
                    // 受保护用户目录: 单条目标同样拒绝
                    if is_protected_user_dir(t) {
                        result["failed"].as_array_mut().unwrap().push(serde_json::json!({
                            "path": t,
                            "reason": "protected-dir"
                        }));
                        continue;
                    }
                    // 安全路径检查 (allowedRoot 现在来自真实组定义)
                    if !is_safe_path(t.as_str(), allowed_root) {
                        result["failed"].as_array_mut().unwrap().push(serde_json::json!({
                            "path": t,
                            "reason": "unsafe-path"
                        }));
                        continue;
                    }

                    let path_buf = PathBuf::from(t);

                    // 文件锁定检查
                    if is_file_locked(&path_buf).await {
                        result["failed"].as_array_mut().unwrap().push(serde_json::json!({
                            "path": t,
                            "reason": "file-locked"
                        }));
                        continue;
                    }

                    // 白名单检查
                    if is_white_listed(&path_buf).await {
                        result["failed"].as_array_mut().unwrap().push(serde_json::json!({
                            "path": t,
                            "reason": "whitelisted"
                        }));
                        continue;
                    }

                    match fs::metadata(&path_buf) {
                        Ok(meta) => {
                            let size = if meta.is_dir() {
                                get_dir_size(&path_buf).await.unwrap_or(0)
                            } else {
                                meta.len()
                            };

                            // 目录目标: 仅当扫描清单明确包含该目录时才允许整体删除;
                            // 否则一律拒绝 (防止把扫描根目录整个 remove_dir_all)
                            let del_result = if meta.is_dir() {
                                let in_list = files_list
                                    .as_ref()
                                    .map(|l| l.iter().any(|x| x == t))
                                    .unwrap_or(false);
                                if !in_list {
                                    result["failed"].as_array_mut().unwrap().push(serde_json::json!({
                                        "path": t,
                                        "reason": "dir-not-in-scan-list"
                                    }));
                                    continue;
                                }
                                fs::remove_dir_all(&path_buf)
                            } else {
                                fs::remove_file(&path_buf)
                            };

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
                        Err(_) => {
                            // 扫描后文件已消失 (被其他程序删除等): 记为跳过而非失败
                            skipped += 1;
                        }
                    }
                }

                // 修剪根目录下因文件删除而空掉的子目录 (永不删除根自身)
                if deleted > 0 {
                    if let Some(d) = &def {
                        if let Some(dirs) = d.get("dirs").and_then(|v| v.as_array()) {
                            for ds in dirs.iter().filter_map(|x| x.as_str()) {
                                let root = PathBuf::from(ds);
                                if root.exists() {
                                    prune_empty_dirs(&root, &root);
                                }
                            }
                        }
                    }
                }
            }
        }

        // 累计结果
        result["freedSize"] = serde_json::json!(result["freedSize"].as_u64().unwrap_or(0) + freed);
        result["count"] = serde_json::json!(result["count"].as_u64().unwrap_or(0) + deleted as u64);
        result["skipped"] =
            serde_json::json!(result["skipped"].as_u64().unwrap_or(0) + skipped as u64);

        // 进度回调
        if let Some(cb) = progress {
            cb(serde_json::json!({
                "current": idx + 1,
                "total": total,
                "freedSize": result["freedSize"]
            }));
        }
    }

    // 统计落库
    let freed_size = result["freedSize"].as_u64().unwrap_or(0);
    if freed_size > 0 {
        let size_mb = (freed_size as f64 / 1024.0 / 1024.0 * 100.0).round() / 100.0;
        store::append_history(today_str(), size_mb);
    }

    result
}

/// 清理执行入口 —— 包装核心, 附带进度事件推送
pub async fn execute_clean(app: AppHandle, items: Vec<serde_json::Value>) -> serde_json::Value {
    let app2 = app.clone();
    let cb = move |p: serde_json::Value| {
        send(&app2, "clean:exec-progress", p);
    };
    execute_clean_core(items, Some(&cb)).await
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

    // ===== v1.0.1 清理语义修复回归 =====

    /// T1: 有扫描清单时逐文件删除, 根目录与其余文件保留
    #[tokio::test]
    async fn test_files_list_semantics_never_nukes_root() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let old_file = root.join("old_installer.exe");
        let keep_file = root.join("my_data.txt");
        File::create(&old_file).unwrap().write_all(b"xxx").unwrap();
        File::create(&keep_file).unwrap().write_all(b"keep me").unwrap();

        crate::business::scan_scheduler::record_group_def(serde_json::json!({
            "groupId": "qa_group", "action": "delete",
            "allowedRoot": root.to_string_lossy(), "dirs": [root.to_string_lossy()]
        }));
        crate::business::scan_scheduler::record_scan_files(
            "qa_group",
            "it_qa",
            vec![old_file.to_string_lossy().to_string()],
        );

        let items = vec![serde_json::json!({
            "groupId": "qa_group", "itemId": "it_qa",
            "path": root.to_string_lossy(), "size": 3
        })];
        let r = execute_clean_core(items, None).await;

        assert_eq!(r["ok"], serde_json::json!(true));
        assert_eq!(r["count"], serde_json::json!(1));
        assert!(!old_file.exists(), "清单内文件应被删除");
        assert!(keep_file.exists(), "清单外文件必须保留");
        assert!(root.exists(), "扫描根目录绝不能被整体删除");
    }

    /// T2: 无扫描清单的目录目标 → 拒绝删除 (root-nuke 防线)
    #[tokio::test]
    async fn test_dir_without_scan_list_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let f = root.join("important.bin");
        File::create(&f).unwrap().write_all(b"data").unwrap();
        // 故意不登记任何扫描清单

        crate::business::scan_scheduler::record_group_def(serde_json::json!({
            "groupId": "qa_nolist", "action": "delete",
            "allowedRoot": root.to_string_lossy()
        }));
        let items = vec![serde_json::json!({
            "groupId": "qa_nolist", "itemId": "it_x",
            "path": root.to_string_lossy(), "size": 4
        })];
        let r = execute_clean_core(items, None).await;

        assert!(f.exists(), "无清单时目录内容绝不能被删除");
        assert!(root.exists());
        let failed = r["failed"].as_array().unwrap();
        assert!(failed.iter().any(|x| x["reason"] == "dir-not-in-scan-list"
            || x["reason"] == "unsafe-path"
            || x["reason"] == "protected-dir"),
            "应有拒绝原因, 实际: {failed:?}");
    }

    /// T3: 受保护用户目录(下载夹) → 拒绝且目录完好
    #[tokio::test]
    async fn test_protected_user_dir_refused() {
        let Some(dl) = crate::system::paths::downloads().await.ok().flatten() else {
            return; // 本机无下载目录则跳过
        };
        assert!(crate::rules_engine::is_protected_user_dir(&dl.to_string_lossy()),
            "真实下载目录必须被判定为受保护");

        crate::business::scan_scheduler::record_group_def(serde_json::json!({
            "groupId": "qa_protected", "action": "delete",
            "allowedRoot": dl.to_string_lossy()
        }));
        let items = vec![serde_json::json!({
            "groupId": "qa_protected", "itemId": "it_dl",
            "path": dl.to_string_lossy(), "size": 0
        })];
        let r = execute_clean_core(items, None).await;

        assert!(dl.exists(), "下载目录必须完好无损");
        let failed = r["failed"].as_array().unwrap();
        assert!(failed.iter().any(|x| x["reason"] == "protected-dir"),
            "必须以 protected-dir 拒绝, 实际: {failed:?}");
    }

    /// T5: 清单中明确列出的目录 (如浏览器扩展目录) 允许整体删除
    #[tokio::test]
    async fn test_dir_in_scan_list_is_removable() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let ext_dir = root.join("some_extension_dir");
        std::fs::create_dir_all(&ext_dir).unwrap();
        File::create(ext_dir.join("f.js")).unwrap().write_all(b"js").unwrap();

        crate::business::scan_scheduler::record_group_def(serde_json::json!({
            "groupId": "qa_ext", "action": "delete",
            "allowedRoot": root.to_string_lossy()
        }));
        crate::business::scan_scheduler::record_scan_files(
            "qa_ext",
            "it_ext",
            vec![ext_dir.to_string_lossy().to_string()],
        );
        let items = vec![serde_json::json!({
            "groupId": "qa_ext", "itemId": "it_ext",
            "path": root.to_string_lossy(), "size": 2
        })];
        let r = execute_clean_core(items, None).await;

        assert_eq!(r["count"], serde_json::json!(1));
        assert!(!ext_dir.exists(), "清单内目录应被整体删除");
        assert!(root.exists(), "根目录保留");
    }

    /// T6: list-only 组 (action=none) 跳过不删
    #[tokio::test]
    async fn test_list_only_group_skipped() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("big.bin");
        File::create(&f).unwrap().write_all(b"big").unwrap();

        crate::business::scan_scheduler::record_group_def(serde_json::json!({
            "groupId": "qa_listonly", "action": "none"
        }));
        let items = vec![serde_json::json!({
            "groupId": "qa_listonly", "itemId": "it_b",
            "path": f.to_string_lossy(), "size": 3
        })];
        let r = execute_clean_core(items, None).await;

        assert!(f.exists(), "list-only 组绝不能删除文件");
        assert_eq!(r["skipped"], serde_json::json!(1));
    }
}