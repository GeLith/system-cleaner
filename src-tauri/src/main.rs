// 系统清理助手 - Tauri 版入口
// 由 Electron 版原项目移植而来, 行为逐条对齐:
//   - 无边框窗口 1100x720 (min 900x600), 页面加载完成后再显示
//   - 单实例锁: 第二个实例唤起已有窗口并聚焦
//   - --minimized 启动参数: 开机自启动时最小化到任务栏
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod business;
mod commands;
mod rules_engine;
mod store;
mod system;
mod types;

use tauri::Manager;

fn minimized_flag() -> bool {
    std::env::args().any(|a| a == "--minimized")
}

/// panic=abort 下进程会瞬间消失, GUI 子系统又没有 stderr —— 落盘才能抓现场
fn install_panic_logger() {
    std::panic::set_hook(Box::new(|info| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = format!(
            "[panic @{:?}] {}\n  location: {:?}\n",
            ts,
            info,
            info.location()
        );
        if let Some(dir) = std::env::var_os("TEMP") {
            let p = std::path::PathBuf::from(dir).join("systemcleaner-panic.log");
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
                let _ = f.write_all(line.as_bytes());
                let bt = std::backtrace::Backtrace::force_capture();
                let _ = f.write_all(format!("{bt}\n").as_bytes());
            }
        }
    }));
}

fn main() {
    install_panic_logger();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // 第二实例: 恢复并聚焦主窗口
            use tauri::Manager;
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.unminimize();
                let _ = win.show();
                let _ = win.set_focus();
            }
        }))
        .setup(move |app| {
            store::init(app.handle().clone());
            // 规则目录解析: 打包版 exe 同级 rules/;dev 版 target/debug 相对源码树
            let rules_dir = {
                let exe = std::env::current_exe()
                    .map(|p| p.parent().map(|d| d.to_path_buf()))
                    .unwrap_or(None)
                    .unwrap_or_default();
                let candidates = [
                    exe.join("rules"),
                    exe.join("../../src/ui/rules"),
                ];
                candidates
                    .into_iter()
                    .find(|p| p.is_dir())
                    .unwrap_or_else(|| exe.join("rules"))
            };
            rules_engine::init(rules_dir);
            // --minimized 自启动: 窗口创建后立即最小化 (可见性由配置 visible:true 保证,
            // 不再做"隐藏->显示"切换)
            if minimized_flag() {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.minimize();
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_stats,
            commands::speedup_scan,
            commands::speedup_cancel,
            commands::speedup_optimize,
            commands::clean_scan,
            commands::clean_cancel,
            commands::clean_execute,
            commands::clean_item_files,
            commands::open_folder,
            commands::shredder_open_file,
            commands::shredder_open_folder,
            commands::shredder_browse_folder,
            commands::shredder_stat_file,
            commands::shred_file,
            commands::shred_folder,
            commands::shred_cancel,
            commands::startup_list,
            commands::startup_toggle,
            commands::startup_remove,
            commands::startup_open_location,
            commands::startup_detail,
            commands::startup_smart_optimize,
            commands::startup_add,
            commands::startup_backup,
            commands::startup_restore,
            commands::startup_list_backups,
            commands::startup_set_ignored,
            commands::get_file_icon,
            commands::store_get,
            commands::store_set,
            commands::settings_get_autostart,
            commands::settings_set_autostart,
            commands::window_minimize,
            commands::window_close,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
