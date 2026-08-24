//! 持久化 store —— 忠实移植 data/store.js
//! 路径: <app_data_dir>/store.json ; 原子写(tmp+rename); 损坏文件保留为 .corrupt
use once_cell::sync::OnceCell;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::Manager;

static STATE: OnceCell<Mutex<StoreState>> = OnceCell::new();

struct StoreState {
    path: PathBuf,
    data: Value,
}

fn defaults() -> Value {
    json!({
        "totalCleanedMB": 0,
        "cleanCount": 0,
        "history": [],
    })
}

pub fn init(app: tauri::AppHandle) {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir());
    let path = dir.join("store.json");

    let data = match fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str::<Value>(&raw) {
            Ok(v) => v,
            Err(_) => {
                // 损坏而非不存在：保留坏文件供排查
                let _ = fs::rename(&path, path.with_extension("json.corrupt"));
                defaults()
            }
        },
        Err(_) => defaults(),
    };

    let _ = STATE.set(Mutex::new(StoreState { path, data }));
}

fn state_path(state: &StoreState) -> PathBuf {
    state.path.clone()
}

fn save(state: &StoreState) {
    if let Some(parent) = state.path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    // 原子写：先写临时文件再 rename，进程崩溃不会留下截断的 store.json
    let tmp = state_path(state).with_extension(format!(
        "json.{}.tmp",
        std::process::id()
    ));
    if let Ok(pretty) = serde_json::to_string_pretty(&state.data) {
        if fs::write(&tmp, pretty).is_ok() {
            let _ = fs::rename(&tmp, state_path(state));
        }
    }
}

pub fn get(key: &str) -> Value {
    let st = STATE.get().expect("store not initialized");
    let st = st.lock().unwrap();
    match st.data.get(key) {
        Some(v) => v.clone(),
        None => defaults().get(key).cloned().unwrap_or(Value::Null),
    }
}

pub fn set(key: &str, value: Value) {
    let st = STATE.get().expect("store not initialized");
    let mut st = st.lock().unwrap();
    st.data[key] = value;
    save(&st);
}

/// 对齐 appendHistory: history 上限 30 条, totalCleanedMB 保留两位小数
pub fn append_history(date: String, size_mb: f64) {
    let st = STATE.get().expect("store not initialized");
    let mut st = st.lock().unwrap();
    if !st.data["history"].is_array() {
        st.data["history"] = json!([]);
    }
    {
        let hist = st.data["history"].as_array_mut().unwrap();
        hist.push(json!({ "date": date, "sizeMB": size_mb }));
        let len = hist.len();
        if len > 30 {
            *hist = hist.split_off(len - 30);
        }
    }
    let total = st.data["totalCleanedMB"].as_f64().unwrap_or(0.0) + size_mb;
    st.data["totalCleanedMB"] = json!((total * 100.0).round() / 100.0);
    st.data["cleanCount"] =
        json!(st.data["cleanCount"].as_i64().unwrap_or(0) + 1);
    save(&st);
}

/// 对齐 getStats 形状: { totalCleanedMB(1位小数), cleanCount, history(≤30) }
pub fn get_stats() -> Value {
    let st = STATE.get().expect("store not initialized");
    let st = st.lock().unwrap();
    let total = (st.data["totalCleanedMB"].as_f64().unwrap_or(0.0) * 10.0).round() / 10.0;
    json!({
        "totalCleanedMB": total,
        "cleanCount": st.data["cleanCount"].as_i64().unwrap_or(0),
        "history": st.data["history"].as_array().map(|a| a.clone()).unwrap_or_default(),
    })
}
