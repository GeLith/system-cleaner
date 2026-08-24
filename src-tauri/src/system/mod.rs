//! 系统原语层 —— 对应 Electron 版 src/main/system/*
//! 移植单元(每个文件对齐同名 js):
//!   exec.rs       spawn + GBK 解码(exec.js 的 decodeSmart 语义)
//!   registry.rs   reg query/add/delete 封装 + RUN_KEYS
//!   service.rs    sc query/config 封装
//!   process.rs    tasklist/taskkill 封装
//!   paths.js      环境变量展开/常用目录解析(含 Downloads 兜底)
//!   filesystem.rs 目录遍历(realpath 防环+深度/数量上限)/删除/重命名探测
pub mod elevate;
pub mod exec;
pub mod filesystem;
pub mod icons;
pub mod paths;
pub mod process;
pub mod registry;
pub mod service;
