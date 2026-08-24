//! 启动项管理 —— 完整移植自 Electron 版 business/startupManager.js
//! 对齐 JS 行号注释, 保持 API 签名与字段名一致

use crate::rules_engine::{get_boot_time_ref, get_safe_speed_boot, get_startup_rules};
use crate::system::exec::run_async;
use crate::system::paths::{app_data, program_data};
use crate::system::registry::{ensure_key, list_values, query_sub_keys, set_binary, set_string, delete_value};
use crate::system::service::{is_protected, list_services, set_service_start_type};
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

// =================== 常量区 (对齐 startupManager.js#10-24) ===================

/// 启动审批注册表键路径
const APPROVED_KEYS_HKCU: &str = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";
const APPROVED_KEYS_HKLM: &str = "HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";

/// Run 键定义数组 —— 对齐 RUN_KEY_DEFS
const RUN_KEY_DEFS: &[(&str, &str, Option<&str>)] = &[
    ("hkcu_run", "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run", Some(APPROVED_KEYS_HKCU)),
    ("hklm_run", "HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\Run", Some(APPROVED_KEYS_HKLM)),
    ("runonce", "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce", None),
    ("runonce", "HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce", None),
];

/// 启用/禁用二进制值 —— 对齐 ENABLED_BINARY / DISABLED_BINARY
const ENABLED_BINARY_HEX: &str = "02000000000000000000";
const DISABLED_BINARY_HEX: &str = "03000000000000000000";

// =================== 缓存区 (对齐 startupManager.js#25-92) ===================

/// 项元数据缓存: id -> { kind, runKey, approvedKey, valueName, command, essential, ... }
static ITEM_META: Lazy<RwLock<HashMap<String, Value>>> = Lazy::new(|| RwLock::new(HashMap::new()));

/// 启动时间基准缓存
static BOOT_TIME_REF_CACHE: Lazy<RwLock<Option<Value>>> = Lazy::new(|| RwLock::new(None));

/// 规则索引缓存
static RULE_INDEX_CACHE: Lazy<RwLock<Option<Value>>> = Lazy::new(|| RwLock::new(None));

/// SafeSpeedBoot 数据缓存
static SSB_CACHE: Lazy<RwLock<Option<Value>>> = Lazy::new(|| RwLock::new(None));

/// 360 关键/安全服务集合 (DsArkWhiteService)
static DS_ARK_WHITE_CACHE: Lazy<RwLock<Option<HashSet<String>>>> = Lazy::new(|| RwLock::new(None));

/// 计划任务缓存 (TTL 30s)
static TASKS_CACHE: Lazy<Mutex<Option<(Vec<Value>, u128)>>> = Lazy::new(|| Mutex::new(None));

/// CLSID 解析缓存
static CLSID_INFO_CACHE: Lazy<RwLock<HashMap<String, Value>>> = Lazy::new(|| RwLock::new(HashMap::new()));

/// 启动项 exe 基名 -> 耗时映射缓存
static BOOT_EXE_MAP_CACHE: Lazy<RwLock<Option<HashMap<String, u64>>>> = Lazy::new(|| RwLock::new(None));

/// 忽略列表缓存
static IGNORE_CACHE: Lazy<RwLock<Option<HashSet<String>>>> = Lazy::new(|| RwLock::new(None));

// =================== 辅助函数区 (对齐 startupManager.js#48-173) ===================

/// 清洗 ID: 仅保留字母数字下划线中划线, 压缩连续下划线, 去首尾下划线
/// 对齐 sanitizeId() #48-50
fn sanitize_id(s: &str) -> String {
    let mut result = String::new();
    let mut prev_underscore = false;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            if ch == '_' {
                if !prev_underscore {
                    result.push('_');
                    prev_underscore = true;
                }
            } else {
                result.push(ch);
                prev_underscore = false;
            }
        } else if !prev_underscore {
            result.push('_');
            prev_underscore = true;
        }
    }
    // 去首尾下划线
    result.trim_matches('_').to_string()
}

/// 从命令行提取 exe 路径 —— 对齐 extractExeFromCommand() #52-61
fn extract_exe_from_command(cmd: &str) -> Option<String> {
    if cmd.is_empty() {
        return None;
    }
    // 优先匹配引号包裹的 .exe
    if let Some(caps) = regex_captures(r#""([^"]+\.exe)""#, cmd) {
        return Some(caps[1].to_string());
    }
    // 次选匹配盘符开头的 .exe 路径
    if let Some(caps) = regex_captures(r#"([A-Za-z]:\\[^"]*?\.exe)"#, cmd) {
        return Some(caps[1].to_string());
    }
    // 兜底: 按空格分割找 .exe
    for part in cmd.split_whitespace() {
        if part.to_lowercase().ends_with(".exe") {
            return Some(part.trim_matches('"').to_string());
        }
    }
    None
}

/// 简易正则捕获辅助 (避免引入 regex crate)
/// 返回值遵守 regex crate Captures 契约: ret[0] = 整体匹配, ret[1] = 捕获组1
fn regex_captures(pattern: &str, text: &str) -> Option<Vec<String>> {
    // 仅支持本文件用到的两种简单模式, 手工实现
    if pattern == r#""([^"]+\.exe)""# {
        // 整体匹配 "xxx.exe", 捕获组为 xxx.exe
        let mut in_quotes = false;
        let mut start = 0;
        for (i, ch) in text.char_indices() {
            if ch == '"' {
                if !in_quotes {
                    in_quotes = true;
                    start = i + 1;
                } else {
                    let candidate = &text[start..i];
                    if candidate.to_lowercase().ends_with(".exe") {
                        return Some(vec![format!("\"{}\"", candidate), candidate.to_string()]);
                    }
                    in_quotes = false;
                }
            }
        }
    } else if pattern == r#"([A-Za-z]:\\[^"]*?\.exe)"# {
        // 整个模式即捕获组: 整体匹配与组相同
        let lower = text.to_lowercase();
        if let Some(pos) = lower.find(".exe") {
            // 向前找盘符
            let before = &text[..=pos + 3];
            if let Some(drive_pos) = before.rfind(':') {
                if drive_pos >= 1 && before[drive_pos - 1..].chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
                    let candidate = &before[drive_pos - 1..];
                    if candidate.contains('\\') {
                        return Some(vec![candidate.to_string(), candidate.to_string()]);
                    }
                }
            }
        }
    }
    None
}

/// 从规则 target 提取 exe/dll 名 —— 对齐 extractRuleTarget() #63-74
fn extract_rule_target(rule: &Value) -> String {
    let target = rule.get("target").and_then(|v| v.as_str()).unwrap_or("");
    let rule_type = rule.get("type").and_then(|v| v.as_u64()).unwrap_or(0);
    if rule_type == 1 {
        let parts: Vec<&str> = target.split(',').collect();
        for part in &parts {
            let p = part.trim();
            if p.to_lowercase().ends_with(".exe")
                || p.to_lowercase().ends_with(".dll")
                || p.to_lowercase().ends_with(".com")
                || p.to_lowercase().ends_with(".bat")
                || p.to_lowercase().ends_with(".cmd")
                || p.to_lowercase().ends_with(".lnk")
            {
                return p.to_string();
            }
        }
        if parts.len() > 1 {
            return parts[1].trim().to_string();
        }
        return String::new();
    }
    target.trim().to_string()
}

/// 构建规则索引: byExe, byName —— 对齐 buildRuleIndex() #76-86
fn build_rule_index(rules: &Value) -> Value {
    let mut by_exe = HashMap::new();
    let mut by_name = HashMap::new();
    if let Some(arr) = rules.as_array() {
        for r in arr {
            let exe = extract_rule_target(r).to_lowercase();
            if !exe.is_empty()
                && (exe.ends_with(".exe")
                    || exe.ends_with(".dll")
                    || exe.ends_with(".com")
                    || exe.ends_with(".bat")
                    || exe.ends_with(".cmd")
                    || exe.ends_with(".lnk"))
                && !by_exe.contains_key(&exe)
            {
                by_exe.insert(exe, r.clone());
            }
            let nm = r.get("name").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
            if nm.len() > 1 && !by_name.contains_key(&nm) {
                by_name.insert(nm, r.clone());
            }
        }
    }
    json!({ "byExe": by_exe, "byName": by_name })
}

/// 匹配规则 —— 对齐 matchRule() #124-135
fn match_rule(index: &Value, name: Option<&str>, target_exe: Option<&str>) -> Option<Value> {
    if let Some(exe) = target_exe {
        let base = Path::new(exe).file_name().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
        if let Some(r) = index.get("byExe").and_then(|v| v.get(&base)) {
            return Some(r.clone());
        }
    }
    if let Some(n) = name {
        let lower = n.to_lowercase();
        if let Some(r) = index.get("byName").and_then(|v| v.get(&lower)) {
            return Some(r.clone());
        }
    }
    None
}

/// 计算禁止率 —— 对齐 computeBanRate() #137-158
fn compute_ban_rate(rule: Option<&Value>, enabled: bool) -> u32 {
    let rate = if let Some(r) = rule {
        match r.get("type").and_then(|v| v.as_u64()).unwrap_or(0) {
            4 => 85,
            16 => 75,
            1 => 40,
            21 => 45,
            15 => 50,
            12 => 55,
            13 => 35,
            10 => 45,
            11 => 60,
            2 => 5,
            9 => 5,
            _ => 30,
        }
    } else {
        30
    };
    let mut rate: u32 = rate;
    if !enabled {
        rate = rate.saturating_sub(15);
    }
    rate.clamp(0, 100)
}

/// 判断是否为系统关键组件 —— 对齐 isSystemEssential() #160-166
fn is_system_essential(rule: Option<&Value>, source: &str, name: &str) -> bool {
    if source == "service" && is_protected(name) {
        return true;
    }
    if let Some(r) = rule {
        let t = r.get("type").and_then(|v| v.as_u64()).unwrap_or(0);
        if t == 2 || t == 9 {
            return true;
        }
    }
    // 360 关键/安全服务 (DsArkWhiteService)
    if source == "service" {
        if let Some(cache) = DS_ARK_WHITE_CACHE.read().unwrap().as_ref() {
            if cache.contains(&name.to_lowercase()) {
                return true;
            }
        }
    }
    false
}

/// 计算建议文案 —— 对齐 computeSuggestion() #168-173
fn compute_suggestion(ban_rate: u32, essential: bool) -> &'static str {
    if essential {
        return "维持现状";
    }
    if ban_rate >= 60 {
        return "建议禁用";
    }
    if ban_rate <= 10 {
        return "建议开启";
    }
    "维持现状"
}

/// CSV 解析 (兼容 schtasks /fo csv 输出) —— 对齐 parseCsv() #177-201
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_quotes {
            if c == '"' {
                if i + 1 < chars.len() && chars[i + 1] == '"' {
                    field.push('"');
                    i += 1;
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == ',' {
            row.push(field);
            field = String::new();
        } else if c == '\n' {
            row.push(field);
            rows.push(row);
            row = Vec::new();
            field = String::new();
        } else if c != '\r' {
            field.push(c);
        }
        i += 1;
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

// =================== 核心异步函数区 (对齐 startupManager.js#28-122) ===================

/// 获取启动时间基准映射 —— 对齐 getBootTimeRefMap() #28-32
async fn get_boot_time_ref_map() -> Result<Value, String> {
    // 快速路径: 缓存命中
    if let Some(cached) = BOOT_TIME_REF_CACHE.read().unwrap().as_ref() {
        return Ok(cached.clone());
    }
    let map = get_boot_time_ref().await?;
    *BOOT_TIME_REF_CACHE.write().unwrap() = Some(map.clone());
    Ok(map)
}

/// 查找启动耗时 (服务名/ exe 基名) —— 对齐 lookupStartupTime() #35-46
async fn lookup_startup_time(_source: &str, name: Option<&str>, exe: Option<&str>) -> Option<u64> {
    let map = get_boot_time_ref_map().await.ok()?;
    if let Some(n) = name {
        if let Some(v) = map.get(n) {
            return v.as_u64();
        }
    }
    if let Some(e) = exe {
        let base = Path::new(e).file_name().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
        if let Some(v) = map.get(&base) {
            return v.as_u64();
        }
        // 遍历查找大小写不敏感匹配
        for (k, v) in map.as_object()? {
            if k.to_lowercase() == base {
                return v.as_u64();
            }
        }
    }
    None
}

/// 获取 SafeSpeedBoot 数据 —— 对齐 getSafeSpeedBootData() #94-98
async fn get_safe_speed_boot_data() -> Result<Value, String> {
    if let Some(cached) = SSB_CACHE.read().unwrap().as_ref() {
        return Ok(cached.clone());
    }
    let data = get_safe_speed_boot().await?;
    *SSB_CACHE.write().unwrap() = Some(data.clone());
    Ok(data)
}

/// 获取规则索引 (融合 startupRules + safeSpeedBoot) —— 对齐 getRuleIndex() #100-122
async fn get_rule_index() -> Result<Value, String> {
    if let Some(cached) = RULE_INDEX_CACHE.read().unwrap().as_ref() {
        return Ok(cached.clone());
    }
    let rules = get_startup_rules().await?;
    let mut index = build_rule_index(&rules);
    // 融合 360 safeSpeedBoot 数据
    let ssb = get_safe_speed_boot_data().await?;
    let ds_ark_white: HashSet<String> = ssb
        .get("dsArkWhite")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_lowercase())).collect())
        .unwrap_or_default();
    *DS_ARK_WHITE_CACHE.write().unwrap() = Some(ds_ark_white);
    if let Some(startup_arr) = ssb.get("startup").and_then(|v| v.as_array()) {
        for name in startup_arr {
            if let Some(nm) = name.as_str() {
                let lower = nm.to_lowercase();
                if lower.len() > 1 && !index.get("byName").and_then(|v| v.get(&lower)).is_some() {
                    if let Some(obj) = index.get_mut("byName") {
                        if let Some(map) = obj.as_object_mut() {
                            map.insert(
                                lower,
                                json!({ "name": nm, "type": 1, "target": "", "source": "safespeedboot" }),
                            );
                        }
                    }
                }
            }
        }
    }
    if let Some(services_arr) = ssb.get("services").and_then(|v| v.as_array()) {
        for name in services_arr {
            if let Some(nm) = name.as_str() {
                let lower = nm.to_lowercase();
                if lower.len() > 1 && !index.get("byName").and_then(|v| v.get(&lower)).is_some() {
                    if let Some(obj) = index.get_mut("byName") {
                        if let Some(map) = obj.as_object_mut() {
                            map.insert(
                                lower,
                                json!({ "name": nm, "type": 12, "target": "", "source": "safespeedboot" }),
                            );
                        }
                    }
                }
            }
        }
    }
    *RULE_INDEX_CACHE.write().unwrap() = Some(index.clone());
    Ok(index)
}

// =================== 计划任务 (对齐 startupManager.js#206-248) ===================

/// 列举计划任务 (TTL 30s 缓存) —— 对齐 listScheduledTasks() #206-248
async fn list_scheduled_tasks() -> Result<Vec<Value>, String> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
    // 检查缓存
    {
        let cache = TASKS_CACHE.lock().unwrap();
        if let Some((ref tasks, cached_time)) = *cache {
            if now - cached_time < 30_000 {
                return Ok(tasks.clone());
            }
        }
    }
    // 执行 schtasks /query /fo csv /v
    let res = run_async("schtasks", &["/query", "/fo", "csv", "/v"], None).await?;
    if res.trim().is_empty() {
        let empty: Vec<Value> = Vec::new();
        *TASKS_CACHE.lock().unwrap() = Some((empty.clone(), now));
        return Ok(empty);
    }
    let rows = parse_csv(&res);
    if rows.len() < 2 {
        let empty: Vec<Value> = Vec::new();
        *TASKS_CACHE.lock().unwrap() = Some((empty.clone(), now));
        return Ok(empty);
    }
    let headers = &rows[0];
    // 列索引兼容中英文系统
    let idx_name = headers.iter().position(|h| h.contains("任务名") || h.to_lowercase().contains("taskname")).unwrap_or(0);
    let idx_status = headers.iter().position(|h| h.contains("计划任务状态") || h.to_lowercase().contains("scheduled task state")).unwrap_or(1);
    let idx_command = headers.iter().position(|h| h.contains("要运行的任务") || h.to_lowercase().contains("task to run")).unwrap_or(2);
    let idx_creator = headers.iter().position(|h| h.contains("创建者") || h.to_lowercase().contains("author")).unwrap_or(3);
    let idx_trigger = headers.iter().position(|h| h.to_lowercase().contains("schedule type") || h.contains("计划类型")).unwrap_or(4);
    let header_name = headers.get(idx_name).cloned().unwrap_or_default();
    let mut tasks = Vec::new();
    for row in rows.iter().skip(1) {
        let name = row.get(idx_name).cloned().unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        // 跳过表头泄漏行
        if !header_name.is_empty() && name == header_name {
            continue;
        }
        let status = row.get(idx_status).cloned().unwrap_or_default();
        let command = row.get(idx_command).cloned().unwrap_or_default();
        let creator = row.get(idx_creator).cloned().unwrap_or_default();
        let trigger = if idx_trigger < row.len() { row[idx_trigger].clone() } else { String::new() };
        tasks.push(json!({
            "name": name,
            "command": command,
            "creator": creator,
            "trigger": trigger,
            "enabled": !status.contains("禁用"),
            "isMicrosoft": name.starts_with("\\Microsoft\\"),
        }));
    }
    *TASKS_CACHE.lock().unwrap() = Some((tasks.clone(), now));
    Ok(tasks)
}

// =================== 启动文件夹 (对齐 startupManager.js#252-278) ===================

/// 解析快捷方式目标路径 —— 对齐 resolveShortcut() #252-258
async fn resolve_shortcut(lnk_path: &str) -> Option<String> {
    let safe = lnk_path.replace('\'', "''");
    let script = format!("$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}'); $s.TargetPath", safe);
    let res = run_async("powershell", &["-NoProfile", "-NonInteractive", "-Command", &script], None).await.ok()?;
    let t = res.trim();
    if t.is_empty() { None } else { Some(t.to_string()) }
}

/// 列举启动文件夹快捷方式 —— 对齐 listStartupFolder() #260-278
async fn list_startup_folder() -> Result<Vec<Value>, String> {
    let folders = [
        app_data().join("Microsoft").join("Windows").join("Start Menu").join("Programs").join("Startup"),
        program_data().join("Microsoft").join("Windows").join("Start Menu").join("Programs").join("StartUp"),
    ];
    let mut items = Vec::new();
    for folder in folders {
        if !folder.exists() {
            continue;
        }
        let files = match fs::read_dir(&folder) {
            Ok(f) => f,
            Err(_) => continue,
        };
        for entry in files.flatten() {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if !file_name.to_lowercase().ends_with(".lnk") {
                continue;
            }
            let full = entry.path();
            let target = resolve_shortcut(&full.to_string_lossy()).await;
            items.push(json!({
                "name": file_name.trim_end_matches(".lnk").trim_end_matches(".LNK"),
                "path": full.to_string_lossy().to_string(),
                "target": target,
            }));
        }
    }
    Ok(items)
}

// =================== 列举入口 (对齐 startupManager.js#282-290) ===================

/// 主列举函数 —— 对齐 list() #282-290
pub async fn list(tab: String) -> Value {
    // 清空元数据缓存
    ITEM_META.write().unwrap().clear();
    let index = match get_rule_index().await {
        Ok(idx) => idx,
        Err(e) => return json!({ "error": e }),
    };
    match tab.as_str() {
        "system" => list_system(index).await,
        "scheduled" => list_scheduled(index).await,
        "contextmenu" => list_context_menu_handlers().await,
        "explorerplugin" => list_explorer_plugins().await,
        _ => list_software(index).await,
    }
}

// =================== 忽略列表加载 (对齐 startupManager.js#776-786) ===================

/// 忽略文件路径
fn ignore_file_path() -> PathBuf {
    app_data().join("com.opencode.systemcleaner").join("startup_ignore.json")
}

/// 加载忽略列表 —— 对齐 loadIgnore() #776-786
async fn load_ignore() -> HashSet<String> {
    if let Some(cached) = IGNORE_CACHE.read().unwrap().as_ref() {
        return cached.clone();
    }
    let path = ignore_file_path();
    let set = if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(arr) = serde_json::from_str::<Vec<String>>(&content) {
            arr.into_iter().collect()
        } else {
            HashSet::new()
        }
    } else {
        HashSet::new()
    };
    *IGNORE_CACHE.write().unwrap() = Some(set.clone());
    set
}

// =================== listSoftware (对齐 startupManager.js#292-356) ===================

/// 列举软件启动项 (Run/RunOnce + 启动文件夹) —— 对齐 listSoftware() #292-356
async fn list_software(index: Value) -> Value {
    let mut items = Vec::new();
    let ignore = load_ignore().await;
    // 注册表 Run/RunOnce
    for (source, run_key, approved_key) in RUN_KEY_DEFS {
        let values = match list_values(run_key).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        if values.is_empty() {
            continue;
        }
        let approved_map: HashMap<String, String> = if let Some(ak) = approved_key {
            list_values(ak).await.unwrap_or_default().into_iter().filter_map(|v| {
                v.get("name").and_then(|n| n.as_str()).zip(v.get("value").and_then(|v| v.as_str())).map(|(n, v)| (n.to_string(), v.to_string()))
            }).collect()
        } else {
            HashMap::new()
        };
        for v in values {
            let value_name = v.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let value_data = v.get("value").and_then(|v| v.as_str()).unwrap_or("");
            let enabled = !approved_map.contains_key(value_name) || approved_map.get(value_name).map(|s| s.starts_with("02")).unwrap_or(true);
            let exe = extract_exe_from_command(value_data);
            let rule = match_rule(&index, Some(value_name), exe.as_deref());
            let ban_rate = compute_ban_rate(rule.as_ref(), enabled);
            let essential = is_system_essential(rule.as_ref(), source, value_name);
            let id = format!("{}_{}", source, sanitize_id(value_name));
            ITEM_META.write().unwrap().insert(id.clone(), json!({
                "kind": "registry",
                "runKey": run_key,
                "approvedKey": approved_key,
                "valueName": value_name,
                "command": value_data,
                "essential": essential,
            }));
            let startup_time = if let Some(exe) = &exe { lookup_startup_time(source, Some(value_name), Some(exe)).await } else { None };
            items.push(json!({
                "id": id,
                "name": value_name,
                "desc": value_data,
                "icon": "app",
                "banRate": ban_rate,
                "suggestion": compute_suggestion(ban_rate, essential),
                "enabled": enabled,
                "source": source,
                "target": exe.unwrap_or_else(|| value_data.to_string()),
                "startupTime": startup_time,
                "ignored": ignore.contains(&id),
                "canToggle": true,
                "settings": ["打开所在目录", "删除启动项"],
            }));
        }
    }
    // 启动文件夹
    let shortcuts = match list_startup_folder().await {
        Ok(s) => s,
        Err(_) => Vec::new(),
    };
    for sc in shortcuts {
        let name = sc.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let target = sc.get("target").and_then(|v| v.as_str());
        let rule = match_rule(&index, Some(name), target);
        let ban_rate = compute_ban_rate(rule.as_ref(), true);
        let essential = is_system_essential(rule.as_ref(), "startup_folder", name);
        let id = format!("startup_folder_{}", sanitize_id(name));
        ITEM_META.write().unwrap().insert(id.clone(), json!({
            "kind": "startup_folder",
            "lnkPath": sc.get("path").and_then(|v| v.as_str()).unwrap_or(""),
            "essential": essential,
        }));
        let startup_time = if let Some(t) = target {
            lookup_startup_time("startup_folder", Some(name), Some(t)).await
        } else {
            None
        };
        items.push(json!({
            "id": id,
            "name": name,
            "desc": target.unwrap_or_else(|| sc.get("path").and_then(|v| v.as_str()).unwrap_or("")),
            "icon": "app",
            "banRate": ban_rate,
            "suggestion": compute_suggestion(ban_rate, essential),
            "enabled": true,
            "source": "startup_folder",
            "target": target.unwrap_or_else(|| sc.get("path").and_then(|v| v.as_str()).unwrap_or("")),
            "startupTime": startup_time,
            "ignored": ignore.contains(&id),
            "canToggle": true,
            "settings": ["打开所在目录", "删除启动项"],
        }));
    }
    // 排序: 启用项在前, 其次禁止率高的在前
    items.sort_by(|a, b| {
        let a_en = a.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
        let b_en = b.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
        if a_en != b_en {
            return if a_en { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
        }
        let a_br = a.get("banRate").and_then(|v| v.as_u64()).unwrap_or(0);
        let b_br = b.get("banRate").and_then(|v| v.as_u64()).unwrap_or(0);
        b_br.cmp(&a_br)
    });
    json!(items)
}

// =================== listScheduled (对齐 startupManager.js#358-401) ===================

/// 列举计划任务启动项 —— 对齐 listScheduled() #358-401
async fn list_scheduled(index: Value) -> Value {
    let mut items = Vec::new();
    let ignore = load_ignore().await;
    let tasks = match list_scheduled_tasks().await {
        Ok(t) => t,
        Err(_) => return json!(items),
    };
    for t in tasks {
        if t.get("isMicrosoft").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }
        // 只保留「登录时 / 系统启动时」触发的计划任务
        let trigger = t.get("trigger").and_then(|v| v.as_str()).unwrap_or("");
        if !regex_is_match(r"(?i)logon|start\s*up|startup|登陆|启动", trigger) {
            continue;
        }
        let name = t.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let command = t.get("command").and_then(|v| v.as_str()).unwrap_or("");
        let enabled = t.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
        let exe = extract_exe_from_command(command);
        let rule = match_rule(&index, Some(name), exe.as_deref());
        let ban_rate = compute_ban_rate(rule.as_ref(), enabled);
        let essential = is_system_essential(rule.as_ref(), "scheduled_task", name);
        let id = format!("scheduled_task_{}", sanitize_id(name));
        ITEM_META.write().unwrap().insert(id.clone(), json!({
            "kind": "task",
            "taskName": name,
            "essential": essential,
        }));
        // 友好显示名
        let clean_name = name.trim_start_matches('\\')
            .replace(|c: char| c == '_' || c == '-', " ")
            .trim()
            .to_string();
        let mut disp_name = clean_name.clone();
        if let Some(exe) = &exe {
            let pretty_exe = Path::new(exe).file_stem().and_then(|s| s.to_str()).unwrap_or("").replace(['-', '_'], " ").trim().to_string();
            if !pretty_exe.is_empty() && (pretty_exe.chars().any(|c| c.is_uppercase()) || pretty_exe.contains(' ')) {
                disp_name = pretty_exe;
            }
        }
        let startup_time = if let Some(e) = &exe {
            lookup_startup_time("scheduled_task", Some(name), Some(e)).await
        } else {
            None
        };
        items.push(json!({
            "id": id,
            "name": disp_name,
            "desc": clean_name,
            "icon": "task",
            "banRate": ban_rate,
            "suggestion": compute_suggestion(ban_rate, essential),
            "enabled": enabled,
            "source": "scheduled_task",
            "target": exe.clone().unwrap_or_else(|| command.to_string()),
            "startupTime": startup_time,
            "ignored": ignore.contains(&id),
            "canToggle": true,
            "settings": ["打开所在目录", "删除启动项"],
        }));
    }
    items.sort_by(|a, b| {
        let a_en = a.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
        let b_en = b.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
        if a_en != b_en {
            return if a_en { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
        }
        let a_br = a.get("banRate").and_then(|v| v.as_u64()).unwrap_or(0);
        let b_br = b.get("banRate").and_then(|v| v.as_u64()).unwrap_or(0);
        b_br.cmp(&a_br)
    });
    json!(items)
}

// =================== listSystem (对齐 startupManager.js#403-439) ===================

/// 列举系统服务启动项 —— 对齐 listSystem() #403-439
async fn list_system(index: Value) -> Value {
    let mut items = Vec::new();
    let ignore = load_ignore().await;
    let boot_ref = match get_boot_time_ref_map().await {
        Ok(m) => m,
        Err(_) => json!({}),
    };
    let services = match list_services().await {
        Ok(s) => s,
        Err(_) => return json!(items),
    };
    for s in services {
        let start_type = s.get("startType").and_then(|v| v.as_str()).unwrap_or("");
        if start_type != "AUTO_START" {
            continue;
        }
        let name = s.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let binary_path = s.get("binaryPath").and_then(|v| v.as_str());
        let exe_base = binary_path.and_then(|p| Path::new(p).file_name().and_then(|s| s.to_str()));
        let rule = match_rule(&index, Some(name), exe_base);
        let ban_rate = compute_ban_rate(rule.as_ref(), true);
        let essential = is_system_essential(rule.as_ref(), "service", name);
        // 360 关键/安全服务: 禁止率清零
        let is_ds_ark = DS_ARK_WHITE_CACHE.read().unwrap().as_ref().map(|set| set.contains(&name.to_lowercase())).unwrap_or(false);
        let eff_ban_rate = if is_ds_ark { 0 } else { ban_rate };
        let id = format!("service_{}", sanitize_id(name));
        let start_type_code = s.get("startTypeCode").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        ITEM_META.write().unwrap().insert(id.clone(), json!({
            "kind": "service",
            "serviceName": name,
            "essential": essential,
            "startTypeCode": start_type_code,
        }));
        let startup_time = boot_ref.get(name).and_then(|v| v.as_u64());
        items.push(json!({
            "id": id,
            "name": name,
            "desc": binary_path.unwrap_or("系统服务"),
            "icon": "service",
            "banRate": eff_ban_rate,
            "suggestion": compute_suggestion(eff_ban_rate, essential),
            "enabled": true,
            "source": "service",
            "target": binary_path.unwrap_or(name),
            "startupTime": startup_time,
            "ignored": ignore.contains(&id),
            "canToggle": true,
            "settings": ["打开所在目录", "删除启动项"],
        }));
    }
    // 系统服务按禁止率降序
    items.sort_by(|a, b| {
        let a_br = a.get("banRate").and_then(|v| v.as_u64()).unwrap_or(0);
        let b_br = b.get("banRate").and_then(|v| v.as_u64()).unwrap_or(0);
        b_br.cmp(&a_br)
    });
    json!(items)
}

// =================== 正则辅助 (简单实现, 避免引入 regex crate) ===================

/// 简单正则匹配: 仅支持本文件用到的模式
fn regex_is_match(pattern: &str, text: &str) -> bool {
    // 支持 (?i) 前缀表示忽略大小写
    let (case_insensitive, pat) = if pattern.starts_with("(?i)") {
        (true, &pattern[4..])
    } else {
        (false, pattern)
    };
    let text_cmp = if case_insensitive { text.to_lowercase() } else { text.to_string() };
    let pat_cmp = if case_insensitive { pat.to_lowercase() } else { pat.to_string() };
    // 简单的 | 分隔替代匹配
    for part in pat_cmp.split('|') {
        if text_cmp.contains(part) {
            return true;
        }
    }
    false
}

/// 判断是否为默认值名 (本地化变体) —— 对齐 isDefaultValueName() #445-448
fn is_default_value_name(name: &str) -> bool {
    let n = name.trim();
    n == "(Default)" || n == "(默认)" || n == "(預設)" || n == "(standard)"
}

// =================== ContextMenuHandlers (对齐 startupManager.js#498-537) ===================

const CMH_LOCATIONS: &[&str] = &[
    "HKCR\\*\\shellex\\ContextMenuHandlers",
    "HKCR\\Directory\\shellex\\ContextMenuHandlers",
    "HKCR\\Directory\\Background\\shellex\\ContextMenuHandlers",
    "HKCR\\Folder\\shellex\\ContextMenuHandlers",
    "HKCR\\Drive\\shellex\\ContextMenuHandlers",
    "HKCR\\AllFileSystemObjects\\shellex\\ContextMenuHandlers",
    "HKCU\\Software\\Classes\\*\\shellex\\ContextMenuHandlers",
    "HKCU\\Software\\Classes\\Directory\\shellex\\ContextMenuHandlers",
    "HKCU\\Software\\Classes\\Directory\\Background\\shellex\\ContextMenuHandlers",
    "HKCU\\Software\\Classes\\Folder\\shellex\\ContextMenuHandlers",
];

/// 解析 CLSID 信息 (组件名 + DLL 路径) —— 对齐 resolveClsidInfo() #466-481
async fn resolve_clsid_info(clsid: &str) -> Value {
    if clsid.is_empty() {
        return json!({ "componentName": null, "dllPath": null });
    }
    let cache_key = clsid.to_lowercase();
    if let Some(cached) = CLSID_INFO_CACHE.read().unwrap().get(&cache_key) {
        return cached.clone();
    }
    let base = format!("HKCR\\CLSID\\{}", clsid);
    let vals = match list_values(&base).await {
        Ok(v) => v,
        Err(_) => return json!({ "componentName": null, "dllPath": null }),
    };
    let mut component_name = None;
    let mut dll_path = None;
    for v in vals {
        let v_name = v.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let v_value = v.get("value").and_then(|v| v.as_str()).unwrap_or("");
        if is_default_value_name(v_name) && !v_value.is_empty() {
            component_name = Some(v_value.to_string());
        }
        if v_name == "InprocServer32" || v_name == "LocalServer32" {
            dll_path = Some(v_value.to_string());
        }
    }
    let info = json!({ "componentName": component_name, "dllPath": dll_path });
    CLSID_INFO_CACHE.write().unwrap().insert(cache_key, info.clone());
    info
}

/// 列举右键菜单扩展 —— 对齐 listContextMenuHandlers() #498-537
async fn list_context_menu_handlers() -> Value {
    let mut items = Vec::new();
    let ignore = load_ignore().await;
    let ssb = match get_safe_speed_boot_data().await {
        Ok(s) => s,
        Err(_) => json!({}),
    };
    let known_set: HashSet<String> = ssb.get("contextMenuHandlers").and_then(|v| v.as_array()).map(|arr| {
        arr.iter().filter_map(|v| v.as_str().map(|s| s.to_lowercase())).collect()
    }).unwrap_or_default();
    // 收集全部 handler 任务
    let mut tasks = Vec::new();
    for loc in CMH_LOCATIONS {
        let handlers = match query_sub_keys(loc).await {
            Ok(h) => h,
            Err(_) => continue,
        };
        for h in handlers {
            tasks.push((loc.to_string(), h));
        }
    }
    // 串行处理 (JS 版用 Promise.all 并发 8, 这里简化为串行, 避免闭包捕获生命周期问题)
    for (loc, h) in tasks {
        let handler_key = format!("{}\\{}", loc, h);
        let vals = match list_values(&handler_key).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        let def = vals.iter().find(|v| is_default_value_name(v.get("name").and_then(|n| n.as_str()).unwrap_or("")));
        let clsid = def.and_then(|v| v.get("value").and_then(|v| v.as_str())).map(|s| s.to_string());
        let info = if let Some(ref c) = clsid { resolve_clsid_info(c).await } else { json!({ "componentName": null, "dllPath": null }) };
        let known = clsid.as_ref().map(|c| known_set.contains(&c.to_lowercase())).unwrap_or(false);
        let id = format!("cmh_{}_{}", sanitize_id(&loc), sanitize_id(&h));
        ITEM_META.write().unwrap().insert(id.clone(), json!({
            "kind": "cmh",
            "key": handler_key,
            "clsid": clsid,
            "name": h,
        }));
        items.push(json!({
            "id": id,
            "name": info.get("componentName").and_then(|v| v.as_str()).unwrap_or(&h),
            "desc": info.get("dllPath").and_then(|v| v.as_str()).unwrap_or(&format!("{} · {}", h, clsid.as_deref().unwrap_or(""))),
            "icon": "shell",
            "banRate": if known { 5 } else { 45 },
            "suggestion": if known { "建议开启" } else { "维持现状" },
            "enabled": true,
            "source": "contextmenu",
            "target": info.get("dllPath").and_then(|v| v.as_str()).unwrap_or(clsid.as_deref().unwrap_or(&h)),
            "startupTime": info.get("dllPath").and_then(|v| v.as_str()).and_then(|_dll| {
                None::<u64>
            }),
            "ignored": ignore.contains(&id),
            "canToggle": true,
            "known": known,
            "settings": ["打开所在目录", "删除启动项"],
        }));
    }
    // 排序: known 优先, 其次 banRate 降序
    items.sort_by(|a, b| {
        let a_known = a.get("known").and_then(|v| v.as_bool()).unwrap_or(false);
        let b_known = b.get("known").and_then(|v| v.as_bool()).unwrap_or(false);
        if a_known != b_known {
            return if a_known { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
        }
        let a_br = a.get("banRate").and_then(|v| v.as_u64()).unwrap_or(0);
        let b_br = b.get("banRate").and_then(|v| v.as_u64()).unwrap_or(0);
        b_br.cmp(&a_br)
    });
    json!(items)
}

// =================== ExplorerPlugins (对齐 startupManager.js#550-592) ===================

const EXPLORER_NS_LOCATIONS: &[&str] = &[
    "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Desktop\\NameSpace",
    "HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Desktop\\NameSpace",
    "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\HideDesktopIcons\\NewStartPanel",
    "HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\HideDesktopIcons\\NewStartPanel",
];

/// 列举资源管理器插件 —— 对齐 listExplorerPlugins() #550-592
async fn list_explorer_plugins() -> Value {
    let mut items = Vec::new();
    let ignore = load_ignore().await;
    let ssb = match get_safe_speed_boot_data().await {
        Ok(s) => s,
        Err(_) => json!({}),
    };
    let known_set: HashSet<String> = ssb.get("explorerPlugins").and_then(|v| v.as_array()).map(|arr| {
        arr.iter().filter_map(|v| v.as_str().map(|s| s.to_lowercase())).collect()
    }).unwrap_or_default();
    let mut seen = HashSet::new();
    for loc in EXPLORER_NS_LOCATIONS {
        let guids = match query_sub_keys(loc).await {
            Ok(g) => g,
            Err(_) => continue,
        };
        for g in guids {
            let lower_g = g.to_lowercase();
            if seen.contains(&lower_g) {
                continue;
            }
            seen.insert(lower_g.clone());
            let base = format!("HKCR\\CLSID\\{}", g);
            let vals = match list_values(&base).await {
                Ok(v) => v,
                Err(_) => continue,
            };
            let mut name = None;
            let mut dll_path = None;
            for v in vals {
                let v_name = v.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let v_value = v.get("value").and_then(|v| v.as_str()).unwrap_or("");
                if is_default_value_name(v_name) && !v_value.is_empty() {
                    name = Some(v_value.to_string());
                }
                if v_name == "InprocServer32" || v_name == "LocalServer32" {
                    dll_path = Some(v_value.to_string());
                }
            }
            let known = known_set.contains(&lower_g);
            let id = format!("explorerplugin_{}", sanitize_id(&g));
            ITEM_META.write().unwrap().insert(id.clone(), json!({
                "kind": "explorerplugin",
                "nsKey": format!("{}\\{}", loc, g),
                "displayName": name.clone().unwrap_or_else(|| g.clone()),
                "clsid": g,
            }));
            items.push(json!({
                "id": id,
                "name": name.unwrap_or_else(|| g.clone()),
                "desc": dll_path.clone().unwrap_or_else(|| g.clone()),
                "icon": "plugin",
                "banRate": if known { 10 } else { 50 },
                "suggestion": if known { "建议开启" } else { "维持现状" },
                "enabled": true,
                "source": "explorerplugin",
                "target": dll_path.unwrap_or_else(|| g.clone()),
                "startupTime": None::<u64>,
                "ignored": ignore.contains(&id),
                "canToggle": true,
                "known": known,
                "settings": ["打开所在目录", "删除启动项"],
            }));
        }
    }
    items.sort_by(|a, b| {
        let a_known = a.get("known").and_then(|v| v.as_bool()).unwrap_or(false);
        let b_known = b.get("known").and_then(|v| v.as_bool()).unwrap_or(false);
        if a_known != b_known {
            return if a_known { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
        }
        let a_br = a.get("banRate").and_then(|v| v.as_u64()).unwrap_or(0);
        let b_br = b.get("banRate").and_then(|v| v.as_u64()).unwrap_or(0);
        b_br.cmp(&a_br)
    });
    json!(items)
}

// =================== toggle (对齐 startupManager.js#596-648) ===================

/// 切换启用/禁用状态 —— 对齐 toggle() #596-648
pub async fn toggle(item_id: String, enabled: bool) -> Value {
    let meta = {
        let guard = ITEM_META.read().unwrap();
        guard.get(&item_id).cloned()
    };
    let meta = match meta {
        Some(m) => m,
        None => return json!({ "ok": false, "message": format!("unknown item: {}", item_id) }),
    };
    let essential = meta.get("essential").and_then(|v| v.as_bool()).unwrap_or(false);
    if essential && !enabled {
        return json!({ "ok": false, "message": "系统关键组件，禁止禁用" });
    }
    let kind = meta.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "registry" => {
            let approved_key = meta.get("approvedKey").and_then(|v| v.as_str());
            if approved_key.is_none() {
                return json!({ "ok": false, "message": "runonce items cannot be toggled" });
            }
            let approved_key = approved_key.unwrap();
            let _ = ensure_key(approved_key).await;
            let value_name = meta.get("valueName").and_then(|v| v.as_str()).unwrap_or("");
            let hex = if enabled { ENABLED_BINARY_HEX } else { DISABLED_BINARY_HEX };
            set_binary(approved_key, value_name, hex).await.unwrap_or_else(|e| json!({ "ok": false, "message": e }))
        }
        "service" => {
            let service_name = meta.get("serviceName").and_then(|v| v.as_str()).unwrap_or("");
            let start_type_code = meta.get("startTypeCode").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            // 启用还原原始启动类型: 3=DEMAND_START->Manual, 其余回退 Auto
            let restore_type = if start_type_code == 3 { "Manual" } else { "Auto" };
            set_service_start_type(service_name, if enabled { restore_type } else { "Disabled" }).await.unwrap_or_else(|e| json!({ "ok": false, "message": e }))
        }
        "task" => {
            let task_name = meta.get("taskName").and_then(|v| v.as_str()).unwrap_or("");
            let res = run_async("schtasks", &["/Change", "/TN", task_name, if enabled { "/ENABLE" } else { "/DISABLE" }], None).await;
            match res {
                Ok(output) => json!({ "ok": true, "code": 0, "message": output }),
                Err(e) => json!({ "ok": false, "code": 1, "message": e }),
            }
        }
        "startup_folder" => {
            let lnk_path = meta.get("lnkPath").and_then(|v| v.as_str()).unwrap_or("");
            let path = PathBuf::from(lnk_path);
            let disabled_path = path.with_extension("lnk.disabled");
            let result = if enabled {
                // 启用: 将 .disabled 重命名回 .lnk
                if disabled_path.exists() {
                    fs::rename(&disabled_path, &path).map(|_| json!({ "ok": true })).unwrap_or_else(|e| json!({ "ok": false, "message": e.to_string() }))
                } else {
                    json!({ "ok": true })
                }
            } else {
                // 禁用: 将 .lnk 重命名为 .disabled
                if path.exists() {
                    fs::rename(&path, &disabled_path).map(|_| json!({ "ok": true })).unwrap_or_else(|e| json!({ "ok": false, "message": e.to_string() }))
                } else {
                    json!({ "ok": false, "message": "shortcut not found" })
                }
            };
            result
        }
        "cmh" => {
            let key = meta.get("key").and_then(|v| v.as_str()).unwrap_or("");
            let clsid = meta.get("clsid").and_then(|v| v.as_str()).unwrap_or("");
            if enabled {
                let _ = ensure_key(key).await;
                set_string(key, "", clsid).await.unwrap_or_else(|e| json!({ "ok": false, "message": e }))
            } else {
                let res = run_async("reg", &["delete", key, "/f"], None).await;
                match res {
                    Ok(output) => json!({ "ok": true, "code": 0, "message": output }),
                    Err(e) => json!({ "ok": false, "code": 1, "message": e }),
                }
            }
        }
        "explorerplugin" => {
            let ns_key = meta.get("nsKey").and_then(|v| v.as_str()).unwrap_or("");
            let display_name = meta.get("displayName").and_then(|v| v.as_str()).unwrap_or("");
            if enabled {
                let _ = ensure_key(ns_key).await;
                set_string(ns_key, "", display_name).await.unwrap_or_else(|e| json!({ "ok": false, "message": e }))
            } else {
                let res = run_async("reg", &["delete", ns_key, "/f"], None).await;
                match res {
                    Ok(output) => json!({ "ok": true, "code": 0, "message": output }),
                    Err(e) => json!({ "ok": false, "code": 1, "message": e }),
                }
            }
        }
        _ => json!({ "ok": false, "message": "unsupported item type" }),
    }
}

// =================== remove (对齐 startupManager.js#652-687) ===================

/// 删除启动项 —— 对齐 remove() #652-687
pub async fn remove(item_id: String) -> Value {
    let meta = {
        let guard = ITEM_META.read().unwrap();
        guard.get(&item_id).cloned()
    };
    let meta = match meta {
        Some(m) => m,
        None => return json!({ "ok": false, "message": format!("unknown item: {}", item_id) }),
    };
    let essential = meta.get("essential").and_then(|v| v.as_bool()).unwrap_or(false);
    if essential {
        return json!({ "ok": false, "message": "系统关键组件，禁止删除" });
    }
    let kind = meta.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "registry" => {
            let run_key = meta.get("runKey").and_then(|v| v.as_str());
            if run_key.is_none() {
                return json!({ "ok": false, "message": "runonce items cannot be removed" });
            }
            let value_name = meta.get("valueName").and_then(|v| v.as_str()).unwrap_or("");
            match delete_value(run_key.unwrap(), value_name).await {
                Ok(v) => v,
                Err(e) => {
                    if !crate::system::elevate::is_access_denied(&e) {
                        return json!({ "ok": false, "message": e });
                    }
                    // HKLM 等管理键: UAC 提权重试 + 回查验证
                    let params =
                        format!("delete \"{}\" /v \"{}\" /f", run_key.unwrap(), value_name);
                    match crate::system::elevate::run_elevated("reg", &params).await {
                        Ok(_) => {
                            let gone = run_async(
                                "reg",
                                &["query", run_key.unwrap(), "/v", value_name],
                                None,
                            )
                            .await
                            .is_err();
                            if gone {
                                json!({ "ok": true, "message": "已通过管理员权限删除" })
                            } else {
                                json!({ "ok": false, "message": "提权删除未生效，值仍存在" })
                            }
                        }
                        Err(e2) => json!({ "ok": false, "message": e2 }),
                    }
                }
            }
        }
        "task" => {
            let task_name = meta.get("taskName").and_then(|v| v.as_str()).unwrap_or("");
            match run_async("schtasks", &["/Delete", "/TN", task_name, "/F"], None).await {
                Ok(output) => json!({ "ok": true, "code": 0, "message": output }),
                Err(e) => {
                    if !crate::system::elevate::is_access_denied(&e) {
                        return json!({ "ok": false, "code": 1, "message": e });
                    }
                    // SYSTEM/他人任务: UAC 提权重试 + 回查验证
                    let params = format!("/Delete /TN \"{}\" /F", task_name);
                    match crate::system::elevate::run_elevated("schtasks", &params).await {
                        Ok(_) => {
                            let gone = run_async(
                                "schtasks",
                                &["/Query", "/TN", task_name],
                                None,
                            )
                            .await
                            .is_err();
                            if gone {
                                json!({ "ok": true, "message": "已通过管理员权限删除" })
                            } else {
                                json!({ "ok": false, "message": "提权删除未生效，任务仍存在" })
                            }
                        }
                        Err(e2) => json!({ "ok": false, "code": 1, "message": e2 }),
                    }
                }
            }
        }
        "startup_folder" => {
            let lnk_path = meta.get("lnkPath").and_then(|v| v.as_str()).unwrap_or("");
            let path = PathBuf::from(lnk_path);
            let disabled_path = path.with_extension("lnk.disabled");
            for p in [path, disabled_path] {
                if p.exists() {
                    let _ = fs::remove_file(&p);
                }
            }
            json!({ "ok": true })
        }
        "service" => {
            json!({ "ok": false, "message": "系统服务不支持直接删除，请使用\"禁止启动\"停用" })
        }
        "cmh" => {
            let key = meta.get("key").and_then(|v| v.as_str()).unwrap_or("");
            match run_async("reg", &["delete", key, "/f"], None).await {
                Ok(output) => json!({ "ok": true, "code": 0, "message": output }),
                Err(e) => {
                    if !crate::system::elevate::is_access_denied(&e) {
                        return json!({ "ok": false, "code": 1, "message": e });
                    }
                    // HKCR 机装键: UAC 提权重试 + 回查验证
                    match crate::system::elevate::run_elevated("reg", &format!("delete \"{}\" /f", key)).await {
                        Ok(_) => {
                            let gone =
                                run_async("reg", &["query", key], None).await.is_err();
                            if gone {
                                json!({ "ok": true, "message": "已通过管理员权限删除" })
                            } else {
                                json!({ "ok": false, "message": "提权删除未生效，键仍存在" })
                            }
                        }
                        Err(e2) => json!({ "ok": false, "code": 1, "message": e2 }),
                    }
                }
            }
        }
        "explorerplugin" => {
            let ns_key = meta.get("nsKey").and_then(|v| v.as_str()).unwrap_or("");
            match run_async("reg", &["delete", ns_key, "/f"], None).await {
                Ok(output) => json!({ "ok": true, "code": 0, "message": output }),
                Err(e) => {
                    if !crate::system::elevate::is_access_denied(&e) {
                        return json!({ "ok": false, "code": 1, "message": e });
                    }
                    match crate::system::elevate::run_elevated("reg", &format!("delete \"{}\" /f", ns_key)).await {
                        Ok(_) => {
                            let gone =
                                run_async("reg", &["query", ns_key], None).await.is_err();
                            if gone {
                                json!({ "ok": true, "message": "已通过管理员权限删除" })
                            } else {
                                json!({ "ok": false, "message": "提权删除未生效，键仍存在" })
                            }
                        }
                        Err(e2) => json!({ "ok": false, "code": 1, "message": e2 }),
                    }
                }
            }
        }
        _ => json!({ "ok": false, "message": "unsupported item type" }),
    }
}

// =================== openLocation (对齐 startupManager.js#689-728) ===================

/// 打开启动项所在位置 —— 对齐 openLocation() #689-728
pub async fn open_location(item_id: String) -> Value {
    let meta = {
        let guard = ITEM_META.read().unwrap();
        guard.get(&item_id).cloned()
    };
    let meta = match meta {
        Some(m) => m,
        None => return json!({ "ok": false, "message": format!("unknown item: {}", item_id) }),
    };
    let kind = meta.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let mut target_dir: Option<PathBuf> = None;
    let mut target_file: Option<PathBuf> = None;
    match kind {
        "startup_folder" => {
            let lnk_path = meta.get("lnkPath").and_then(|v| v.as_str()).unwrap_or("");
            target_file = Some(PathBuf::from(lnk_path));
            target_dir = target_file.as_ref().and_then(|p| p.parent().map(|p| p.to_path_buf()));
        }
        "registry" => {
            let cmd = meta.get("command").and_then(|v| v.as_str()).unwrap_or("");
            if let Some(exe) = extract_exe_from_command(cmd) {
                target_file = Some(PathBuf::from(exe));
                target_dir = target_file.as_ref().and_then(|p| p.parent().map(|p| p.to_path_buf()));
            }
        }
        "cmh" | "explorerplugin" => {
            let clsid = meta.get("clsid").and_then(|v| v.as_str()).unwrap_or("");
            let info = resolve_clsid_info(clsid).await;
            if let Some(dll) = info.get("dllPath").and_then(|v| v.as_str()) {
                let exe = dll.split_whitespace().next().unwrap_or("").trim_matches('"');
                let exe_path = PathBuf::from(exe);
                if exe_path.exists() {
                    target_file = Some(exe_path.clone());
                    target_dir = exe_path.parent().map(|p| p.to_path_buf());
                }
            }
        }
        _ => {}
    }
    let target_dir = match target_dir {
        Some(d) if d.exists() => d,
        _ => return json!({ "ok": false, "message": "无法定位该启动项的位置" }),
    };
    let result = if let Some(file) = target_file {
        if file.exists() {
            run_async("explorer", &["/select,", &file.to_string_lossy()], None).await
        } else {
            run_async("explorer", &[&target_dir.to_string_lossy()], None).await
        }
    } else {
        run_async("explorer", &[&target_dir.to_string_lossy()], None).await
    };
    match result {
        Ok(_) => json!({ "ok": true }),
        Err(e) => json!({ "ok": false, "message": e }),
    }
}

// =================== getDetail (对齐 startupManager.js#730-748) ===================

/// 获取启动项详情 —— 对齐 getDetail() #730-748
pub async fn get_detail(item_id: String) -> Value {
    let meta = {
        let guard = ITEM_META.read().unwrap();
        guard.get(&item_id).cloned()
    };
    let meta = match meta {
        Some(m) => m,
        None => return json!({ "ok": false, "message": format!("unknown item: {}", item_id) }),
    };
    let kind = meta.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let essential = meta.get("essential").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut detail = json!({ "kind": kind, "essential": essential });
    match kind {
        "registry" => {
            detail["runKey"] = json!(meta.get("runKey").and_then(|v| v.as_str()).unwrap_or(""));
            detail["valueName"] = json!(meta.get("valueName").and_then(|v| v.as_str()).unwrap_or(""));
            detail["command"] = json!(meta.get("command").and_then(|v| v.as_str()).unwrap_or(""));
        }
        "task" => {
            detail["taskName"] = json!(meta.get("taskName").and_then(|v| v.as_str()).unwrap_or(""));
        }
        "startup_folder" => {
            detail["lnkPath"] = json!(meta.get("lnkPath").and_then(|v| v.as_str()).unwrap_or(""));
        }
        "service" => {
            detail["serviceName"] = json!(meta.get("serviceName").and_then(|v| v.as_str()).unwrap_or(""));
        }
        "cmh" | "explorerplugin" => {
            detail["clsid"] = json!(meta.get("clsid").and_then(|v| v.as_str()).unwrap_or(""));
        }
        _ => {}
    }
    json!({ "ok": true, "detail": detail })
}

// =================== smartOptimize (对齐 startupManager.js#750-770) ===================

/// 一键优化 —— 对齐 smartOptimize() #750-770
pub async fn smart_optimize() -> Value {
    // 优化前自动备份
    let _ = backup().await;
    let index = match get_rule_index().await {
        Ok(idx) => idx,
        Err(e) => return json!({ "ok": false, "message": e }),
    };
    let mut all = Vec::new();
    // 收集所有启动项
    if let Value::Array(arr) = list_software(index.clone()).await {
        all.extend(arr);
    }
    if let Value::Array(arr) = list_system(index.clone()).await {
        all.extend(arr);
    }
    if let Value::Array(arr) = list_context_menu_handlers().await {
        all.extend(arr);
    }
    if let Value::Array(arr) = list_explorer_plugins().await {
        all.extend(arr);
    }
    let mut count = 0;
    for item in all {
        let item_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let meta = ITEM_META.read().unwrap().get(item_id).cloned();
        let essential = meta.as_ref().and_then(|m| m.get("essential").and_then(|v| v.as_bool())).unwrap_or(false);
        let ban_rate = item.get("banRate").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let ignored = item.get("ignored").and_then(|v| v.as_bool()).unwrap_or(false);
        let enabled = item.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
        let can_toggle = item.get("canToggle").and_then(|v| v.as_bool()).unwrap_or(false);
        if ban_rate >= 60 && !essential && !ignored && enabled && can_toggle {
            let r = toggle(item_id.to_string(), false).await;
            if r.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                count += 1;
            }
        }
    }
    json!({ "ok": true, "count": count })
}

// =================== 忽略/信任白名单 (对齐 startupManager.js#772-802) ===================

/// 保存忽略列表 —— 对齐 saveIgnore() #788-794
async fn save_ignore(set: &HashSet<String>) {
    *IGNORE_CACHE.write().unwrap() = Some(set.clone());
    let path = ignore_file_path();
    let _ = fs::create_dir_all(path.parent().unwrap());
    let _ = fs::write(&path, serde_json::to_string(&set.iter().collect::<Vec<_>>()).unwrap_or_default());
}

/// 设置忽略状态 —— 对齐 setIgnored() #796-802
pub async fn set_ignored(item_id: String, ignored: bool) -> Value {
    let mut set = load_ignore().await;
    if ignored {
        set.insert(item_id);
    } else {
        set.remove(&item_id);
    }
    save_ignore(&set).await;
    json!({ "ok": true, "ignored": ignored })
}

/// 导出 loadIgnore (供外部调用)
pub async fn load_ignore_export() -> Value {
    let set = load_ignore().await;
    json!(set.iter().collect::<Vec<_>>())
}

/// 导出 saveIgnore (供外部调用)
pub async fn save_ignore_export(items: Vec<String>) -> Value {
    let set: HashSet<String> = items.into_iter().collect();
    save_ignore(&set).await;
    json!({ "ok": true })
}

// =================== 启动耗时补全 (对齐 startupManager.js#804-824) ===================

/// 获取 exe 基名 -> 耗时映射 —— 对齐 getBootExeMap() #807-817
async fn get_boot_exe_map() -> HashMap<String, u64> {
    if let Some(cached) = BOOT_EXE_MAP_CACHE.read().unwrap().as_ref() {
        return cached.clone();
    }
    let map = match get_boot_time_ref_map().await {
        Ok(m) => m,
        Err(_) => return HashMap::new(),
    };
    let mut m = HashMap::new();
    if let Some(obj) = map.as_object() {
        for (k, v) in obj {
            let base = k.to_lowercase();
            if !m.contains_key(&base) {
                if let Some(time) = v.as_u64() {
                    m.insert(base, time);
                }
            }
        }
    }
    *BOOT_EXE_MAP_CACHE.write().unwrap() = Some(m.clone());
    m
}

/// 通过 exe 路径查找启动耗时 —— 对齐 lookupBootTimeByExe() #819-824
pub async fn lookup_boot_time_by_exe(exe: String) -> Value {
    if exe.is_empty() {
        return json!(null);
    }
    let m = get_boot_exe_map().await;
    let base = Path::new(&exe).file_name().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    if let Some(time) = m.get(&base) {
        json!(time)
    } else {
        json!(null)
    }
}

// =================== 添加启动项 (对齐 startupManager.js#827-851) ===================

/// 添加启动项 —— 对齐 add() #827-851
pub async fn add(item: Value) -> Value {
    if !item.is_object() {
        return json!({ "ok": false, "message": "参数无效" });
    }
    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("").trim();
    let command = item.get("command").and_then(|v| v.as_str()).unwrap_or("").trim();
    if name.is_empty() || command.is_empty() {
        return json!({ "ok": false, "message": "名称和命令不能为空" });
    }
    let location = item.get("location").and_then(|v| v.as_str()).unwrap_or("");
    if location == "startup_folder" {
        let folder = app_data().join("Microsoft").join("Windows").join("Start Menu").join("Programs").join("Startup");
        let _ = fs::create_dir_all(&folder);
        let lnk = folder.join(format!("{}.lnk", name));
        let safe_lnk = lnk.to_string_lossy().replace('\'', "''");
        let safe_cmd = command.replace('\'', "''");
        let script = format!(
            "$ws = New-Object -ComObject WScript.Shell; $s = $ws.CreateShortcut('{}'); $s.TargetPath = '{}'; $s.Save()",
            safe_lnk, safe_cmd
        );
        let res = run_async("powershell", &["-NoProfile", "-NonInteractive", "-Command", &script], None).await;
        match res {
            Ok(_) => json!({ "ok": true }),
            Err(e) => json!({ "ok": false, "message": e }),
        }
    } else {
        // 默认写入 HKCU Run
        let key = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
        let _ = ensure_key(key).await;
        set_string(key, name, command).await.unwrap_or_else(|e| json!({ "ok": false, "message": e }))
    }
}

// =================== 备份/恢复 (对齐 startupManager.js#853-916) ===================

/// 备份目录
fn backup_dir() -> PathBuf {
    app_data().join("com.opencode.systemcleaner").join("startup_backups")
}

/// 生成快照 —— 对齐 _snapshot() #856-866
async fn snapshot() -> Vec<Value> {
    let index = match get_rule_index().await {
        Ok(idx) => idx,
        Err(_) => return Vec::new(),
    };
    let mut all = Vec::new();
    if let Value::Array(arr) = list_software(index.clone()).await {
        all.extend(arr);
    }
    if let Value::Array(arr) = list_system(index.clone()).await {
        all.extend(arr);
    }
    if let Value::Array(arr) = list_scheduled(index.clone()).await {
        all.extend(arr);
    }
    if let Value::Array(arr) = list_context_menu_handlers().await {
        all.extend(arr);
    }
    if let Value::Array(arr) = list_explorer_plugins().await {
        all.extend(arr);
    }
    all.into_iter().map(|it| json!({
        "id": it.get("id").and_then(|v| v.as_str()).unwrap_or(""),
        "enabled": it.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false),
        "source": it.get("source").and_then(|v| v.as_str()).unwrap_or(""),
        "name": it.get("name").and_then(|v| v.as_str()).unwrap_or(""),
    })).collect()
}

/// 备份当前启动项状态 —— 对齐 backup() #868-875
pub async fn backup() -> Value {
    let snap = snapshot().await;
    let dir = backup_dir();
    let _ = fs::create_dir_all(&dir);
    let now = SystemTime::now();
    let ts = now.duration_since(UNIX_EPOCH).unwrap().as_millis().to_string();
    let file = dir.join(format!("startup_{}.json", ts));
    let content = json!({
        "createdAt": now.duration_since(UNIX_EPOCH).unwrap().as_millis() as u64,
        "items": snap,
    });
    let _ = fs::write(&file, serde_json::to_string_pretty(&content).unwrap_or_default());
    json!({ "ok": true, "file": file.file_name().and_then(|s| s.to_str()).unwrap_or(""), "count": content["items"].as_array().map(|a| a.len()).unwrap_or(0) })
}

/// 列举备份文件 —— 对齐 listBackups() #877-893
pub async fn list_backups() -> Value {
    let dir = backup_dir();
    let _ = fs::create_dir_all(&dir);
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        let mut files: Vec<_> = entries.flatten().filter(|e| {
            e.file_name().to_string_lossy().ends_with(".json")
        }).collect();
        files.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
        for entry in files {
            let path = entry.path();
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(j) = serde_json::from_str::<Value>(&content) {
                    out.push(json!({
                        "file": entry.file_name().to_string_lossy().to_string(),
                        "createdAt": j.get("createdAt").and_then(|v| v.as_u64()).unwrap_or(0),
                        "count": j.get("items").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
                    }));
                }
            }
        }
    }
    json!(out)
}

/// 恢复备份 —— 对齐 restore() #895-916
pub async fn restore(file_name: String) -> Value {
    let file = backup_dir().join(Path::new(&file_name).file_name().unwrap());
    let content = match fs::read_to_string(&file) {
        Ok(c) => c,
        Err(_) => return json!({ "ok": false, "message": "备份文件读取失败" }),
    };
    let j: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return json!({ "ok": false, "message": "备份文件格式错误" }),
    };
    // 重新填充 itemMeta (restore 可能在未先 list 的情况下被调用)
    let index = match get_rule_index().await {
        Ok(idx) => idx,
        Err(_) => json!({}),
    };
    let _ = list_software(index.clone()).await;
    let _ = list_system(index.clone()).await;
    let _ = list_scheduled(index.clone()).await;
    let _ = list_context_menu_handlers().await;
    let _ = list_explorer_plugins().await;
    let mut applied = 0;
    let mut skipped = 0;
    if let Some(items) = j.get("items").and_then(|v| v.as_array()) {
        for it in items {
            let id = it.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let enabled = it.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
            if ITEM_META.read().unwrap().contains_key(id) {
                let r = toggle(id.to_string(), enabled).await;
                if r.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                    applied += 1;
                } else {
                    skipped += 1;
                }
            } else {
                skipped += 1;
            }
        }
    }
    json!({ "ok": true, "applied": applied, "skipped": skipped })
}

// =================== 导出的分类辅助函数 (对齐 startupManager.js#934-945) ===================

/// 导出 sanitize_id
pub fn sanitize_id_export(s: String) -> String {
    sanitize_id(&s)
}

/// 导出 extract_exe_from_command
pub fn extract_exe_from_command_export(cmd: String) -> Value {
    extract_exe_from_command(&cmd).map_or(json!(null), |s| json!(s))
}

/// 导出 extract_rule_target
pub fn extract_rule_target_export(rule: Value) -> String {
    extract_rule_target(&rule)
}

/// 导出 build_rule_index
pub fn build_rule_index_export(rules: Value) -> Value {
    build_rule_index(&rules)
}

/// 导出 match_rule
pub fn match_rule_export(index: Value, name: Option<String>, target_exe: Option<String>) -> Value {
    match_rule(&index, name.as_deref(), target_exe.as_deref()).unwrap_or(json!(null))
}

/// 导出 compute_ban_rate
pub fn compute_ban_rate_export(rule: Option<Value>, enabled: bool) -> u32 {
    compute_ban_rate(rule.as_ref(), enabled)
}

/// 导出 compute_suggestion
pub fn compute_suggestion_export(ban_rate: u32, essential: bool) -> &'static str {
    compute_suggestion(ban_rate, essential)
}

/// 导出 is_system_essential
pub fn is_system_essential_export(rule: Option<Value>, source: String, name: String) -> bool {
    is_system_essential(rule.as_ref(), &source, &name)
}

/// 导出 list_scheduled_tasks
pub async fn list_scheduled_tasks_export() -> Value {
    json!(list_scheduled_tasks().await.unwrap_or_default())
}

/// 导出 parse_csv
pub fn parse_csv_export(text: String) -> Value {
    json!(parse_csv(&text))
}

// 移除标记