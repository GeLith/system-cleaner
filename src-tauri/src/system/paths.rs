//! 路径与环境变量解析 —— 对齐 Electron 版 system/paths.js
//! 环境变量展开(%VAR%), 已知文件夹, Downloads 兜底, 回收站, 浏览器缓存, 模板解析

use crate::system::exec::run_async;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::Mutex;

/// 读取环境变量 —— 对齐 paths.js#6-8 env()
pub fn env(name: &str) -> String {
    env::var(name).unwrap_or_default()
}

/// 获取环境变量或默认值
fn env_or(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_string())
}

/// %TEMP% 目录 —— 对齐 paths.js#10-12 tempDir()
pub fn temp_dir() -> PathBuf {
    let temp = env("TEMP");
    if !temp.is_empty() {
        return PathBuf::from(temp);
    }
    // 兜底: %USERPROFILE%\AppData\Local\Temp
    let up = env("USERPROFILE");
    if !up.is_empty() {
        return PathBuf::from(up).join("AppData").join("Local").join("Temp");
    }
    PathBuf::from("C:\\Temp")
}

/// Windows 系统临时目录 —— 对齐 paths.js#14-16 windowsTempDir()
pub fn windows_temp_dir() -> PathBuf {
    PathBuf::from(env_or("SystemRoot", "C:\\Windows")).join("Temp")
}

/// 用户配置文件目录 —— 对齐 paths.js#18-20 userProfile()
pub fn user_profile() -> PathBuf {
    PathBuf::from(env_or("USERPROFILE", ""))
}

/// %LOCALAPPDATA% —— 对齐 paths.js#22-24 localAppData()
pub fn local_app_data() -> PathBuf {
    let local = env("LOCALAPPDATA");
    if !local.is_empty() {
        return PathBuf::from(local);
    }
    user_profile().join("AppData").join("Local")
}

/// %APPDATA% (Roaming) —— 对齐 paths.js#26-28 appData()
pub fn app_data() -> PathBuf {
    let roaming = env("APPDATA");
    if !roaming.is_empty() {
        return PathBuf::from(roaming);
    }
    user_profile().join("AppData").join("Roaming")
}

/// %SystemRoot% —— 对齐 paths.js#30-32 systemRoot()
pub fn system_root() -> PathBuf {
    PathBuf::from(env_or("SystemRoot", "C:\\Windows"))
}

/// %ProgramFiles% —— 对齐 paths.js#34-36 programFiles()
pub fn program_files() -> PathBuf {
    PathBuf::from(env_or("ProgramFiles", "C:\\Program Files"))
}

/// %ProgramFiles(x86)% —— 对齐 paths.js#38-40 programFilesX86()
pub fn program_files_x86() -> PathBuf {
    PathBuf::from(env_or("ProgramFiles(x86)", "C:\\Program Files (x86)"))
}

/// %ProgramData% —— 对齐 paths.js#42-44 programData()
pub fn program_data() -> PathBuf {
    PathBuf::from(env_or("ProgramData", "C:\\ProgramData"))
}

/// 已知文件夹缓存 —— 对齐 paths.js#46-75 getKnownFolders()
/// 通过 PowerShell [Environment]::GetFolderPath 批量获取
static KNOWN_FOLDERS_CACHE: Lazy<Mutex<Option<HashMap<String, String>>>> = Lazy::new(|| Mutex::new(None));

pub async fn get_known_folders() -> Result<HashMap<String, String>, String> {
    // 快速路径: 缓存命中
    if let Some(cached) = KNOWN_FOLDERS_CACHE.lock().unwrap().as_ref() {
        return Ok(cached.clone());
    }

    let script = r#"
[Environment]::GetFolderPath('Desktop');
[Environment]::GetFolderPath('MyDocuments');
[Environment]::GetFolderPath('UserProfile');
[Environment]::GetFolderPath('ApplicationData');
[Environment]::GetFolderPath('LocalApplicationData');
[Environment]::GetFolderPath('CommonApplicationData');
[Environment]::GetFolderPath('ProgramFiles');
[Environment]::GetFolderPath('System');
[Environment]::GetFolderPath('Windows');
"#;

    let res = run_async("powershell", &["-NoProfile", "-NonInteractive", "-Command", script], None).await?;
    let lines: Vec<String> = res.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();

    let mut map = HashMap::new();
    if lines.len() >= 9 {
        map.insert("Desktop".to_string(), lines[0].clone());
        map.insert("Documents".to_string(), lines[1].clone());
        map.insert("UserProfile".to_string(), lines[2].clone());
        map.insert("AppData".to_string(), lines[3].clone());
        map.insert("LocalAppData".to_string(), lines[4].clone());
        map.insert("ProgramData".to_string(), lines[5].clone());
        map.insert("ProgramFiles".to_string(), lines[6].clone());
        map.insert("System".to_string(), lines[7].clone());
        map.insert("Windows".to_string(), lines[8].clone());
    }

    *KNOWN_FOLDERS_CACHE.lock().unwrap() = Some(map.clone());
    Ok(map)
}

/// Downloads 文件夹缓存 —— 对齐 paths.js#77-104 downloads()
/// 优先从注册表 User Shell Folders 读取 {374DE290-123F-4565-9164-39C4925E467B}
/// 兜底到 %USERPROFILE%\Downloads
static DOWNLOADS_CACHE: Lazy<Mutex<Option<PathBuf>>> = Lazy::new(|| Mutex::new(None));

pub async fn downloads() -> Result<Option<PathBuf>, String> {
    // 缓存命中
    if let Some(cached) = DOWNLOADS_CACHE.lock().unwrap().as_ref() {
        return Ok(Some(cached.clone()));
    }

    // 尝试注册表
    let res = run_async(
        "reg",
        &[
            "query",
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\User Shell Folders",
            "/v",
            "{374DE290-123F-4565-9164-39C4925E467B}",
        ],
        None,
    ).await;

    if let Ok(output) = res {
        // REG_EXPAND_SZ    %USERPROFILE%\Downloads
        for line in output.lines() {
            if line.contains("REG_") && (line.contains("EXPAND_SZ") || line.contains("SZ")) {
                // 提取值部分
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    let raw = parts[2..].join(" ").trim().trim_matches('"').to_string();
                    // 展开环境变量 %VAR%
                    let expanded = expand_env_vars(&raw);
                    let path = PathBuf::from(expanded);
                    if path.exists() {
                        *DOWNLOADS_CACHE.lock().unwrap() = Some(path.clone());
                        return Ok(Some(path));
                    }
                }
            }
        }
    }

    // 兜底
    let fallback = user_profile().join("Downloads");
    let result = if fallback.exists() { Some(fallback) } else { None };
    *DOWNLOADS_CACHE.lock().unwrap() = result.clone();
    Ok(result)
}

/// 回收站路径列表 —— 对齐 paths.js#106-113 recycleBins()
/// 遍历就绪的固定磁盘, 返回每个盘的 $Recycle.Bin
pub async fn recycle_bins() -> Result<Vec<PathBuf>, String> {
    let script = r#"[System.IO.DriveInfo]::GetDrives() | Where-Object { $_.IsReady -and $_.DriveType -eq 3 } | ForEach-Object { $_.RootDirectory.FullName }"#;
    let res = run_async("powershell", &["-NoProfile", "-NonInteractive", "-Command", script], None).await?;
    let drives: Vec<String> = res.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    Ok(drives.into_iter().map(|d| PathBuf::from(d).join("$Recycle.Bin")).collect())
}

/// 常见浏览器缓存目录 —— 对齐 paths.js#115-126 browserCacheDirs()
pub fn browser_cache_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let candidates = [
        local_app_data().join("Google").join("Chrome").join("User Data"),
        local_app_data().join("Microsoft").join("Edge").join("User Data"),
        local_app_data().join("Mozilla").join("Firefox").join("Profiles"),
    ];
    for c in candidates {
        if c.exists() {
            dirs.push(c);
        }
    }
    dirs
}

/// 环境变量展开: %VAR% -> 值 —— 对齐 paths.js#137 replace
fn expand_env_vars(input: &str) -> String {
    let mut result = input.to_string();
    // 简单替换, 避免正则依赖
    while let Some(start) = result.find('%') {
        if let Some(end) = result[start + 1..].find('%') {
            let end = start + 1 + end;
            let var_name = &result[start + 1..end];
            let value = env(var_name);
            result.replace_range(start..=end, &value);
        } else {
            break;
        }
    }
    result
}

/// 解析路径模板: {KnownFolder} + %ENV% -> 绝对路径(存在则返回, 否则 null)
/// 对齐 paths.js#132-141 resolvePath()
pub async fn resolve_path(template: &str) -> Result<Option<PathBuf>, String> {
    if template.trim().is_empty() {
        return Ok(None);
    }
    let mut p = template.trim().to_string();

    // 1. {KnownFolder} 替换
    let kf = get_known_folders().await?;
    for (name, value) in kf {
        let placeholder = format!("{{{}}}", name);
        p = p.replace(&placeholder, &value);
    }

    // 2. %ENV% 替换
    p = expand_env_vars(&p);

    // 3. 规范化
    let path = PathBuf::from(p);
    let abs = if path.is_absolute() {
        path
    } else {
        return Ok(None);
    };

    Ok(if abs.exists() { Some(abs) } else { None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_expand() {
        std::env::set_var("TEST_VAR", "hello");
        assert_eq!(expand_env_vars("%TEST_VAR%"), "hello");
        assert_eq!(expand_env_vars("C:\\%TEST_VAR%\\world"), "C:\\hello\\world");
    }

    #[test]
    fn test_user_profile() {
        let p = user_profile();
        assert!(p.is_absolute() || p.as_os_str().is_empty());
    }
}