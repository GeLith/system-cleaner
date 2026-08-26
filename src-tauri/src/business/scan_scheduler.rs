//! 扫描编排器 —— 忠实移植 business/scanScheduler.js (570 行)
//! - supersede: SCAN_KEY 单调递增, 旧扫描检查点自动失效
//! - 状态全部走短临界区(std Mutex 不跨 .await), 保证 future Send
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::AppHandle;

use crate::business::startup::{
    build_rule_index_export, compute_ban_rate_export, extract_exe_from_command_export,
    list_scheduled_tasks_export, match_rule_export, sanitize_id_export,
};
use crate::rules_engine;
use crate::system::{exec, filesystem, paths, process, registry, service};
use crate::types;

// ============================================================
// 模块状态(对齐 js 单例字段)
// ============================================================

static SCAN_KEY: AtomicU64 = AtomicU64::new(0);
static CANCELLED: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);

/// 扫描结果登记: key = "{groupId}:{itemId}" -> 文件路径列表
static RESULTS: Lazy<Mutex<HashMap<String, Vec<String>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
/// 修复项登记: fixId -> fix 参数(如 category)
static FIX_MAP: Lazy<Mutex<HashMap<String, Value>>> = Lazy::new(|| Mutex::new(HashMap::new()));
/// 分组定义缓存: groupId -> def
static GROUP_DEFS: Lazy<Mutex<HashMap<String, Value>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// 测试辅助: 手动登记扫描文件清单
#[doc(hidden)]
pub fn record_scan_files(group_id: &str, item_id: &str, files: Vec<String>) {
    RESULTS
        .lock()
        .unwrap()
        .insert(format!("{}:{}", group_id, item_id), files);
}

/// 测试辅助: 手动登记组定义
#[doc(hidden)]
pub fn record_group_def(def: Value) {
    let gid = def
        .get("groupId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    GROUP_DEFS.lock().unwrap().insert(gid, def);
}

#[derive(Default)]
struct BigFilesCache {
    time: u128,
    items: Option<Vec<Value>>,
    files_by_item: HashMap<String, Vec<String>>,
}
static BIG_CACHE: Lazy<Mutex<BigFilesCache>> = Lazy::new(|| Mutex::new(BigFilesCache::default()));

// ============================================================
// 常量
// ============================================================

pub const SAFE_TO_CLOSE: &[&str] = &[
    "qqmusic.exe", "kugou.exe", "kuwo.exe", "cloudmusic.exe", "potplayermini.exe", "vlc.exe",
    "foobar2000.exe", "qqlive.exe", "iqiyi.exe", "youkudesktop.exe", "baofengplatform.exe",
    "thunderplatform.exe", "thunder.exe", "xunleiservice.exe", "baidunetdisk.exe",
    "utorrent.exe", "qbittorrent.exe", "googleupdate.exe", "msedgeupdate.exe",
    "chromeupdate.exe", "qqexternal.exe",
];

/// 软件缓存目录模板(%APPDATA%/%LOCALAPPDATA% 由 paths.resolve_path 展开)
const APP_CACHE_DIRS: &[(&str, &str, &[&str])] = &[
    ("微信", "WeChat.exe", &["%APPDATA%\\Tencent\\WeChat", "%LOCALAPPDATA%\\Tencent\\WeChat"]),
    ("QQ", "QQ.exe", &["%APPDATA%\\Tencent\\QQ", "%LOCALAPPDATA%\\Tencent\\QQ"]),
    ("TIM", "TIM.exe", &["%APPDATA%\\Tencent\\TIM"]),
    ("钉钉", "DingTalk.exe", &["%APPDATA%\\DingTalk"]),
    ("企业微信", "WXWork.exe", &["%APPDATA%\\Tencent\\WXWork"]),
    ("百度网盘", "BaiduNetdisk.exe", &["%APPDATA%\\baidu\\BaiduNetdisk"]),
    ("迅雷", "Thunder.exe", &["%APPDATA%\\Thunder Network"]),
    ("网易云音乐", "cloudmusic.exe", &["%APPDATA%\\NetEase\\CloudMusic"]),
    ("QQ音乐", "QQMusic.exe", &["%APPDATA%\\Tencent\\QQMusic"]),
    ("酷狗音乐", "KuGou.exe", &["%APPDATA%\\KuGou"]),
    ("爱奇艺", "iQIYI.exe", &["%APPDATA%\\iQIYI Video"]),
    ("腾讯视频", "QQLive.exe", &["%APPDATA%\\Tencent\\QQLive"]),
    ("WPS", "wps.exe", &["%APPDATA%\\Kingsoft\\WPS Office"]),
    ("Steam", "steam.exe", &["%APPDATA%\\Steam"]),
    ("Discord", "Discord.exe", &["%APPDATA%\\discord"]),
    ("Telegram", "Telegram.exe", &["%APPDATA%\\Telegram Desktop"]),
];

/// StartupApproved 禁用二进制(对齐 js#22 DISABLED_BINARY)
fn disabled_binary() -> Value {
    json!([3, 0, 0, 0, 0, 0, 0, 0, 0, 0])
}

// ============================================================
// 公共访问器(命令层入口)
// ============================================================

/// 取号: 新扫描前调用, 返回隔离键(js#102-104)
pub fn begin_clean_scan() -> u64 {
    SCAN_KEY.fetch_add(1, Ordering::SeqCst) + 1
}

pub fn cancel() {
    CANCELLED.store(true, Ordering::SeqCst);
}

pub fn is_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

pub fn get_fix(fix_id: &str) -> Option<Value> {
    FIX_MAP.lock().unwrap().get(fix_id).cloned()
}

pub fn get_group_def(group_id: &str) -> Option<Value> {
    GROUP_DEFS.lock().unwrap().get(group_id).cloned()
}

/// 未命中返回 Null(命令层兜底为 [])
pub fn get_scan_files(group_id: &str, item_id: &str) -> Option<Vec<String>> {
    let key = format!("{}:{}", group_id, item_id);
    RESULTS.lock().unwrap().get(&key).cloned()
}

/// speedup_optimize 需要整表(id->fix 含 category)构建 byCategory 聚合
pub fn get_fix_map_snapshot() -> HashMap<String, Value> {
    FIX_MAP.lock().unwrap().clone()
}

fn set_running(v: bool) {
    RUNNING.store(v, Ordering::SeqCst);
}

/// supersede 探针工厂: 组间检查点 + scan_dir 内部轮询共用同一语义
fn is_cancelled_box(scan_key: u64) -> Box<dyn Fn() -> bool + Send + Sync> {
    Box::new(move || CANCELLED.load(Ordering::Relaxed) || scan_key != SCAN_KEY.load(Ordering::Relaxed))
}

    /// scan_key == u64::MAX 是 speedup 扫描的「无 key」哨兵:
    /// 此时只响应全局取消标志, 不做 supersede 比对 (SCAN_KEY 初始为 0,
    /// 若不特判, MAX != 0 恒真会导致首次迭代即误判取消 -> 空扫描直接 done)
    fn is_cancelled_now(scan_key: u64) -> bool {
        if scan_key == u64::MAX {
            return CANCELLED.load(Ordering::Relaxed);
        }
        CANCELLED.load(Ordering::Relaxed) || scan_key != SCAN_KEY.load(Ordering::Relaxed)
    }

// ============================================================
// speedup 扫描 (js#120-298)
// ============================================================

/// 对齐 js#140-144 assignId: 返回体带 id, 登记表额外带 category
fn assign_fix(counter: &mut u32, category: &str, mut base: Value) -> Value {
    *counter += 1;
    let id = format!("fix_{:03}", counter);
    if let Some(o) = base.as_object_mut() {
        o.insert("id".into(), json!(id.clone()));
    }
    let mut stored = base.clone();
    if let Some(o) = stored.as_object_mut() {
        o.insert("category".into(), json!(category));
    }
    FIX_MAP.lock().unwrap().insert(id, stored);
    base
}

/// 注册表 DWORD 读值兼容(数字或字符串)
fn reg_i64(v: Option<&Value>) -> Option<i64> {
    let v = v?;
    match v.get("value") {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

pub async fn scan_speedup(app: AppHandle) {
    CANCELLED.store(false, Ordering::SeqCst);
    set_running(true);
    FIX_MAP.lock().unwrap().clear();
    // 对齐 js#125-132 分类定义
    let categories = vec![
        json!({"id":"win11","name":"Win11加速项","desc":"优化Win11系统和内存设置","icon":"win11"}),
        json!({"id":"boot","name":"开机加速","desc":"禁用非必要自启项目","icon":"boot"}),
        json!({"id":"software","name":"软件运行加速","desc":"退出暂不使用的后台软件","icon":"software"}),
        json!({"id":"system","name":"系统加速","desc":"关闭非必要系统服务","icon":"system"}),
        json!({"id":"disk","name":"硬盘加速","desc":"检查磁盘空间与传输效率","icon":"disk"}),
        json!({"id":"network","name":"网络加速","desc":"优化DNS与网络参数","icon":"network"}),
    ];
    let total_cats = categories.len();
    let mut items: Vec<Value> = Vec::new();
    let mut counter: u32 = 0;
    for (i, cat) in categories.iter().enumerate() {
        if is_cancelled_now(u64::MAX) { break; } // speedup 无 scanKey, 仅看取消标志
        let cat_id = cat["id"].as_str().unwrap_or("").to_string();
        types::send(&app, "speedup:progress", json!({
            "phase": "speedup", "current": i, "total": total_cats,
            "itemId": cat_id, "message": format!("正在扫描{}", cat["name"].as_str().unwrap_or("")),
        }));
        types::send(&app, "speedup:item-status", json!({
            "itemId": cat_id, "status": "scanning", "found": 0,
            "desc": cat["desc"].as_str().unwrap_or(""),
        }));
        let found = match cat_id.as_str() {
            "win11" => scan_win11(&mut counter).await,
            "boot" => scan_boot(&mut counter).await,
            "software" => scan_software(&mut counter).await,
            "system" => scan_system(&mut counter).await,
            "disk" => scan_disk(&mut counter).await,
            "network" => scan_network(&mut counter).await,
            _ => Vec::new(),
        };
        // 对齐 js#150-151 注释: 总数=各分类真实发现之和
        let item = json!({
            "id": cat_id, "name": cat["name"], "desc": cat["desc"], "icon": cat["icon"],
            "status": "found", "found": found.len(), "items": found,
        });
        let found_n = item["found"].as_i64().unwrap_or(0);
        items.push(item);
        types::send(&app, "speedup:item-status", json!({
            "itemId": cat_id, "status": "found", "found": found_n,
            "desc": cat["desc"].as_str().unwrap_or(""),
        }));
    }
    let total_fixes: i64 = items.iter().map(|it| it["found"].as_i64().unwrap_or(0)).sum();
    types::send(&app, "speedup:done", json!({ "total": total_fixes, "items": items }));
    set_running(false);
}

/// 对齐 js#170-193 _scanWin11: 五个注册表阈值检查
async fn scan_win11(counter: &mut u32) -> Vec<Value> {
    let mut fixes = Vec::new();
    let checks: &[(&str, &str, i64, Value, &str)] = &[
        ("HKCU\\Control Panel\\Desktop", "MenuShowDelay", 200,
         json!({"keyPath":"HKCU\\Control Panel\\Desktop","valueName":"MenuShowDelay","value":200,"type":"dword"}),
         "优化菜单显示延迟"),
    ];
    let _ = checks; // 逐条展开以保持与 js 相同的 detail 文案
    if let Ok(Some(menu)) = registry::query_value("HKCU\\Control Panel\\Desktop", "MenuShowDelay").await {
        if reg_i64(Some(&menu)).unwrap_or(0) > 200 {
            fixes.push(assign_fix(counter, "win11", json!({
                "name": "优化菜单显示延迟",
                "detail": "菜单弹出延迟过高，影响操作响应速度",
                "action": "reg_set",
                "target": {"keyPath":"HKCU\\Control Panel\\Desktop","valueName":"MenuShowDelay","value":200,"type":"dword"},
                "risky": false, "suggestion": "建议优化"
            })));
        }
    }
    if let Ok(Some(wk)) = registry::query_value("HKCU\\Control Panel\\Desktop", "WaitToKillAppTimeout").await {
        if reg_i64(Some(&wk)).unwrap_or(0) > 5000 {
            fixes.push(assign_fix(counter, "win11", json!({
                "name": "缩短程序关闭等待时间",
                "detail": "关机时等待程序退出的时间过长",
                "action": "reg_set",
                "target": {"keyPath":"HKCU\\Control Panel\\Desktop","valueName":"WaitToKillAppTimeout","value":5000,"type":"dword"},
                "risky": false, "suggestion": "建议优化"
            })));
        }
    }
    if let Ok(Some(ws)) = registry::query_value("HKLM\\SYSTEM\\CurrentControlSet\\Control", "WaitToKillServiceTimeout").await {
        if reg_i64(Some(&ws)).unwrap_or(0) > 5000 {
            fixes.push(assign_fix(counter, "win11", json!({
                "name": "缩短服务关闭等待时间",
                "detail": "关机时等待服务退出的时间过长",
                "action": "reg_set",
                "target": {"keyPath":"HKLM\\SYSTEM\\CurrentControlSet\\Control","valueName":"WaitToKillServiceTimeout","value":5000,"type":"dword"},
                "risky": false, "suggestion": "建议优化"
            })));
        }
    }
    if let Ok(Some(hb)) = registry::query_value("HKLM\\SYSTEM\\CurrentControlSet\\Control\\Power", "HibernateEnabled").await {
        if reg_i64(Some(&hb)).unwrap_or(0) == 1 {
            fixes.push(assign_fix(counter, "win11", json!({
                "name": "关闭休眠功能",
                "detail": "休眠文件占用磁盘空间，关闭可释放空间",
                "action": "reg_set",
                "target": {"keyPath":"HKLM\\SYSTEM\\CurrentControlSet\\Control\\Power","valueName":"HibernateEnabled","value":0,"type":"dword"},
                "risky": true, "suggestion": "建议关闭"
            })));
        }
    }
    if let Ok(Some(vfx)) = registry::query_value("HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\VisualEffects", "VisualFXSetting").await {
        if reg_i64(Some(&vfx)).unwrap_or(2) != 2 {
            fixes.push(assign_fix(counter, "win11", json!({
                "name": "调整为最佳性能",
                "detail": "关闭动画特效可提升系统响应速度",
                "action": "reg_set",
                "target": {"keyPath":"HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\VisualEffects","valueName":"VisualFXSetting","value":2,"type":"dword"},
                "risky": true, "suggestion": "建议优化"
            })));
        }
    }
    fixes
}

/// 对齐 js#195-228 _scanBoot: Run 键 + 计划任务按规则库评分
async fn scan_boot(counter: &mut u32) -> Vec<Value> {
    let mut fixes = Vec::new();
    let rules = rules_engine::get_startup_rules().await.unwrap_or(Value::Null);
    let index = build_rule_index_export(rules);
    let run_keys: [(&str, &str); 2] = [
        (
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run",
        ),
        (
            "HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
            "HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run",
        ),
    ];
    for (key, approved_key) in run_keys {
        let values = registry::list_values(key).await.unwrap_or_default();
        if values.is_empty() { continue; }
        // 对齐 js#206-209: StartupApproved 值以 "02" 开头视为启用
        let approved = registry::list_values(approved_key).await.unwrap_or_default();
        let is_enabled = |name: &str| -> bool {
            match approved.iter().find(|a| a.get("name").and_then(|v| v.as_str()) == Some(name)) {
                None => true,
                Some(a) => a.get("value").and_then(|v| v.as_str()).map(|s| s.starts_with("02")).unwrap_or(true),
            }
        };
        for v in &values {
            let name = match v.get("name").and_then(|x| x.as_str()) { Some(n) => n.to_string(), None => continue };
            let raw_value = v.get("value").cloned().unwrap_or(Value::Null);
            let cmd_str = raw_value.as_str().unwrap_or("").to_string();
            let enabled = is_enabled(&name);
            let exe = extract_exe_from_command_export(cmd_str.clone());
            let exe_opt = exe.as_str().map(|s| s.to_string());
            let rule = match_rule_export(index.clone(), Some(name.clone()), exe_opt);
            let rule_opt = if rule.is_null() { None } else { Some(rule.clone()) };
            let ban_rate = compute_ban_rate_export(rule_opt, enabled);
            // 对齐 js#213: 评分>=60 且当前启用才建议禁用
            if ban_rate >= 60 && enabled {
                fixes.push(assign_fix(counter, "boot", json!({
                    "name": format!("禁用启动项 {}", name),
                    "detail": raw_value,
                    "action": "reg_set",
                    "target": {"keyPath": approved_key, "valueName": name, "value": disabled_binary(), "type": "binary"},
                    "risky": false, "suggestion": "建议禁用"
                })));
            }
        }
    }
    // 计划任务(js#218-226)
    let tasks = list_scheduled_tasks_export().await;
    if let Some(arr) = tasks.as_array() {
        for t in arr {
            let t_name = t.get("name").and_then(|x| x.as_str()).unwrap_or("");
            if t.get("isMicrosoft").and_then(|x| x.as_bool()).unwrap_or(false) { continue; }
            let command = t.get("command").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let enabled = t.get("enabled").and_then(|x| x.as_bool()).unwrap_or(false);
            let exe = extract_exe_from_command_export(command.clone());
            let exe_opt = exe.as_str().map(|s| s.to_string());
            let rule = match_rule_export(index.clone(), Some(t_name.to_string()), exe_opt);
            let rule_opt = if rule.is_null() { None } else { Some(rule) };
            let ban_rate = compute_ban_rate_export(rule_opt, enabled);
            if ban_rate >= 60 && enabled {
                let clean_name = t_name.trim_start_matches('\\').to_string();
                fixes.push(assign_fix(counter, "boot", json!({
                    "name": format!("禁用计划任务 {}", clean_name),
                    "detail": if command.is_empty() { Value::from(t_name) } else { Value::from(command) },
                    "action": "reg_set",
                    "target": {"taskName": t_name, "type": "task"},
                    "risky": false, "suggestion": "建议禁用"
                })));
            }
        }
    }
    fixes
}

/// 对齐 js#230-239 _scanSoftware: SAFE_TO_CLOSE 后台程序退出建议
async fn scan_software(counter: &mut u32) -> Vec<Value> {
    let mut fixes = Vec::new();
    if let Ok(procs) = process::list_processes().await {
        for p in procs {
            let name = p.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
            if SAFE_TO_CLOSE.contains(&name.to_lowercase().as_str()) {
                let detail = p.get("path").and_then(|x| x.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("PID {}", p.get("pid").and_then(|x| x.as_u64()).unwrap_or(0)));
                fixes.push(assign_fix(counter, "software", json!({
                    "name": format!("退出后台程序 {}", name),
                    "detail": detail,
                    "action": "kill_process",
                    "target": p.get("pid").cloned().unwrap_or(Value::Null),
                    "risky": false, "suggestion": "建议退出"
                })));
            }
        }
    }
    fixes
}

/// 对齐 js#241-254 _scanSystem: 高耗时自启系统服务
async fn scan_system(counter: &mut u32) -> Vec<Value> {
    let mut fixes = Vec::new();
    let boot_ref = rules_engine::get_boot_time_ref().await.unwrap_or(Value::Null);
    if let Ok(services) = service::list_services().await {
        for s in services {
            let start_type = s.get("startType").and_then(|v| v.as_str()).unwrap_or("");
            if start_type != "AUTO_START" { continue; }
            let name = match s.get("name").and_then(|v| v.as_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            if service::is_protected(&name) { continue; }
            let base = boot_ref.get(&name).and_then(|v| v.as_u64()).unwrap_or(0);
            if base >= 300 {
                fixes.push(assign_fix(counter, "system", json!({
                    "name": format!("禁用高耗时服务 {}", name),
                    "detail": format!("开机耗时基准 {}ms", base),
                    "action": "disable_service",
                    "target": name,
                    "risky": false, "suggestion": "建议禁用"
                })));
            }
        }
    }
    fixes
}

/// 对齐 js#256-272 _scanDisk: 系统盘可用空间不足提示
async fn scan_disk(counter: &mut u32) -> Vec<Value> {
    let mut fixes = Vec::new();
    let res = exec::run_async(
        "powershell",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$d=[System.IO.DriveInfo]::GetDrives() | Where-Object { $_.IsReady -and $_.DriveType -eq 3 } | Select-Object -First 1; if($d){[math]::Round($d.TotalFreeSpace/1GB,1);[math]::Round($d.TotalSize/1GB,1)}",
        ],
        None,
    )
    .await;
    if let Ok(out) = res {
        let lines: Vec<&str> = out
            .split(['\r', '\n'])
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect();
        if lines.len() >= 2 {
            let free_gb: f64 = lines[0].parse().unwrap_or(0.0);
            let total_gb: f64 = lines[1].parse().unwrap_or(0.0);
            let pct = if total_gb > 0.0 { free_gb / total_gb } else { 1.0 };
            if pct < 0.15 {
                fixes.push(assign_fix(counter, "disk", json!({
                    "name": "磁盘空间不足，建议清理临时文件",
                    "detail": format!("系统盘剩余 {:.1}GB / {:.1}GB", free_gb, total_gb),
                    "action": "clean_file",
                    "target": path_str(paths::temp_dir()),
                    "risky": false, "suggestion": "建议清理"
                })));
            }
        }
    }
    fixes
}

/// 对齐 js#274-298 _scanNetwork: 活动网卡缺 DNS 配置 -> 公共 DNS; 固定加 flush_dns
async fn scan_network(counter: &mut u32) -> Vec<Value> {
    let mut fixes = Vec::new();
    let ifaces = registry::query_sub_keys(
        "HKLM\\SYSTEM\\CurrentControlSet\\Services\\Tcpip\\Parameters\\Interfaces",
    )
    .await
    .unwrap_or_default();
    let mut no_dns: Option<String> = None;
    for iface in ifaces {
        let key = format!(
            "HKLM\\SYSTEM\\CurrentControlSet\\Services\\Tcpip\\Parameters\\Interfaces\\{}",
            iface
        );
        let dhcp = registry::query_value(&key, "DhcpIPAddress").await.ok().flatten();
        let has_active_dhcp = dhcp
            .as_ref()
            .and_then(|d| d.get("value").and_then(|v| v.as_str()))
            .map(|s| {
                let t = s.trim();
                !t.is_empty() && t != "0.0.0.0"
            })
            .unwrap_or(false);
        let mut has_static_ip = false;
        if !has_active_dhcp {
            let ip = registry::query_value(&key, "IPAddress").await.ok().flatten();
            has_static_ip = ip
                .as_ref()
                .and_then(|d| d.get("value").and_then(|v| v.as_str()))
                .map(|s| {
                    let t = s.trim();
                    !t.is_empty() && t != "0.0.0.0"
                })
                .unwrap_or(false);
        }
        if !has_active_dhcp && !has_static_ip {
            continue;
        }
        let ns = registry::query_value(&key, "NameServer").await.ok().flatten();
        let ns_val = ns
            .as_ref()
            .and_then(|d| d.get("value").and_then(|v| v.as_str()))
            .unwrap_or("")
            .trim()
            .to_string();
        if ns_val.is_empty() {
            no_dns = Some(key);
            break;
        }
    }
    if let Some(dns_key) = no_dns {
        fixes.push(assign_fix(counter, "network", json!({
            "name": "配置公共DNS加速解析",
            "detail": "当前网络接口使用默认DNS，可切换至公共DNS提升解析速度",
            "action": "reg_set",
            "target": {
                "keyPath": dns_key,
                "valueName": "NameServer",
                "value": "223.5.5.5,8.8.8.8",
                "type": "string"
            },
            "risky": true, "suggestion": "建议优化"
        })));
    }
    fixes.push(assign_fix(counter, "network", json!({
        "name": "刷新DNS缓存",
        "detail": "清除本地DNS解析缓存，加速域名解析",
        "action": "flush_dns",
        "target": Value::Null,
        "risky": false, "suggestion": "建议执行"
    })));
    fixes
}

/// 对齐 js#56-79 findBrowserCacheDirs: 浏览器缓存子目录探测
fn find_browser_cache_dirs(kind: &str, base: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    let profiles = match std::fs::read_dir(base) {
        Ok(rd) => rd,
        Err(_) => return dirs,
    };
    if kind == "firefox" {
        for p in profiles.flatten() {
            let cache2 = p.path().join("cache2");
            if cache2.exists() {
                dirs.push(cache2);
            }
        }
        return dirs;
    }
    for p in profiles.flatten() {
        let name = p.file_name().to_string_lossy().to_string();
        if !(name == "Default" || name.starts_with("Profile ")) {
            continue;
        }
        for sub in ["Cache", "Code Cache", "GPUCache"] {
            let d = p.path().join(sub);
            if d.exists() {
                dirs.push(d);
            }
        }
    }
    dirs
}

fn path_str(p: PathBuf) -> String {
    p.to_string_lossy().into_owned()
}

/// 对齐 js#344-385 _buildGroupDefs: 按 tab 返回分组定义(目录用字符串存储)
async fn build_group_defs(tab: &str) -> Vec<Value> {
    let la = paths::local_app_data();
    let pd = paths::program_data();
    let sr = paths::system_root();

    let trash_defs: Vec<Value> = vec![
        json!({"groupId":"recycle_bin","groupName":"回收站","icon":"recycle","category":"trash","checked":true,"risky":true,"type":"recycle_bin","action":"recycle_bin","allowedRoot":null}),
        json!({"groupId":"sys_temp","groupName":"Windows临时文件","icon":"system","category":"trash","checked":true,"risky":false,"type":"dir_scan","action":"delete","dirs":[path_str(paths::temp_dir())],"minAgeDays":7,"maxDepth":8,"desc":"%TEMP% 中超过7天的临时文件"}),
        json!({"groupId":"windows_temp","groupName":"系统Temp目录","icon":"system","category":"trash","checked":true,"risky":false,"type":"dir_scan","action":"delete","dirs":[path_str(paths::windows_temp_dir())],"minAgeDays":7,"maxDepth":8,"desc":"C:\\Windows\\Temp 中超过7天的文件"}),
        json!({"groupId":"thumb_cache","groupName":"缩略图缓存","icon":"image","category":"trash","checked":true,"risky":false,"type":"glob_scan","action":"delete","dir":path_str(la.join("Microsoft").join("Windows").join("Explorer")),"pattern":"thumbcache_","desc":"缩略图数据库文件"}),
        json!({"groupId":"prefetch","groupName":"预读取缓存","icon":"system","category":"trash","checked":true,"risky":false,"type":"dir_scan","action":"delete","dirs":[path_str(sr.join("Prefetch"))],"minAgeDays":7,"maxDepth":2,"desc":"超过7天的预读取文件"}),
        json!({"groupId":"browser_cache","groupName":"浏览器缓存","icon":"browser","category":"trash","checked":true,"risky":false,"type":"browser_cache","action":"delete","desc":"Edge/Chrome/Firefox 缓存文件"}),
        json!({"groupId":"crash_dumps","groupName":"崩溃转储","icon":"system","category":"trash","checked":true,"risky":false,"type":"dir_scan","action":"delete","dirs":[path_str(la.join("CrashDumps")), path_str(pd.join("Microsoft").join("Windows").join("WER"))],"minAgeDays":0,"maxDepth":4,"desc":"程序崩溃产生的转储文件"}),
        json!({"groupId":"error_reports","groupName":"错误报告","icon":"system","category":"trash","checked":true,"risky":false,"type":"dir_scan","action":"delete","dirs":[path_str(la.join("Microsoft").join("Windows").join("WER"))],"minAgeDays":0,"maxDepth":4,"desc":"Windows错误报告"}),
        json!({"groupId":"soft_dist","groupName":"系统更新缓存","icon":"system","category":"trash","checked":true,"risky":false,"type":"dir_scan","action":"delete","dirs":[path_str(sr.join("SoftwareDistribution").join("Download"))],"minAgeDays":0,"maxDepth":4,"desc":"Windows更新下载缓存"}),
        json!({"groupId":"font_cache","groupName":"字体缓存","icon":"system","category":"trash","checked":true,"risky":false,"type":"dir_scan","action":"delete","dirs":[path_str(la.join("FontCache"))],"minAgeDays":0,"maxDepth":2,"desc":"字体缓存数据库"}),
        json!({"groupId":"inet_cache","groupName":"IE缓存","icon":"browser","category":"trash","checked":true,"risky":false,"type":"dir_scan","action":"delete","dirs":[path_str(la.join("Microsoft").join("Windows").join("INetCache"))],"minAgeDays":7,"maxDepth":6,"desc":"IE/Edge 传统缓存"}),
    ];
    let software_defs: Vec<Value> = vec![
        json!({"groupId":"soft_cache","groupName":"软件缓存","icon":"app","category":"software","checked":true,"risky":false,"type":"app_cache","action":"delete","desc":"已安装软件的缓存与临时文件"}),
        json!({"groupId":"big_files","groupName":"大文件(仅列出)","icon":"file","category":"software","checked":false,"risky":true,"type":"big_files","action":"none","listOnly":true,"desc":"扫描下载/桌面/文档中超过100MB的文件, 仅供查看, 不会自动删除"}),
    ];
    let plugin_defs: Vec<Value> = vec![
        json!({"groupId":"browser_ext","groupName":"浏览器扩展残留","icon":"plugin","category":"plugin","checked":true,"risky":false,"type":"extensions","action":"delete","desc":"Chrome/Edge 扩展目录中的残留"}),
        json!({"groupId":"sys_plugin","groupName":"系统插件残留","icon":"plugin","category":"plugin","checked":true,"risky":false,"type":"sys_plugin","action":"delete","desc":"系统插件与ActiveX残留"}),
    ];
    match tab {
        "trash" => trash_defs,
        "software" => software_defs,
        "all" => {
            let mut v = trash_defs;
            v.extend(
                software_defs
                    .into_iter()
                    .filter(|d| d["groupId"].as_str() != Some("download_installers")),
            );
            v.extend(plugin_defs);
            v
        }
        _ => plugin_defs,
    }
}

/// 构建 walkProgress 进度回调(对齐 js#390-394)
fn make_walk_progress(app: AppHandle, group_name: String, idx: usize, total: usize) -> Box<dyn Fn(Value) + Send + Sync> {
    Box::new(move |p: Value| {
        let path = p.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let cur_path = if path.is_empty() { group_name.clone() } else { path };
        types::send(
            &app,
            "clean:progress",
            json!({
                "currentPath": cur_path,
                "current": idx,
                "total": total,
                "percent": (((idx as f64) + 0.5) / (total.max(1) as f64) * 100.0).round() as i64
            }),
        );
    })
}

/// 对齐 js#387-566 _scanCleanGroup: 按 def.type 分派扫描
async fn scan_clean_group(def: &Value, app: &AppHandle, scan_key: u64) -> Vec<Value> {
    let mut items: Vec<Value> = Vec::new();
    let group_id = def.get("groupId").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let group_name = def.get("groupName").and_then(|v| v.as_str()).unwrap_or("").to_string();

    match def.get("type").and_then(|v| v.as_str()) {
        Some("recycle_bin") => {
            if let Ok(bins) = paths::recycle_bins().await {
                for bin in bins {
                    if is_cancelled_now(scan_key) { break; }
                    if !bin.exists() { continue; }
                    if let Ok(st) = filesystem::get_dir_stats(&bin).await {
                        let size = st.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                        let count = st.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                        if size > 0 || count > 0 {
                            let item_id = format!("it_{}_{}", group_id, sanitize_id_export(bin.to_string_lossy().to_string()));
                            items.push(json!({
                                "id": item_id,
                                "name": format!("回收站 ({})", bin.to_string_lossy()),
                                "desc": "回收站中的文件",
                                "size": size,
                                "count": count,
                                "path": bin.to_string_lossy().to_string(),
                                "safe": true,
                                "checked": true,
                            }));
                        }
                    }
                }
            }
        }
        Some("dir_scan") => {
            let dirs: Vec<String> = def
                .get("dirs")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();
            let dir_len = dirs.len();
            for (di, dir_str) in dirs.iter().enumerate() {
                if is_cancelled_now(scan_key) { break; }
                let dir = PathBuf::from(dir_str);
                if !dir.exists() { continue; }
                let min_age = def.get("minAgeDays").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let max_depth = def.get("maxDepth").and_then(|v| v.as_u64()).unwrap_or(8) as usize;
                let ext = def.get("extFilter").and_then(|v| v.as_str()).map(|s| s.to_string());
                let on_prog = make_walk_progress(app.clone(), group_name.clone(), di, dir_len);
                if let Ok(res) = filesystem::scan_dir(
                    &dir,
                    min_age,
                    max_depth,
                    ext.as_deref(),
                    Some(is_cancelled_box(scan_key)),
                    Some(on_prog),
                )
                .await
                {
                    let total_count = res.get("totalCount").and_then(|v| v.as_u64()).unwrap_or(0);
                    if total_count > 0 {
                        let total_size = res.get("totalSize").and_then(|v| v.as_u64()).unwrap_or(0);
                        let paths_vec: Vec<String> = res
                            .get("files")
                            .and_then(|v| v.as_array())
                            .map(|a| a.iter().filter_map(|f| f.get("path").and_then(|p| p.as_str()).map(|s| s.to_string())).collect())
                            .unwrap_or_default();
                        let item_id = format!("it_{}_{}", group_id, sanitize_id_export(dir_str.clone()));
                        RESULTS.lock().unwrap().insert(format!("{}:{}", group_id, item_id), paths_vec);
                        items.push(json!({
                            "id": item_id,
                            "name": group_name.clone(),
                            "desc": def.get("desc").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            "size": total_size,
                            "count": total_count,
                            "path": dir_str,
                            "safe": true,
                            "checked": true,
                        }));
                    }
                }
            }
        }
        Some("glob_scan") => {
            let dir_str = def.get("dir").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let dir = PathBuf::from(&dir_str);
            if dir.exists() {
                let pattern = def.get("pattern").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                let mut files: Vec<Value> = Vec::new();
                if let Ok(rd) = std::fs::read_dir(&dir) {
                    for entry in rd.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if !name.to_lowercase().starts_with(&pattern) { continue; }
                        if let Ok(meta) = entry.metadata() {
                            if meta.is_file() {
                                files.push(json!({
                                    "path": entry.path().to_string_lossy().to_string(),
                                    "size": meta.len(),
                                }));
                            }
                        }
                    }
                }
                if !files.is_empty() {
                    let total_size: u64 = files.iter().filter_map(|f| f.get("size").and_then(|v| v.as_u64())).sum();
                    let paths_vec: Vec<String> = files.iter().filter_map(|f| f.get("path").and_then(|p| p.as_str()).map(|s| s.to_string())).collect();
                    let item_id = format!("it_{}_{}", group_id, sanitize_id_export(dir_str.clone()));
                    RESULTS.lock().unwrap().insert(format!("{}:{}", group_id, item_id), paths_vec);
                    items.push(json!({
                        "id": item_id,
                        "name": group_name.clone(),
                        "desc": def.get("desc").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        "size": total_size,
                        "count": files.len() as u64,
                        "path": dir_str,
                        "safe": true,
                        "checked": true,
                    }));
                }
            }
        }
        Some("browser_cache") => {
            let procs = process::list_processes().await.unwrap_or_default();
            let running: HashSet<String> = procs
                .iter()
                .filter_map(|p| p.get("name").and_then(|v| v.as_str()).map(|s| s.to_lowercase()))
                .collect();
            let browsers: Vec<(&str, &str, PathBuf, &str)> = vec![
                ("Edge", "msedge.exe", paths::local_app_data().join("Microsoft").join("Edge").join("User Data"), "chromium"),
                ("Chrome", "chrome.exe", paths::local_app_data().join("Google").join("Chrome").join("User Data"), "chromium"),
                ("Firefox", "firefox.exe", paths::local_app_data().join("Mozilla").join("Firefox").join("Profiles"), "firefox"),
            ];
            for (name, proc, base, kind) in browsers.iter() {
                if is_cancelled_now(scan_key) { break; }
                if running.contains(&proc.to_lowercase()) { continue; }
                if !base.exists() { continue; }
                let cache_dirs = find_browser_cache_dirs(kind, base);
                let cd_len = cache_dirs.len();
                for (cdi, cd) in cache_dirs.iter().enumerate() {
                    if is_cancelled_now(scan_key) { break; }
                    let on_prog = make_walk_progress(app.clone(), format!("{}缓存", name), cdi, cd_len);
                    if let Ok(res) = filesystem::scan_dir(cd, 0, 6, None, Some(is_cancelled_box(scan_key)), Some(on_prog)).await {
                        let total_count = res.get("totalCount").and_then(|v| v.as_u64()).unwrap_or(0);
                        if total_count > 0 {
                            let total_size = res.get("totalSize").and_then(|v| v.as_u64()).unwrap_or(0);
                            let paths_vec: Vec<String> = res.get("files").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|f| f.get("path").and_then(|p| p.as_str()).map(|s| s.to_string())).collect()).unwrap_or_default();
                            let item_id = format!("it_{}_{}", group_id, sanitize_id_export(cd.to_string_lossy().into_owned()));
                            RESULTS.lock().unwrap().insert(format!("{}:{}", group_id, item_id), paths_vec);
                            items.push(json!({
                                "id": item_id,
                                "name": format!("{}缓存", name),
                                "desc": cd.to_string_lossy().to_string(),
                                "size": total_size,
                                "count": total_count,
                                "path": cd.to_string_lossy().to_string(),
                                "safe": true,
                                "checked": true,
                            }));
                        }
                    }
                }
            }
        }
        Some("app_cache") => {
            let procs = process::list_processes().await.unwrap_or_default();
            let running: HashSet<String> = procs
                .iter()
                .filter_map(|p| p.get("name").and_then(|v| v.as_str()).map(|s| s.to_lowercase()))
                .collect();
            for app_def in APP_CACHE_DIRS.iter() {
                if is_cancelled_now(scan_key) { break; }
                if running.contains(&app_def.1.to_lowercase()) { continue; }
                let dir_len = app_def.2.len();
                for (di, dir_tpl) in app_def.2.iter().enumerate() {
                    if is_cancelled_now(scan_key) { break; }
                    let resolved = match paths::resolve_path(dir_tpl).await {
                        Ok(Some(p)) => p,
                        _ => continue,
                    };
                    let on_prog = make_walk_progress(app.clone(), format!("{}缓存", app_def.0), di, dir_len);
                    if let Ok(res) = filesystem::scan_dir(&resolved, 7, 6, None, Some(is_cancelled_box(scan_key)), Some(on_prog)).await {
                        let total_count = res.get("totalCount").and_then(|v| v.as_u64()).unwrap_or(0);
                        if total_count > 0 {
                            let total_size = res.get("totalSize").and_then(|v| v.as_u64()).unwrap_or(0);
                            let paths_vec: Vec<String> = res.get("files").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|f| f.get("path").and_then(|p| p.as_str()).map(|s| s.to_string())).collect()).unwrap_or_default();
                            let item_id = format!("it_{}_{}", group_id, sanitize_id_export(resolved.to_string_lossy().into_owned()));
                            RESULTS.lock().unwrap().insert(format!("{}:{}", group_id, item_id), paths_vec);
                            items.push(json!({
                                "id": item_id,
                                "name": format!("{}缓存", app_def.0),
                                "desc": resolved.to_string_lossy().to_string(),
                                "size": total_size,
                                "count": total_count,
                                "path": resolved.to_string_lossy().to_string(),
                                "safe": true,
                                "checked": true,
                            }));
                        }
                    }
                }
            }
        }
        Some("big_files") => {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
            {
                let cache = BIG_CACHE.lock().unwrap();
                if let Some(items_cached) = &cache.items {
                    if now - cache.time < 600000 {
                        for (k, v) in &cache.files_by_item {
                            RESULTS.lock().unwrap().insert(k.clone(), v.clone());
                        }
                        return items_cached.clone();
                    }
                }
            }
            let mut roots: Vec<PathBuf> = Vec::new();
            if let Ok(Some(dl)) = paths::downloads().await {
                roots.push(dl);
            }
            roots.push(paths::user_profile().join("Desktop"));
            roots.push(paths::user_profile().join("Documents"));
            let root_len = roots.len();
            for (ri, root) in roots.iter().enumerate() {
                if is_cancelled_now(scan_key) { break; }
                if !root.exists() { continue; }
                let on_prog = make_walk_progress(app.clone(), format!("大文件 ({})", root.to_string_lossy()), ri, root_len);
                if let Ok(res) = filesystem::scan_dir(root, 0, 4, None, Some(is_cancelled_box(scan_key)), Some(on_prog)).await {
                    let files = res.get("files").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                    let big: Vec<Value> = files
                        .into_iter()
                        .filter(|f| f.get("size").and_then(|s| s.as_u64()).unwrap_or(0) >= 100 * 1024 * 1024)
                        .collect();
                    if !big.is_empty() {
                        let total_size: u64 = big.iter().filter_map(|f| f.get("size").and_then(|v| v.as_u64())).sum();
                        let paths_vec: Vec<String> = big.iter().filter_map(|f| f.get("path").and_then(|p| p.as_str()).map(|s| s.to_string())).collect();
                        let item_id = format!("it_{}_{}", group_id, sanitize_id_export(root.to_string_lossy().into_owned()));
                        RESULTS.lock().unwrap().insert(format!("{}:{}", group_id, item_id), paths_vec);
                        items.push(json!({
                            "id": item_id,
                            "name": format!("大文件 ({})", root.to_string_lossy()),
                            "desc": "超过100MB的文件",
                            "size": total_size,
                            "count": big.len() as u64,
                            "path": root.to_string_lossy().to_string(),
                            "safe": true,
                            "checked": true,
                        }));
                    }
                }
            }
            {
                let mut cache = BIG_CACHE.lock().unwrap();
                cache.time = now;
                cache.items = Some(items.clone());
                let mut fbi: HashMap<String, Vec<String>> = HashMap::new();
                for (k, v) in RESULTS.lock().unwrap().iter() {
                    if k.starts_with(&format!("{}:", group_id)) {
                        fbi.insert(k.clone(), v.clone());
                    }
                }
                cache.files_by_item = fbi;
            }
        }
        Some("extensions") => {
            let bases: Vec<PathBuf> = vec![
                paths::local_app_data().join("Google").join("Chrome").join("User Data").join("Default").join("Extensions"),
                paths::local_app_data().join("Microsoft").join("Edge").join("User Data").join("Default").join("Extensions"),
            ];
            for base in bases.iter() {
                if is_cancelled_now(scan_key) { break; }
                if !base.exists() { continue; }
                if let Ok(rd) = std::fs::read_dir(base) {
                    for entry in rd.flatten() {
                        if is_cancelled_now(scan_key) { break; }
                        let ext_id = entry.file_name().to_string_lossy().to_string();
                        if !(ext_id.len() == 32 && ext_id.chars().all(|c| c.is_ascii_alphabetic())) {
                            continue;
                        }
                        let ext_dir = entry.path();
                        if let Ok(size) = filesystem::get_dir_size(&ext_dir).await {
                            if size > 0 {
                                let item_id = format!("it_{}_{}", group_id, sanitize_id_export(ext_dir.to_string_lossy().into_owned()));
                                RESULTS.lock().unwrap().insert(format!("{}:{}", group_id, item_id), vec![ext_dir.to_string_lossy().into_owned()]);
                                let prefix: String = ext_id.chars().take(8).collect();
                                items.push(json!({
                                    "id": item_id,
                                    "name": format!("扩展 {}...", prefix),
                                    "desc": ext_dir.to_string_lossy().to_string(),
                                    "size": size,
                                    "count": 1,
                                    "path": ext_dir.to_string_lossy().to_string(),
                                    "safe": true,
                                    "checked": true,
                                }));
                            }
                        }
                    }
                }
            }
        }
        Some("sys_plugin") => {
            let dirs: Vec<PathBuf> = vec![
                paths::system_root().join("Downloaded Program Files"),
                paths::app_data().join("Microsoft").join("AddIns"),
            ];
            for dir in dirs.iter() {
                if is_cancelled_now(scan_key) { break; }
                if !dir.exists() { continue; }
                if let Ok(res) = filesystem::scan_dir(dir, 0, 3, None, Some(is_cancelled_box(scan_key)), None).await {
                    let total_count = res.get("totalCount").and_then(|v| v.as_u64()).unwrap_or(0);
                    if total_count > 0 {
                        let total_size = res.get("totalSize").and_then(|v| v.as_u64()).unwrap_or(0);
                        let paths_vec: Vec<String> = res.get("files").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|f| f.get("path").and_then(|p| p.as_str()).map(|s| s.to_string())).collect()).unwrap_or_default();
                        let item_id = format!("it_{}_{}", group_id, sanitize_id_export(dir.to_string_lossy().into_owned()));
                        RESULTS.lock().unwrap().insert(format!("{}:{}", group_id, item_id), paths_vec);
                        items.push(json!({
                            "id": item_id,
                            "name": format!("插件残留 ({})", dir.to_string_lossy()),
                            "desc": dir.to_string_lossy().to_string(),
                            "size": total_size,
                            "count": total_count,
                            "path": dir.to_string_lossy().to_string(),
                            "safe": true,
                            "checked": true,
                        }));
                    }
                }
            }
        }
        _ => {}
    }
    items
}

/// 对齐 js#302-342 scanClean: 编排分组扫描 + 事件投递
pub async fn scan_clean(app: AppHandle, tab: String, scan_key: u64) {
    CANCELLED.store(false, Ordering::SeqCst);
    set_running(true);
    RESULTS.lock().unwrap().clear();
    GROUP_DEFS.lock().unwrap().clear();
    let is_cancelled = || is_cancelled_now(scan_key);
    let defs = build_group_defs(&tab).await;
    let len = defs.len();
    let mut groups: Vec<Value> = Vec::new();
    let mut total_size: u64 = 0;
    let mut total_count: u64 = 0;
    for (i, def) in defs.iter().enumerate() {
        if is_cancelled() { break; }
        let group_id = def.get("groupId").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let group_name = def.get("groupName").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let icon = def.get("icon").and_then(|v| v.as_str()).unwrap_or("").to_string();
        GROUP_DEFS.lock().unwrap().insert(group_id.clone(), def.clone());
        types::send(
            &app,
            "clean:progress",
            json!({
                "currentPath": group_name,
                "current": i,
                "total": len,
                "percent": ((i as f64 / len.max(1) as f64) * 100.0).round() as i64
            }),
        );
        let group_checked = def.get("checked").and_then(|v| v.as_bool()).unwrap_or(false)
            && !def.get("risky").and_then(|v| v.as_bool()).unwrap_or(false);
        types::send(
            &app,
            "clean:group-status",
            json!({
                "groupId": group_id,
                "groupName": group_name,
                "icon": icon,
                "status": "scanning",
                "scanKey": scan_key,
                "found": [],
                "checked": group_checked
            }),
        );
        let mut items = scan_clean_group(def, &app, scan_key).await;
        for it in items.iter_mut() {
            it["checked"] = json!(group_checked);
            it["risky"] = json!(def.get("risky").and_then(|v| v.as_bool()).unwrap_or(false));
        }
        if is_cancelled() { break; }
        let g_size: u64 = items.iter().map(|it| it.get("size").and_then(|v| v.as_u64()).unwrap_or(0)).sum();
        let g_count: u64 = items.iter().map(|it| it.get("count").and_then(|v| v.as_u64()).unwrap_or(0)).sum();
        total_size += g_size;
        total_count += g_count;
        let extra = items.len();
        types::send(
            &app,
            "clean:group-status",
            json!({
                "groupId": group_id,
                "groupName": group_name,
                "icon": icon,
                "status": "done",
                "scanKey": scan_key,
                "found": items.clone(),
                "checked": group_checked
            }),
        );
        let risky = def.get("risky").and_then(|v| v.as_bool()).unwrap_or(false);
        groups.push(json!({
            "groupId": group_id,
            "groupName": group_name,
            "icon": icon,
            "category": tab,
            "checked": group_checked,
            "risky": risky,
            "items": items,
            "expandable": true,
            "extraCount": extra,
        }));
    }
    if !is_cancelled() {
        types::send(
            &app,
            "clean:done",
            json!({ "totalSize": total_size, "totalCount": total_count, "groups": groups, "scanKey": scan_key }),
        );
    }
    set_running(false);
}



