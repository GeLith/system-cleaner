//! 文件系统操作 —— 对齐 Electron 版 system/fileSystem.js
//! 目录遍历(realpath 防环+深度/数量上限), 删除, 重命名探测, 回收站, 统计

use crate::system::exec::run_async;
use crate::rules_engine::is_safe_path;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 简单随机数生成(避免引入 fastrand)
fn random_suffix() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed) ^ SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64
}

/// 重命名探测文件锁定 —— 对齐 fileSystem.js#12-28 isFileLocked()
/// 重命名需要 DELETE 权限, 失败视为被占用
pub async fn is_file_locked(file_path: &Path) -> bool {
    if !file_path.exists() {
        return false;
    }
    let dir = file_path.parent().unwrap_or(Path::new("."));
    let probe_name = format!(
        ".lockprobe_{}_{}_{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis(),
        random_suffix()
    );
    let tmp = dir.join(probe_name);
    match fs::rename(file_path, &tmp) {
        Ok(_) => {
            let _ = fs::rename(&tmp, file_path);
            false
        }
        Err(_) => {
            let _ = fs::rename(&tmp, file_path);
            true
        }
    }
}

/// 扫描目录树 —— 对齐 fileSystem.js#39-112 scanDir()
/// opts: minAgeDays, maxDepth(默认10), extFilter, isCancelled, onProgress
/// 返回 { files:[{path,size,mtime}], totalSize, totalCount, skippedRunning }
pub async fn scan_dir(
    dir: &Path,
    min_age_days: u32,
    max_depth: usize,
    ext_filter: Option<&str>,
    is_cancelled: Option<Box<dyn Fn() -> bool + Send + Sync>>,
    on_progress: Option<Box<dyn Fn(Value) + Send + Sync>>,
) -> Result<Value, String> {
    let min_age_ms = min_age_days as u128 * 24 * 3600 * 1000;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
    let mut files = Vec::new();
    let mut total_size = 0u64;
    let mut total_count = 0usize;
    let mut cancelled = false;
    let mut last_progress_at = 0u128;

    // realpath 循环守卫 —— 对齐 fileSystem.js#49-56
    let root_real = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let mut seen_real = HashSet::new();
    seen_real.insert(root_real.to_string_lossy().to_lowercase());

    // 简单的扩展名过滤: 传入如 "tmp|log|bak" 形式, 不用正则
    let ext_filters: Vec<String> = ext_filter.map(|s| s.split('|').map(|x| x.to_lowercase()).collect()).unwrap_or_default();

    fn report_progress(
        on_progress: &Option<Box<dyn Fn(Value) + Send + Sync>>,
        force: bool,
        last_progress_at: &mut u128,
        total_count: usize,
        total_size: u64,
        path: &Path,
    ) {
        if on_progress.is_none() {
            return;
        }
        let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
        if !force && t - *last_progress_at < 400 {
            return;
        }
        *last_progress_at = t;
        if let Some(cb) = on_progress {
            cb(json!({
                "count": total_count,
                "totalSize": total_size,
                "path": path.to_string_lossy().to_string(),
            }));
        }
    }

    async fn walk(
        current: &Path,
        depth: usize,
        max_depth: usize,
        min_age_ms: u128,
        now: u128,
        ext_filters: &[String],
        seen_real: &mut HashSet<String>,
        files: &mut Vec<Value>,
        total_size: &mut u64,
        total_count: &mut usize,
        cancelled: &mut bool,
        is_cancelled: &Option<Box<dyn Fn() -> bool + Send + Sync>>,
        on_progress: &Option<Box<dyn Fn(Value) + Send + Sync>>,
        last_progress_at: &mut u128,
        since_check: &mut usize,
    ) -> Result<(), String> {
        if *cancelled {
            return Ok(());
        }
        if let Some(cb) = is_cancelled {
            if cb() {
                *cancelled = true;
                return Ok(());
            }
        }
        if depth > max_depth {
            return Ok(());
        }

        let entries = fs::read_dir(current).map_err(|e| format!("read_dir failed: {}", e))?;
        let mut since_check_local = *since_check;

        for entry in entries {
            if *cancelled {
                return Ok(());
            }
            // 批量轮询取消标志: 每 50 个条目检查一次 —— 对齐 fileSystem.js#79-84
            since_check_local += 1;
            if since_check_local >= 50 {
                since_check_local = 0;
                if let Some(cb) = is_cancelled {
                    if cb() {
                        *cancelled = true;
                        return Ok(());
                    }
                }
                report_progress(on_progress, false, last_progress_at, *total_count, *total_size, current);
            }

            let entry = entry.map_err(|e| format!("entry failed: {}", e))?;
            let full = entry.path();
            let ft = entry.file_type().map_err(|e| format!("file_type failed: {}", e))?;

            if ft.is_dir() {
                let real = fs::canonicalize(&full).ok();
                if let Some(real_path) = real {
                    let key = real_path.to_string_lossy().to_lowercase();
                    if seen_real.contains(&key) {
                        continue;
                    }
                    seen_real.insert(key);
                } else {
                    continue;
                }
                Box::pin(walk(
                    &full,
                    depth + 1,
                    max_depth,
                    min_age_ms,
                    now,
                    ext_filters,
                    seen_real,
                    files,
                    total_size,
                    total_count,
                    cancelled,
                    is_cancelled,
                    on_progress,
                    last_progress_at,
                    &mut since_check_local,
                )).await?;
                if *cancelled {
                    return Ok(());
                }
            } else if ft.is_file() {
                let meta = fs::metadata(&full).map_err(|e| format!("metadata failed: {}", e))?;
                let mtime = meta.modified().map_err(|e| format!("modified failed: {}", e))?
                    .duration_since(UNIX_EPOCH).unwrap().as_millis();
                if min_age_ms > 0 && now - mtime < min_age_ms {
                    continue;
                }
                if !ext_filters.is_empty() {
                    let ext = full.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
                    if !ext_filters.iter().any(|f| f == &ext) {
                        continue;
                    }
                }
                let size = meta.len();
                files.push(json!({
                    "path": full.to_string_lossy().to_string(),
                    "size": size,
                    "mtime": mtime,
                }));
                *total_size += size;
                *total_count += 1;
            }
        }
        *since_check = since_check_local;
        Ok(())
    }

    let mut since_check = 0;
    walk(
        dir,
        0,
        max_depth,
        min_age_ms,
        now,
        &ext_filters,
        &mut seen_real,
        &mut files,
        &mut total_size,
        &mut total_count,
        &mut cancelled,
        &is_cancelled,
        &on_progress,
        &mut last_progress_at,
        &mut since_check,
    ).await?;

    report_progress(&on_progress, true, &mut last_progress_at, total_count, total_size, dir);

    Ok(json!({
        "files": files,
        "totalSize": total_size,
        "totalCount": total_count,
        "skippedRunning": 0,
    }))
}

/// 计算目录大小 —— 对齐 fileSystem.js#114-144 getDirSize()
pub async fn get_dir_size(dir: &Path) -> Result<u64, String> {
    let root_real = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let mut seen_real = HashSet::new();
    seen_real.insert(root_real.to_string_lossy().to_lowercase());

    async fn walk(current: &Path, seen_real: &mut HashSet<String>) -> Result<u64, String> {
        let entries = fs::read_dir(current).map_err(|e| format!("read_dir failed: {}", e))?;
        let mut sum = 0u64;
        for entry in entries {
            let entry = entry.map_err(|e| format!("entry failed: {}", e))?;
            let full = entry.path();
            let ft = entry.file_type().map_err(|e| format!("file_type failed: {}", e))?;
            if ft.is_dir() {
                let real = fs::canonicalize(&full).ok();
                if let Some(real_path) = real {
                    let key = real_path.to_string_lossy().to_lowercase();
                    if seen_real.contains(&key) {
                        continue;
                    }
                    seen_real.insert(key);
                    sum += Box::pin(walk(&full, seen_real)).await?;
                }
            } else if ft.is_file() {
                let meta = fs::metadata(&full).map_err(|e| format!("metadata failed: {}", e))?;
                sum += meta.len();
            }
        }
        Ok(sum)
    }

    walk(dir, &mut seen_real).await
}

/// 计算目录文件数 —— 对齐 fileSystem.js#146-176 countFiles()
pub async fn count_files(dir: &Path) -> Result<usize, String> {
    let root_real = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let mut seen_real = HashSet::new();
    seen_real.insert(root_real.to_string_lossy().to_lowercase());

    async fn walk(current: &Path, seen_real: &mut HashSet<String>) -> Result<usize, String> {
        let entries = fs::read_dir(current).map_err(|e| format!("read_dir failed: {}", e))?;
        let mut count = 0usize;
        for entry in entries {
            let entry = entry.map_err(|e| format!("entry failed: {}", e))?;
            let full = entry.path();
            let ft = entry.file_type().map_err(|e| format!("file_type failed: {}", e))?;
            if ft.is_dir() {
                let real = fs::canonicalize(&full).ok();
                if let Some(real_path) = real {
                    let key = real_path.to_string_lossy().to_lowercase();
                    if seen_real.contains(&key) {
                        continue;
                    }
                    seen_real.insert(key);
                    count += Box::pin(walk(&full, seen_real)).await?;
                }
            } else if ft.is_file() {
                count += 1;
            }
        }
        Ok(count)
    }

    walk(dir, &mut seen_real).await
}

/// 列举文件夹文件(供粉碎机 UI) —— 对齐 fileSystem.js#184-221 listFolderFiles()
/// maxDepth 默认 12, maxFiles 默认 5000
/// 返回 { files:[{path,name,size}], truncated }
pub async fn list_folder_files(
    folder_path: &Path,
    max_depth: usize,
    max_files: usize,
) -> Result<Value, String> {
    let mut files = Vec::new();
    let mut truncated = false;

    let root_real = match fs::canonicalize(folder_path) {
        Ok(p) => p,
        Err(_) => return Ok(json!({ "files": Vec::<Value>::new(), "truncated": false })),
    };
    let mut seen_real = HashSet::new();
    seen_real.insert(root_real.to_string_lossy().to_lowercase());

    async fn walk(
        dir: &Path,
        depth: usize,
        max_depth: usize,
        max_files: usize,
        seen_real: &mut HashSet<String>,
        files: &mut Vec<Value>,
        truncated: &mut bool,
    ) -> Result<(), String> {
        if depth > max_depth || files.len() >= max_files {
            if files.len() >= max_files {
                *truncated = true;
            }
            return Ok(());
        }
        let entries = fs::read_dir(dir).map_err(|e| format!("read_dir failed: {}", e))?;
        for entry in entries {
            if files.len() >= max_files {
                *truncated = true;
                break;
            }
            let entry = entry.map_err(|e| format!("entry failed: {}", e))?;
            let full = entry.path();
            let ft = entry.file_type().map_err(|e| format!("file_type failed: {}", e))?;
            if ft.is_file() {
                let meta = fs::metadata(&full).map_err(|e| format!("metadata failed: {}", e))?;
                files.push(json!({
                    "path": full.to_string_lossy().to_string(),
                    "name": entry.file_name().to_string_lossy().to_string(),
                    "size": meta.len(),
                }));
            } else if ft.is_dir() {
                let real = fs::canonicalize(&full).ok();
                if let Some(real_path) = real {
                    let key = real_path.to_string_lossy().to_lowercase();
                    if seen_real.contains(&key) {
                        continue;
                    }
                    seen_real.insert(key);
                    Box::pin(walk(&full, depth + 1, max_depth, max_files, seen_real, files, truncated)).await?;
                }
            }
        }
        Ok(())
    }

    walk(&root_real, 0, max_depth, max_files, &mut seen_real, &mut files, &mut truncated).await?;
    Ok(json!({ "files": files, "truncated": truncated }))
}

/// 安全删除 —— 对齐 fileSystem.js#228-251 safeDelete()
/// 通过 isSafePath 守卫关键路径, 跳过锁定文件
/// 返回 { freed, deleted, failed:[{path,reason}], running }
pub async fn safe_delete(paths: Vec<PathBuf>) -> Result<Value, String> {
    let mut freed = 0u64;
    let mut deleted = 0usize;
    let mut failed = Vec::new();
    let mut running = 0usize;

    for p in paths {
        // 安全路径检查 —— 对齐 fileSystem.js#232-235
        if !is_safe_path(&p.to_string_lossy(), &p.to_string_lossy()) {
            failed.push(json!({ "path": p.to_string_lossy().to_string(), "reason": "unsafe-path" }));
            continue;
        }
        // 锁定检查
        if is_file_locked(&p).await {
            running += 1;
            continue;
        }
        // 删除
        match fs::metadata(&p) {
            Ok(meta) => {
                let size = if meta.is_dir() {
                    get_dir_size(&p).await.unwrap_or(0)
                } else {
                    meta.len()
                };
                if let Err(e) = fs::remove_dir_all(&p).or_else(|_| fs::remove_file(&p)) {
                    failed.push(json!({ "path": p.to_string_lossy().to_string(), "reason": e.to_string() }));
                } else {
                    freed += size;
                    deleted += 1;
                }
            }
            Err(e) => {
                failed.push(json!({ "path": p.to_string_lossy().to_string(), "reason": e.to_string() }));
            }
        }
    }

    Ok(json!({
        "freed": freed,
        "deleted": deleted,
        "failed": failed,
        "running": running,
    }))
}

/// 移动到回收站 —— 对齐 fileSystem.js#256-261 deleteWithRecycleBin()
/// 通过 PowerShell Microsoft.VisualBasic.FileIO.FileSystem.DeleteFile
pub async fn delete_with_recycle_bin(file_path: &Path) -> Result<Value, String> {
    let safe = file_path.to_string_lossy().replace('\'', "''");
    let script = format!(
        "Add-Type -AssemblyName Microsoft.VisualBasic; [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile('{}','OnlyErrorDialogs','SendToRecycleBin')",
        safe
    );
    let res = run_async("powershell", &["-NoProfile", "-NonInteractive", "-Command", &script], None).await?;
    let ok = !res.contains("Exception") && !res.contains("Error");
    Ok(json!({
        "ok": ok,
        "code": if ok { 0 } else { 1 },
        "message": res,
    }))
}

/// 清空回收站 —— 对齐 fileSystem.js#266-270 emptyRecycleBin()
pub async fn empty_recycle_bin(drive_letter: &str) -> Result<Value, String> {
    let script = format!(
        "Clear-RecycleBin -DriveLetter {} -Force -ErrorAction SilentlyContinue",
        drive_letter
    );
    let res = run_async("powershell", &["-NoProfile", "-NonInteractive", "-Command", &script], None).await?;
    let ok = !res.contains("Exception") && !res.contains("Error");
    Ok(json!({
        "ok": ok,
        "code": if ok { 0 } else { 1 },
        "message": res,
    }))
}

/// 单次遍历获取目录大小+文件数 —— 对齐 fileSystem.js#276-308 getDirStats()
/// 返回 { size, count }
pub async fn get_dir_stats(dir: &Path) -> Result<Value, String> {
    let root_real = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let mut seen_real = HashSet::new();
    seen_real.insert(root_real.to_string_lossy().to_lowercase());

    async fn walk(current: &Path, seen_real: &mut HashSet<String>) -> Result<(u64, usize), String> {
        let entries = fs::read_dir(current).map_err(|e| format!("read_dir failed: {}", e))?;
        let mut size = 0u64;
        let mut count = 0usize;
        for entry in entries {
            let entry = entry.map_err(|e| format!("entry failed: {}", e))?;
            let full = entry.path();
            let ft = entry.file_type().map_err(|e| format!("file_type failed: {}", e))?;
            if ft.is_dir() {
                let real = fs::canonicalize(&full).ok();
                if let Some(real_path) = real {
                    let key = real_path.to_string_lossy().to_lowercase();
                    if seen_real.contains(&key) {
                        continue;
                    }
                    seen_real.insert(key);
                    let (s, c) = Box::pin(walk(&full, seen_real)).await?;
                    size += s;
                    count += c;
                }
            } else if ft.is_file() {
                let meta = fs::metadata(&full).map_err(|e| format!("metadata failed: {}", e))?;
                size += meta.len();
                count += 1;
            }
        }
        Ok((size, count))
    }

    let (size, count) = walk(dir, &mut seen_real).await?;
    Ok(json!({ "size": size, "count": count }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_is_file_locked() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("test.txt");
        let mut f = File::create(&file).unwrap();
        f.write_all(b"hello").unwrap();
        drop(f);
        assert!(!is_file_locked(&file).await);
    }

    #[tokio::test]
    async fn test_get_dir_size() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("test.txt");
        let mut f = File::create(&file).unwrap();
        f.write_all(b"hello world").unwrap();
        drop(f);
        let size = get_dir_size(dir.path()).await.unwrap();
        assert_eq!(size, 11);
    }
}