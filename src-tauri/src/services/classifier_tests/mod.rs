//! `classifier` 测试模块树：规则引擎（`generate_plan`）与 LLM 兜底（`apply_llm_fallback`）。
//!
//! 原因：原 `classifier_tests.rs`（433 行）超测试文件 400 行警告阈值，按关注点拆为
//! 两个测试模块，公共夹具在 `support`。声明集中在此处，`services/mod.rs` 只保留
//! 一行 `mod classifier_tests;`（与 `sidecar/manager_tests` 同构）。
//!
//! 注：测试文件用 `use super::super::classifier::*` 访问被测模块的公开项。

mod llm_fallback_tests;
mod plan_tests;
mod support;
