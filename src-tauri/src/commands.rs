//! 34 个 Tauri 命令 —— 与 Electron 版 ipc.js 的 ipcMain.handle 一一对应。
//! 参数命名: Rust snake_case, 前端 invoke 传 camelCase (tauri 自动映射)。
//! 返回值统一 serde_json::Value —— 忠实复刻 JS 对象字面量形状。

use serde_json::Value;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::store;

/// 防重入标志:对齐 ipc.js#11 _cleanRunning
static CLEAN_RUNNING: AtomicBool = AtomicBool::new(false);

// ---------- App / Store ----------

#[tauri::command]
pub fn get_app_stats() -> Value {
    // 对齐 ipc.js#32 -> store.getStats()
    store::get_stats()
}

#[tauri::command]
pub fn store_get(key: String) -> Value {
    store::get(&key)
}

#[tauri::command]
pub fn store_set(key: String, value: Value) -> Value {
    store::set(&key, value);
    serde_json::json!({ "ok": true })
}

// ---------- 加速 (speedupManager + scanScheduler) ----------

#[tauri::command]
pub fn speedup_scan(app: AppHandle) -> Value {
    // 对齐 ipc.js#34-42: 运行中返回 {ok:false,...}; 否则启动并经事件推送进度
    if crate::business::scan_scheduler::is_running() {
        return serde_json::json!({ "ok": false, "message": "speedup scan already running" });
    }
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        crate::business::scan_scheduler::scan_speedup(app2).await;
    });
    serde_json::json!({ "ok": true })
}

#[tauri::command]
pub fn speedup_cancel() -> Value {
    crate::business::scan_scheduler::cancel();
    serde_json::json!({ "ok": true })
}

#[tauri::command]
pub async fn speedup_optimize(fix_ids: Vec<String>) -> Value {
    // 对齐 ipc.js#49-69: optimize 后返回逐条结果
    let fix_map = crate::business::scan_scheduler::get_fix_map_snapshot();
    crate::business::speedup::optimize(fix_ids, &fix_map).await
}

// ---------- 清理 (scanScheduler + cleanExecutor) ----------

#[tauri::command]
pub fn clean_scan(app: AppHandle, tab: String) -> Value {
    // 对齐 ipc.js#73-82: supersede 语义, 永不 ok:false; 返回 {ok:true, scanKey}
    let scan_key = crate::business::scan_scheduler::begin_clean_scan();
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        crate::business::scan_scheduler::scan_clean(app2, tab, scan_key).await;
    });
    serde_json::json!({ "ok": true, "scanKey": scan_key })
}

#[tauri::command]
pub fn clean_cancel() -> Value {
    crate::business::scan_scheduler::cancel();
    serde_json::json!({ "ok": true })
}

#[tauri::command]
pub async fn clean_execute(app: AppHandle, items: Vec<Value>) -> Value {
    // 对齐 ipc.js#89-97: 清理执行期间拒绝再次触发(双击/并发导致重复删除+统计翻倍)
    if CLEAN_RUNNING.swap(true, Ordering::SeqCst) {
        return serde_json::json!({ "ok": false, "message": "clean already running", "failed": [] });
    }
    let result = crate::business::clean_executor::execute_clean(app, items).await;
    CLEAN_RUNNING.store(false, Ordering::SeqCst);
    result
}

#[tauri::command]
pub fn clean_item_files(group_id: String, item_id: String) -> Value {
    // 对齐 ipc.js#99-100: 未扫描返回 []
    match crate::business::scan_scheduler::get_scan_files(&group_id, &item_id) {
        Some(v) => serde_json::json!(v),
        None => serde_json::json!([]),
    }
}

// ---------- 文件夹/资源管理器 ----------

#[tauri::command]
pub fn open_folder(app: AppHandle, file_path: String) -> Value {
    // 对齐 ipc.js#102-107: shell.openPath —— OS 默认方式打开(失败静默)
    let _ = app.opener().open_path(file_path, None::<&str>);
    serde_json::json!({ "ok": true })
}

// ---------- 碎纸机 (shredderManager) ----------

#[tauri::command]
pub async fn shredder_open_file(app: AppHandle) -> Value {
    // 对齐 ipc.js#164-169: 多选文件对话框, 取消返回 {paths:[]}
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = std::sync::mpsc::channel::<Option<Vec<String>>>();
    app.dialog()
        .file()
        .set_title("选择要粉碎的文件")
        .pick_files(move |paths| {
            let v = paths.map(|ps| ps.into_iter().map(|p| p.to_string()).collect::<Vec<_>>());
            let _ = tx.send(v);
        });
    match rx.recv() {
        Ok(Some(paths)) => serde_json::json!({ "paths": paths }),
        _ => serde_json::json!({ "paths": [] }),
    }
}

#[tauri::command]
pub async fn shredder_open_folder(app: AppHandle) -> Value {
    // 对齐 ipc.js#170-175: 目录对话框, 取消返回 {path:""}
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = std::sync::mpsc::channel::<Option<String>>();
    app.dialog()
        .file()
        .set_title("选择要粉碎的文件夹")
        .pick_folder(move |path| {
            let v = path.map(|p| p.to_string());
            let _ = tx.send(v);
        });
    match rx.recv() {
        Ok(Some(path)) => serde_json::json!({ "path": path }),
        _ => serde_json::json!({ "path": "" }),
    }
}

#[tauri::command]
pub async fn shredder_browse_folder(folder_path: String) -> Value {
    // 对齐 ipc.js#188-191: fileSystem.list_folder_files (realpath防环+深度/数量上限)
    match crate::system::filesystem::list_folder_files(std::path::Path::new(&folder_path), 8, 5000).await {
        Ok(v) => v,
        Err(e) => serde_json::json!({ "error": e }),
    }
}

#[tauri::command]
pub async fn shredder_stat_file(file_path: String) -> Value {
    // 对齐 ipc.js#176-185: {name,size,isDirectory} 或 {error}
    let p = std::path::Path::new(&file_path);
    if !p.exists() {
        return serde_json::json!({ "error": "文件不存在" });
    }
    let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
    match std::fs::metadata(p) {
        Ok(meta) => serde_json::json!({
            "name": name,
            "size": meta.len(),
            "isDirectory": meta.is_dir(),
        }),
        Err(e) => serde_json::json!({ "error": e.to_string() }),
    }
}

#[tauri::command]
pub async fn shred_file(file_path: String, method: String) -> Value {
    // 对齐 ipc.js#192-193: method 缺省 'dod' 由 shredder 内部兜底
    let m = if method.is_empty() { None } else { Some(method.as_str()) };
    crate::business::shredder::shred_file(Path::new(&file_path), m).await
}

#[tauri::command]
pub async fn shred_folder(app: AppHandle, folder_path: String, method: String) -> Value {
    // 对齐 ipc.js#194-201: 进度经 shredder.rs 内部 types::send 推送; 失败返回 {ok:false,error}
    let m = if method.is_empty() { None } else { Some(method.as_str()) };
    crate::business::shredder::shred_folder(&app, Path::new(&folder_path), m).await
}

#[tauri::command]
pub fn shred_cancel() -> Value {
    // 对齐 ipc.js#202 (fire-and-forget)
    crate::business::shredder::cancel();
    serde_json::json!({ "ok": true })
}

// ---------- 启动项 (startupManager) ----------

#[tauri::command]
pub async fn startup_list(tab: String) -> Value {
    crate::business::startup::list(tab).await
}

#[tauri::command]
pub async fn startup_toggle(item_id: String, enabled: bool) -> Value {
    crate::business::startup::toggle(item_id, enabled).await
}

#[tauri::command]
pub async fn startup_remove(item_id: String) -> Value {
    crate::business::startup::remove(item_id).await
}

#[tauri::command]
pub async fn startup_open_location(item_id: String) -> Value {
    crate::business::startup::open_location(item_id).await
}

#[tauri::command]
pub async fn startup_detail(item_id: String) -> Value {
    crate::business::startup::get_detail(item_id).await
}

#[tauri::command]
pub async fn startup_smart_optimize() -> Value {
    crate::business::startup::smart_optimize().await
}

#[tauri::command]
pub async fn startup_add(item: Value) -> Value {
    crate::business::startup::add(item).await
}

#[tauri::command]
pub async fn startup_backup() -> Value {
    crate::business::startup::backup().await
}

#[tauri::command]
pub async fn startup_restore(file_name: String) -> Value {
    crate::business::startup::restore(file_name).await
}

#[tauri::command]
pub async fn startup_list_backups() -> Value {
    crate::business::startup::list_backups().await
}

#[tauri::command]
pub async fn startup_set_ignored(item_id: String, ignored: bool) -> Value {
    crate::business::startup::set_ignored(item_id, ignored).await
}

#[tauri::command]
pub async fn get_file_icon(file_path: String) -> Value {
    // contract: ipc.js#120 - returns dataURL string or null (pages.js#1105 uses it as src)
    let p = file_path;
    let url = tauri::async_runtime::spawn_blocking(move || {
        crate::system::icons::file_icon_data_url(&p)
    })
    .await
    .unwrap_or_default();
    if url.is_empty() {
        Value::Null
    } else {
        Value::String(url)
    }
}

// ---------- 设置: 开机自启 ----------
// 对齐 ipc.js#131-159: 以注册表 Run 键为事实源(hkcu+hklm 都查),
// setLoginItemSettings 对应写 HKCU Run, 参数 ['--minimized']

const AUTOSTART_VALUE_NAME: &str = "SystemCleanerTauri";

#[tauri::command]
pub async fn settings_get_autostart() -> Value {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let keys = [
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
        "HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
    ];
    let mut enabled = false;
    for k in keys {
        if let Ok(vals) = crate::system::registry::list_values(k).await {
            for v in vals {
                if let Some(val) = v.get("value").and_then(|x| x.as_str()) {
                    if !exe.is_empty() && val.to_lowercase().contains(&exe) {
                        enabled = true;
                        break;
                    }
                }
            }
        }
        if enabled {
            break;
        }
    }
    // 契约对齐 ipc.js#146: 返回纯布尔 (pages.js refreshSettings 直接 !!on 判定)
    serde_json::json!(enabled)
}

#[tauri::command]
pub async fn settings_set_autostart(enabled: bool) -> Value {
    let key = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    if enabled {
        let exe = match std::env::current_exe() {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(e) => return serde_json::json!({ "ok": false, "error": e.to_string() }),
        };
        let value = format!("\"{}\" --minimized", exe);
        match crate::system::registry::set_string(key, AUTOSTART_VALUE_NAME, &value).await {
            Ok(_) => serde_json::json!({ "ok": true, "enabled": true }),
            Err(e) => serde_json::json!({ "ok": false, "error": e }),
        }
    } else {
        // 删除忽略错误(可能本就不存在)
        let _ = crate::system::registry::delete_value(key, AUTOSTART_VALUE_NAME).await;
        serde_json::json!({ "ok": true, "enabled": false })
    }
}

// ---------- 窗口控制 (electronAPI) ----------


#[tauri::command]
pub fn window_minimize(app: AppHandle) -> Value {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.minimize();
    }
    serde_json::json!({ "ok": true })
}

#[tauri::command]
pub fn window_close(app: AppHandle) -> Value {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.close();
    }
    serde_json::json!({ "ok": true })
}
