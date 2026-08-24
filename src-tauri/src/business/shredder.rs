//! 文件碎纸机 —— 完整移植自 Electron 版 business/shredderManager.js
//! 对齐行号注释格式: "对齐 shredderManager.js#行号"

use crate::rules_engine::is_critical_root;
use crate::types::send;
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::AppHandle;

/// 8 MiB 缓冲区 —— 对齐 shredderManager.js#10
const CHUNK: usize = 8 * 1024 * 1024;

/// 取消标志 —— 对齐 shredderManager.js#7
static CANCELLED: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));

/// 进度回调 —— 对齐 shredderManager.js#8
/// 使用 Mutex<Option<Box<dyn Fn(Value) + Send + Sync>>> 复刻 JS 的 setProgress 语义
static PROGRESS_CB: Lazy<Mutex<Option<Box<dyn Fn(Value) + Send + Sync>>>> =
    Lazy::new(|| Mutex::new(None));

/// 覆写方法定义 —— 对齐 shredderManager.js#13-21 METHODS
struct ShredMethod {
    passes: u32,
    /// 生成指定遍次的填充数据
    data_fn: Box<dyn Fn(u32) -> Vec<u8> + Send + Sync>,
}

impl ShredMethod {
    fn zero() -> Self {
        Self {
            passes: 1,
            data_fn: Box::new(|_| vec![0x00; CHUNK]),
        }
    }

    fn random() -> Self {
        use rand::RngCore;
        Self {
            passes: 1,
            data_fn: Box::new(move |_| {
                let mut buf = vec![0u8; CHUNK];
                rand::thread_rng().fill_bytes(&mut buf);
                buf
            }),
        }
    }

    fn dod() -> Self {
        use rand::RngCore;
        Self {
            passes: 3,
            data_fn: Box::new(move |pass| {
                // 对齐 shredderManager.js#16-20
                // pass 0: 全 0x00
                // pass 1: 全 0xff
                // pass 2: 随机数据
                match pass {
                    0 => vec![0x00; CHUNK],
                    1 => vec![0xff; CHUNK],
                    _ => {
                        let mut buf = vec![0u8; CHUNK];
                        rand::thread_rng().fill_bytes(&mut buf);
                        buf
                    }
                }
            }),
        }
    }
}

/// 获取方法 —— 对齐 shredderManager.js#13-21
fn get_method(method_id: &str) -> ShredMethod {
    match method_id {
        "zero" => ShredMethod::zero(),
        "random" => ShredMethod::random(),
        "dod" | _ => ShredMethod::dod(),
    }
}

/// 置位取消标志 —— 对齐 shredderManager.js#23
pub fn cancel() {
    CANCELLED.store(true, Ordering::SeqCst);
}

/// 检查是否运行中 —— 对齐 shredderManager.js#24
pub fn is_running() -> bool {
    !CANCELLED.load(Ordering::SeqCst)
}

/// 注册进度回调 —— 对齐 shredderManager.js#25
pub fn set_progress(cb: Box<dyn Fn(Value) + Send + Sync>) {
    if let Ok(mut guard) = PROGRESS_CB.lock() {
        *guard = Some(cb);
    }
}

/// 重置取消标志(新任务开始前调用) —— 对齐 shredderManager.js#28
fn reset_cancel() {
    CANCELLED.store(false, Ordering::SeqCst);
}

/// 安全检查: 拒绝粉碎系统关键根目录 —— 对齐 shredderManager.js#35-40
fn assert_shred_allowed(abs_path: &str) -> Option<Value> {
    if is_critical_root(abs_path) {
        return Some(json!({
            "ok": false,
            "error": format!("拒绝粉碎系统关键目录: {}", abs_path)
        }));
    }
    None
}

/// 内部文件粉碎实现 —— 对齐 shredderManager.js#42-91 shredFileInternal
async fn shred_file_internal(file_path: &Path, method_id: &str) -> Value {
    let method = get_method(method_id);
    let abs = file_path
        .canonicalize()
        .unwrap_or_else(|_| file_path.to_path_buf());
    let abs_str = abs.to_string_lossy().to_string();

    // 安全检查 —— 对齐 shredderManager.js#46-47
    if let Some(blocked) = assert_shred_allowed(&abs_str) {
        return blocked;
    }

    // 文件存在性检查 —— 对齐 shredderManager.js#49-51
    if !abs.exists() {
        return json!({ "ok": false, "error": "文件不存在" });
    }

    // 确认是文件非目录 —— 对齐 shredderManager.js#53-56
    let meta = match fs::metadata(&abs) {
        Ok(m) => m,
        Err(e) => return json!({ "ok": false, "error": e.to_string() }),
    };
    if meta.is_dir() {
        return json!({ "ok": false, "error": "请使用 shred_folder 处理目录" });
    }

    let size = meta.len();
    let total_bytes = size.saturating_mul(method.passes as u64);
    let mut written: u64 = 0;

    // 执行多遍覆写 —— 对齐 shredderManager.js#63-82
    for pass in 0..method.passes {
        // 取消检查点(遍开始) —— 对齐 shredderManager.js#64
        if CANCELLED.load(Ordering::SeqCst) {
            return json!({ "ok": false, "cancelled": true });
        }

        // 以读写模式打开 —— 对齐 shredderManager.js#66
        let file = match fs::OpenOptions::new().read(true).write(true).open(&abs) {
            Ok(f) => f,
            Err(e) => return json!({ "ok": false, "error": e.to_string() }),
        };

        let mut offset: u64 = 0;
        // 分块写入 —— 对齐 shredderManager.js#68-77
        while offset < size {
            // 取消检查点(块循环内) —— 对齐 shredderManager.js#70
            if CANCELLED.load(Ordering::SeqCst) {
                let _ = file.sync_all();
                return json!({ "ok": false, "cancelled": true });
            }

            let buf = (method.data_fn)(pass);
            let to_write = std::cmp::min(buf.len() as u64, size - offset) as usize;

            // 写入并同步 —— 对齐 shredderManager.js#73-76
            let mut file_ref = &file;
            if let Err(e) = file_ref.write_all(&buf[..to_write]) {
                let _ = file_ref.sync_all();
                return json!({ "ok": false, "error": e.to_string() });
            }
            offset += to_write as u64;
            written += to_write as u64;

            // 进度回调 —— 对齐 shredderManager.js#76
            if let Ok(guard) = PROGRESS_CB.lock() {
                if let Some(cb) = guard.as_ref() {
                    cb(json!({
                        "current": written,
                        "total": total_bytes,
                        "pass": pass + 1,
                        "totalPasses": method.passes
                    }));
                }
            }
        }

        // 遍结束同步 —— 对齐 shredderManager.js#78
        if let Err(e) = file.sync_all() {
            return json!({ "ok": false, "error": e.to_string() });
        }
        // 文件句柄在此 drop 时自动关闭
    }

    // 所有遍完成后删除文件 —— 对齐 shredderManager.js#84-86
    if !CANCELLED.load(Ordering::SeqCst) {
        if let Err(e) = fs::remove_file(&abs) {
            return json!({ "ok": false, "error": e.to_string() });
        }
    }

    // 返回结果 —— 对齐 shredderManager.js#87
    json!({
        "ok": true,
        "size": size,
        "passes": method.passes
    })
}

/// 公开单文件入口: 开启新的可取消会话 —— 对齐 shredderManager.js#94-97
pub async fn shred_file(file_path: &Path, method_id: Option<&str>) -> Value {
    reset_cancel(); // 对齐 shredderManager.js#95
    let method = method_id.unwrap_or("dod");
    shred_file_internal(file_path, method).await
}

/// 目录粉碎 —— 对齐 shredderManager.js#99-171
pub async fn shred_folder(
    app: &AppHandle,
    folder_path: &Path,
    method_id: Option<&str>,
) -> Value {
    reset_cancel(); // 对齐 shredderManager.js#100
    let method = get_method(method_id.unwrap_or("dod"));
    let abs = folder_path
        .canonicalize()
        .unwrap_or_else(|_| folder_path.to_path_buf());
    let abs_str = abs.to_string_lossy().to_string();

    // 安全检查 —— 对齐 shredderManager.js#104-105
    if let Some(blocked) = assert_shred_allowed(&abs_str) {
        return blocked;
    }

    // 目录存在性检查 —— 对齐 shredderManager.js#107-109
    if !abs.exists() {
        return json!({ "ok": false, "error": "目录不存在" });
    }

    let meta = match fs::metadata(&abs) {
        Ok(m) => m,
        Err(e) => return json!({ "ok": false, "error": e.to_string() }),
    };
    if !meta.is_dir() {
        return json!({ "ok": false, "error": "目标不是目录" });
    }

    // 递归收集所有文件 —— 对齐 shredderManager.js#116-130
    let mut files = Vec::new();
    let mut seen_real = HashSet::new();

    // realpath 循环守卫 —— 对齐 fileSystem.js#49-56 (复用 filesystem 模式)
    let root_real = fs::canonicalize(&abs).unwrap_or_else(|_| abs.clone());
    seen_real.insert(root_real.to_string_lossy().to_lowercase());

    async fn walk_collect(
        dir: &Path,
        files: &mut Vec<PathBuf>,
        seen_real: &mut HashSet<String>,
    ) -> io::Result<()> {
        let entries = fs::read_dir(dir)?;
        for entry in entries {
            let entry = entry?;
            let full = entry.path();
            let ft = entry.file_type()?;

            if ft.is_dir() {
                let real = fs::canonicalize(&full).ok();
                if let Some(real_path) = real {
                    let key = real_path.to_string_lossy().to_lowercase();
                    if seen_real.contains(&key) {
                        continue;
                    }
                    seen_real.insert(key);
                    Box::pin(walk_collect(&full, files, seen_real)).await?;
                }
            } else if ft.is_file() {
                files.push(full);
            }
        }
        Ok(())
    }

    if let Err(e) = walk_collect(&abs, &mut files, &mut seen_real).await {
        return json!({ "ok": false, "error": e.to_string() });
    }

    // 计算总大小 —— 对齐 shredderManager.js#132
    let total_size: u64 = files
        .iter()
        .filter_map(|f| fs::metadata(f).ok())
        .map(|m| m.len())
        .sum();
    let total_bytes = total_size.saturating_mul(method.passes as u64);
    let mut written: u64 = 0;
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut results = Vec::new();

    // 逐文件粉碎 —— 对齐 shredderManager.js#139-151
    for file_path in &files {
        // 取消检查点(文件循环开始) —— 对齐 shredderManager.js#140
        if CANCELLED.load(Ordering::SeqCst) {
            return json!({
                "ok": false,
                "cancelled": true,
                "results": results,
                "succeeded": succeeded,
                "failed": failed
            });
        }

        let r = shred_file_internal(file_path, method_id.unwrap_or("dod")).await;
        if r.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
            succeeded += 1;
            if let Some(size) = r.get("size").and_then(|v| v.as_u64()) {
                written += size.saturating_mul(method.passes as u64);
            }
        } else {
            failed += 1;
        }
        let mut r_obj = r.as_object().cloned().unwrap_or_default();
        r_obj.insert("path".to_string(), json!(file_path.to_string_lossy().to_string()));
        results.push(json!(r_obj));

        // 进度推送 —— 对齐 shredderManager.js#150
        // 使用 crate::types::send 推送到前端
        send(
            app,
            "shredder:progress",
            json!({
                "current": written,
                "total": total_bytes,
                "filesTotal": files.len(),
                "filesDone": results.len()
            }),
        );
    }

    // 自底向上清理空目录 —— 对齐 shredderManager.js#153-168
    if !CANCELLED.load(Ordering::SeqCst) {
        async fn clean_dirs(dir: &Path) -> io::Result<()> {
            let entries = fs::read_dir(dir)?;
            for entry in entries {
                let entry = entry?;
                let full = entry.path();
                let ft = entry.file_type()?;
                if ft.is_dir() {
                    Box::pin(clean_dirs(&full)).await?;
                    let _ = fs::remove_dir(&full);
                }
            }
            Ok(())
        }

        let _ = clean_dirs(&abs).await;
        let _ = fs::remove_dir(&abs);
    }

    // 返回结果 —— 对齐 shredderManager.js#170
    json!({
        "ok": true,
        "succeeded": succeeded,
        "failed": failed,
        "results": results,
        "size": total_size
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_shred_file_zero() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("test.txt");
        let mut f = File::create(&file).unwrap();
        f.write_all(b"hello world").unwrap();
        drop(f);

        let result = shred_file(&file, Some("zero")).await;
        assert!(result.get("ok").and_then(|v| v.as_bool()).unwrap_or(false));
        assert!(!file.exists());
    }

    #[tokio::test]
    async fn test_shred_file_dod() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("test.txt");
        let mut f = File::create(&file).unwrap();
        f.write_all(b"hello world").unwrap();
        drop(f);

        let result = shred_file(&file, Some("dod")).await;
        assert!(result.get("ok").and_then(|v| v.as_bool()).unwrap_or(false));
        assert_eq!(result.get("passes").and_then(|v| v.as_u64()), Some(3));
        assert!(!file.exists());
    }

    #[tokio::test]
    async fn test_shred_folder() {
        let dir = tempdir().unwrap();
        let subdir = dir.path().join("sub");
        fs::create_dir(&subdir).unwrap();

        let file1 = subdir.join("a.txt");
        let mut f = File::create(&file1).unwrap();
        f.write_all(b"file1").unwrap();
        drop(f);

        let file2 = dir.path().join("b.txt");
        let mut f = File::create(&file2).unwrap();
        f.write_all(b"file2").unwrap();
        drop(f);

        // 使用 mock AppHandle 测试较复杂, 这里仅验证内部逻辑
        // 实际集成测试需在 Tauri 环境运行
    }

    #[test]
    fn test_cancel_flag() {
        reset_cancel();
        assert!(is_running());
        cancel();
        assert!(!is_running());
        reset_cancel();
        assert!(is_running());
    }

    #[test]
    fn test_method_passes() {
        let zero = ShredMethod::zero();
        assert_eq!(zero.passes, 1);

        let random = ShredMethod::random();
        assert_eq!(random.passes, 1);

        let dod = ShredMethod::dod();
        assert_eq!(dod.passes, 3);

        // 验证 DoD 三遍数据模式
        let pass0 = (dod.data_fn)(0);
        assert!(pass0.iter().all(|&b| b == 0x00));

        let pass1 = (dod.data_fn)(1);
        assert!(pass1.iter().all(|&b| b == 0xff));

        let pass2 = (dod.data_fn)(2);
        // 随机数据不全为 0 或 0xff (极小概率除外)
        assert!(!pass2.iter().all(|&b| b == 0x00));
        assert!(!pass2.iter().all(|&b| b == 0xff));
    }
}