//! 规则引擎 —— 移植 rules/engine.js
//! 加载 ui/rules/*.json + isSafePath/isCriticalRoot/expandTemplate
//! 运行时通过 init(rules_dir) 注入规则目录(开发/打包均可工作)

use crate::system::paths::resolve_path;
use once_cell::sync::{Lazy, OnceCell};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// 规则文件列表 —— 对齐 engine.js#6-15 RULE_FILES
const RULE_FILES: &[&str] = &[
    "startupRules.json",
    "bootTimeRef.json",
    "bootSpeed.json",
    "safeSpeedBoot.json",
    "cleanTree.json",
    "softDetect.json",
    "desktopGarbage.json",
    "scanModules.json",
];

/// 规则目录(运行时由 main.rs setup 调用 init 设置)
static RULES_DIR: OnceCell<PathBuf> = OnceCell::new();

/// 规则缓存 —— 对齐 engine.js#16-17 cache/loadPromise
static RULES_CACHE: Lazy<RwLock<Option<HashMap<String, Value>>>> = Lazy::new(|| RwLock::new(None));

/// 初始化规则目录 —— 由 main.rs setup 调用
/// dev: exe_dir/../ui/rules, bundle: tauri resource 解析后的 rules 目录
pub fn init(rules_dir: PathBuf) {
    let _ = RULES_DIR.set(rules_dir);
}

/// 获取规则目录(未初始化时回退到 exe 相对路径)
fn get_rules_dir() -> PathBuf {
    RULES_DIR.get().cloned().unwrap_or_else(|| {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .map(|p| p.join("..").join("ui").join("rules"))
            .unwrap_or_else(|| PathBuf::from("rules"))
    })
}

/// 加载所有规则文件 —— 对齐 engine.js#19-32 loadRules()
pub async fn load_rules() -> Result<HashMap<String, Value>, String> {
    // 读缓存
    if let Some(cached) = RULES_CACHE.read().unwrap().as_ref() {
        return Ok(cached.clone());
    }

    let rules_dir = get_rules_dir();
    let mut data = HashMap::new();

    for f in RULE_FILES {
        let key = f.trim_end_matches(".json");
        let path = rules_dir.join(f);
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("read {} failed: {}", path.display(), e))?;
        let parsed: Value = serde_json::from_str(&content)
            .map_err(|e| format!("parse {} failed: {}", path.display(), e))?;
        data.insert(key.to_string(), parsed);
    }

    *RULES_CACHE.write().unwrap() = Some(data.clone());
    Ok(data)
}

/// 获取启动项规则 —— 对齐 engine.js#34-37 getStartupRules()
pub async fn get_startup_rules() -> Result<Value, String> {
    let rules = load_rules().await?;
    Ok(rules.get("startupRules")
        .and_then(|v| v.get("items"))
        .cloned()
        .unwrap_or(json!([])))
}

/// 获取清理树规则 —— 对齐 engine.js#39-42 getCleanTree()
pub async fn get_clean_tree() -> Result<Value, String> {
    let rules = load_rules().await?;
    Ok(rules.get("cleanTree")
        .and_then(|v| v.get("groups"))
        .cloned()
        .unwrap_or(json!([])))
}

/// 获取软件检测规则 —— 对齐 engine.js#44-47 getSoftDetect()
pub async fn get_soft_detect() -> Result<Value, String> {
    let rules = load_rules().await?;
    Ok(rules.get("softDetect")
        .and_then(|v| v.get("items"))
        .cloned()
        .unwrap_or(json!([])))
}

/// 获取启动时间基准 —— 对齐 engine.js#49-54 getBootTimeRef()
/// 返回 { service: baseTimeMs }
pub async fn get_boot_time_ref() -> Result<Value, String> {
    let rules = load_rules().await?;
    let entries = rules.get("bootTimeRef")
        .and_then(|v| v.get("entries"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut map = serde_json::Map::new();
    for e in entries {
        if let (Some(svc), Some(time)) = (e.get("service").and_then(|v| v.as_str()), e.get("baseTimeMs").and_then(|v| v.as_u64())) {
            map.insert(svc.to_string(), json!(time));
        }
    }
    Ok(json!(map))
}

/// 获取启动速度分箱 —— 对齐 engine.js#56-59 getBootSpeed()
pub async fn get_boot_speed() -> Result<Value, String> {
    let rules = load_rules().await?;
    Ok(rules.get("bootSpeed")
        .and_then(|v| v.get("bins"))
        .cloned()
        .unwrap_or(json!([])))
}

/// 获取安全加速启动配置 —— 对齐 engine.js#61-64 getSafeSpeedBoot()
pub async fn get_safe_speed_boot() -> Result<Value, String> {
    let rules = load_rules().await?;
    Ok(rules.get("safeSpeedBoot").cloned().unwrap_or(json!({})))
}

/// 关键系统根目录 —— 对齐 engine.js#66-72 CRITICAL_ROOTS
const CRITICAL_ROOTS: &[&str] = &[
    "C:\\Windows",
    "C:\\Program Files",
    "C:\\Program Files (x86)",
    "C:\\ProgramData",
    "C:\\Users",
];

/// 路径规范化: canonicalize(存在时)并剥离 Windows 扩展前缀 \\?\
/// 解决 "存在的路径带 \\?\ 前缀、不存在的保持原样" 的不对称导致的比较失败
fn norm_path(p: &PathBuf) -> String {
    let canon = p.canonicalize().unwrap_or_else(|_| p.clone());
    let mut s = canon.to_string_lossy().to_string();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        s = format!(r"\\{}", rest);
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        s = rest.to_string();
    }
    s.trim_end_matches('\\').to_string()
}
/// 判断是否为关键根目录 —— 对齐 engine.js#74-83 isCriticalRoot()
/// 手工实现盘符根目录检测: ^[a-z]:\\$
fn is_drive_root(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() == 3 && bytes[1] == b':' && bytes[2] == b'\\' && bytes[0].is_ascii_alphabetic()
}

pub fn is_critical_root(path: &str) -> bool {
    let lower = path.to_lowercase();
    // 盘符根目录如 C:\
    if is_drive_root(&lower) {
        return true;
    }
    for c in CRITICAL_ROOTS {
        if lower == c.to_lowercase() {
            return true;
        }
    }
    // 用户配置文件目录
    if let Ok(up) = std::env::var("USERPROFILE") {
        if lower == up.to_lowercase() {
            return true;
        }
    }
    false
}

/// 判断 child 是否在 parent 内部 —— 对齐 engine.js#85-88 isInside()
fn is_inside(parent: &str, child: &str) -> bool {
    // Windows 路径大小写不敏感; 统一小写后再比较
    let parent = parent.to_lowercase();
    let child = child.to_lowercase();
    match Path::new(&child).strip_prefix(Path::new(&parent)) {
        Ok(rel) => !rel.as_os_str().is_empty() && !rel.components().any(|c| c.as_os_str() == ".."),
        Err(_) => false,
    }
}

/// 用户 shell 受保护目录 —— 用户的个人数据, 绝不允许被当作清理目标根目录
/// (下载/桌面/文档/图片/音乐/视频/OneDrive, 含中文系统别名)
pub fn protected_user_dirs() -> Vec<PathBuf> {
    let up = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default());
    vec![
        up.join("Downloads"),
        up.join("桌面"),
        up.join("Desktop"),
        up.join("Documents"),
        up.join("文档"),
        up.join("Pictures"),
        up.join("图片"),
        up.join("Music"),
        up.join("音乐"),
        up.join("Videos"),
        up.join("视频"),
        up.join("OneDrive"),
    ]
}

/// 目标是否为受保护的用户目录根(精确匹配, 不影响其内部文件的正常清理流程)
pub fn is_protected_user_dir(abs_path: &str) -> bool {
    if abs_path.is_empty() {
        return false;
    }
    let ts = norm_path(&PathBuf::from(abs_path));
    protected_user_dirs()
        .iter()
        .any(|p| norm_path(p).eq_ignore_ascii_case(&ts))
}

/// 安全路径检查 —— 对齐 engine.js#97-113 isSafePath()
/// - target 必须在 allowedRoot 内(或等于 declared root)
/// - target 不能是关键根目录
/// - 若 target 在关键根内, allowedRoot 必须严格在同一关键根内(防跨根逃逸)
pub fn is_safe_path(abs_path: &str, allowed_root: &str) -> bool {
    if abs_path.is_empty() || allowed_root.is_empty() {
        return false;
    }
    let target_str = norm_path(&PathBuf::from(abs_path));
    let root_str = norm_path(&PathBuf::from(allowed_root));

    // target 必须在 root 内(或相等) —— 使用 is_inside 替代 pathdiff
    if target_str != root_str && !is_inside(&root_str, &target_str) {
        return false;
    }

    // target 不能是关键根
    if is_critical_root(&target_str) {
        return false;
    }

    // target 不能是受保护的用户目录根 (下载/桌面/文档等)
    if is_protected_user_dir(&target_str) {
        return false;
    }

    // 关键根列表 + 用户配置文件
    let mut crits: Vec<String> = CRITICAL_ROOTS.iter().map(|s| s.to_string()).collect();
    if let Ok(up) = std::env::var("USERPROFILE") {
        crits.push(up);
    }

    for c in crits {
        let c_lower = c.to_lowercase();
        let target_lower = target_str.to_lowercase();
        if is_inside(&c, &target_str) || target_lower == c_lower {
            // allowedRoot 必须严格在关键根内部(不能等于关键根)
            if !is_inside(&c, &root_str) {
                return false;
            }
        }
    }
    true
}

/// 展开模板路径 —— 对齐 engine.js#115-117 expandTemplate()
/// 委托给 paths::resolvePath
pub async fn expand_template(tpl: &str) -> Result<Option<PathBuf>, String> {
    resolve_path(tpl).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_critical_root() {
        assert!(is_critical_root("C:\\Windows"));
        assert!(is_critical_root("C:\\Program Files"));
        assert!(is_critical_root("C:\\"));
        assert!(!is_critical_root("C:\\Temp"));
    }

    #[test]
    fn test_is_inside() {
        assert!(is_inside("C:\\Windows", "C:\\Windows\\System32"));
        assert!(is_inside("C:\\Users\\Test", "C:\\Users\\Test\\AppData"));
        assert!(!is_inside("C:\\Windows", "C:\\Program Files"));
        assert!(!is_inside("C:\\Windows", "C:\\Windows"));
    }

    #[test]
    fn test_is_safe_path() {
        // 正常情况
        assert!(is_safe_path("C:\\Temp\\test.txt", "C:\\Temp"));
        assert!(is_safe_path("C:\\Temp\\sub\\file.txt", "C:\\Temp"));
        // 越界
        assert!(!is_safe_path("C:\\Windows\\system32\\cmd.exe", "C:\\Temp"));
        // 关键根目录
        assert!(!is_safe_path("C:\\Windows", "C:\\Windows"));
        assert!(!is_safe_path("C:\\Program Files", "C:\\Program Files"));
    }

    #[test]
    fn test_protected_user_dirs() {
        if let Ok(up) = std::env::var("USERPROFILE") {
            let up = PathBuf::from(up);
            // 不要求目录真实存在
            assert!(is_protected_user_dir(&up.join("Downloads").to_string_lossy()));
            assert!(is_protected_user_dir(&up.join("Desktop").to_string_lossy()));
            assert!(is_protected_user_dir(&up.join("Documents").to_string_lossy()));
            assert!(!is_protected_user_dir(&up.join("NotARealShellDir").to_string_lossy()));
            // 受保护目录自身不允许作为清理目标
            assert!(!is_safe_path(
                &up.join("Downloads").to_string_lossy(),
                &up.join("Downloads").to_string_lossy()
            ));
            // 其内部的普通文件不受影响 (允许被正常清理流程处理)
            let inner = up.join("Downloads").join("some_file.txt");
            assert!(is_protected_user_dir(&inner.to_string_lossy()) == false);
        }
        assert!(!is_protected_user_dir(""));
    }
}
