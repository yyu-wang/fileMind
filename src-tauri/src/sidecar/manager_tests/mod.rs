//! `SidecarManager` 测试模块树。
//!
//! 原因：原 `manager_tests.rs`（899 行）超测试文件 600 行阈值，按关注点拆为 4 个测试模块，
//! 公共夹具在 `support`。声明集中在此处，`manager.rs` 只保留一行 `mod manager_tests;`——
//! 若把声明写回 manager.rs，会把该文件顶过行数基线（门禁按「增长」判 FAIL）。
//!
//! 注：测试文件用 `use super::super::*;` 访问 `manager` 的私有项（本模块树是 manager 的后代），
//! 再用 `use super::support::*;` 取公共夹具。

mod orphan_tests;
mod paths_tests;
mod start_tests;
mod state_tests;
mod support;
