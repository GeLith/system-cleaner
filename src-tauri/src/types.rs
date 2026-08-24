//! 跨模块共享类型与事件推送助手
use serde_json::Value;
use tauri::{AppHandle, Emitter};

/// 对齐 Electron ipc.js 的 send(): 向主窗口推送事件。
/// 频道白名单由前端 api-shim.js 保证(与 preload.js EVENT_CHANNELS 一致)。
pub fn send(app: &AppHandle, channel: &str, payload: Value) {
    let _ = app.emit(channel, payload);
}
