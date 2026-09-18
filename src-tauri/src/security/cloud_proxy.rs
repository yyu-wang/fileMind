//! 云端 LLM 请求代理（安全 07-§4 / T7.4）。
//!
//! Sidecar 组装好（已脱敏的）OpenAI Chat Completions 请求体 POST 到本代理 →
//! Rust 从 Keychain 读取 Key 并注入 `Authorization: Bearer` → 转发到云端 API →
//! 原样回传响应（含 SSE 流式，JSON/流共用一条转发路径）。API Key 全程不进入
//! Python 进程内存。
//!
//! 传输层（共享 Client、转发、响应回传、服务启动）已按职责拆至
//! [`super::cloud_proxy_server`]（2026-09-18，原文件 335 行超 Rust 模块警告阈值
//! 300），本模块保留代理状态、上游地址解析与鉴权。
//!
//! 安全措施：
//! - 共享 token 鉴权（`X-FileMind-Token`）：防本机其他进程盗用云端 API 配额
//! - 上游 URL 来源：用户在设置页写入 `cloud_providers.base_url`（P-07 自定义提供商），
//!   代理在每次请求时查 DB 实时取；表单已在入库前校验为 https 或本地回环 http，
//!   Python/Sidecar 无法绕过。无 DB 句柄环境（单测）回落旧硬编码白名单。
//! - Key 仅作局部变量，使用后经 `zeroize` 清零再 drop（`unsafe_code=deny`）
//! - 审计日志只记 provider 与是否流式，不记 Key、不记内容（T7.1 二次兜底）

use std::sync::{Arc, Mutex};

use axum::http::HeaderMap;

pub use super::cloud_proxy_server::spawn_proxy_server;
use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::security;

/// 代理监听地址（仅本机回环）。
pub const CLOUD_PROXY_HOST: &str = "127.0.0.1";
/// 代理监听端口（固定，与 Sidecar 8765 相邻）。
pub const CLOUD_PROXY_PORT: u16 = 8766;

/// 调用方鉴权请求头；Sidecar 经 `FILEMIND_CLOUD_PROXY_TOKEN` env 取值。
const TOKEN_HEADER: &str = "x-filemind-token";

/// 常量时间字符串比较（BE-m4）：逐字节异或累积，不因首个不匹配字节提前返回，
/// 避免逐字节计时侧信道猜测 token。长度差折叠进同一累积值，长度信息也不泄漏。
/// 回环 + 256bit 随机 token 下实际风险极低，顺手加固。
#[must_use]
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    // 先累计长度差：长度不等时仍完整跑完字节比较，总耗时与内容无关
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        let byte_a = a.get(i).copied().unwrap_or(0);
        let byte_b = b.get(i).copied().unwrap_or(0);
        diff |= usize::from(byte_a ^ byte_b);
    }
    diff == 0
}

/// 云端代理状态（启动时构造，随 axum 路由共享）。
#[derive(Clone)]
pub struct CloudProxyState {
    /// 本机调用方共享 token（仅内存 + Sidecar env 持有）。
    token: String,
    /// 测试用上游 URL 覆盖；生产为 `None`（走 DB 查询）。
    upstream_override: Option<String>,
    /// Keychain 读取器（fn 指针，测试可注入假实现）。
    key_reader: fn(&str) -> AppResult<Option<String>>,
    /// 运行时 DB 句柄（P-07：查 `base_url` 的唯一真源）。
    /// 单测环境为 `None`；main.rs 生产启动必须注入。
    db: Option<Arc<Mutex<Database>>>,
}

impl CloudProxyState {
    /// 生产构造：Keychain 读取器固定为 `security::get_key`，无 DB（DB 另外调用
    /// [`Self::with_db`] 注入，避免在 `main.rs` 改动前编译失败）。
    #[must_use]
    pub fn new(token: String) -> Self {
        Self {
            token,
            upstream_override: None,
            key_reader: security::get_key,
            db: None,
        }
    }

    /// 给生产实例补 DB 句柄（链式调用，main.rs 中 `CloudProxyState::new(t).with_db(db)`）。
    #[must_use]
    pub fn with_db(mut self, db: Arc<Mutex<Database>>) -> Self {
        self.db = Some(db);
        self
    }

    /// 测试构造：注入假 Key 读取器与假上游 URL。
    #[cfg(test)]
    fn with_fakes(
        token: String,
        key_reader: fn(&str) -> AppResult<Option<String>>,
        upstream_override: String,
    ) -> Self {
        Self {
            token,
            upstream_override: Some(upstream_override),
            key_reader,
            db: None,
        }
    }

    /// 校验请求头中的共享 token（常量时间比较，BE-m4）。
    #[must_use]
    pub(super) fn token_matches(&self, headers: &HeaderMap) -> bool {
        headers
            .get(TOKEN_HEADER)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| constant_time_eq(v, &self.token))
    }

    /// 读取指定 provider 的 API Key（仅内存短驻，透传 Keychain 结果）。
    ///
    /// # Errors
    ///
    /// Keychain 读取失败时返回对应错误。
    pub(super) fn read_key(&self, provider: &str) -> AppResult<Option<String>> {
        (self.key_reader)(provider)
    }

    /// 目标上游 URL：测试覆盖 → DB 查询 → 硬编码白名单兜底。
    ///
    /// 返回时自动在 `base_url` 后拼接 `/chat/completions`，供请求方直接 POST。
    #[must_use]
    pub(super) fn upstream_url(&self, provider: &str) -> Option<String> {
        if let Some(override_url) = &self.upstream_override {
            return Some(override_url.clone());
        }
        // 1) DB 有值 → 用户自定义 base_url（P-07 自定义提供商）
        if let Some(db_arc) = &self.db {
            let guard = match db_arc.lock() {
                Ok(g) => g,
                Err(e) => {
                    log::error!("cloud_proxy upstream_url: DB lock poisoned ({e})");
                    return None;
                }
            };
            match crate::db::ConfigRepo::get_provider_base_url(guard.conn(), provider) {
                Ok(Some(base)) if !base.ends_with('/') => {
                    return Some(format!("{base}/chat/completions"));
                }
                Ok(Some(base)) => {
                    // 表单已在入库时禁止尾斜杠；此处兜底避免 404 双斜杠
                    return Some(format!("{base}chat/completions"));
                }
                Ok(None) => {
                    // 故意不落 error：首次升级 / 用户新建中，可能短暂为空
                    log::debug!("cloud_proxy: provider={provider} 未配置 base_url");
                }
                Err(e) => {
                    log::error!("cloud_proxy upstream_url: DB 读取失败 ({e})");
                    return None;
                }
            }
        }
        // 2) DB 无句柄或无记录 → 编译期白名单兜底（升级过渡 + 单测/CLI）
        provider_upstream_legacy(provider).map(str::to_owned)
    }
}

/// 升级过渡期的硬编码白名单：仅当 DB 无数据时兜底使用。
/// 保持与 P-07 改造前一致，保证升级瞬间旧请求不崩。
#[must_use]
fn provider_upstream_legacy(provider: &str) -> Option<&'static str> {
    match provider {
        "openai" => Some("https://api.openai.com/v1/chat/completions"),
        "deepseek" => Some("https://api.deepseek.com/chat/completions"),
        _ => None,
    }
}

/// 生成调用方共享 token（32 字节系统熵随机 hex）。
///
/// # Errors
///
/// 系统熵源不可用时返回 `SidecarUnavailable`。
pub fn generate_token() -> AppResult<String> {
    use rand::TryRng;
    let mut bytes = [0u8; 32];
    rand::rngs::SysRng
        .try_fill_bytes(&mut bytes)
        .map_err(|e| AppError::SidecarUnavailable(format!("云端代理 token 生成失败: {e}")))?;
    Ok(hex::encode(bytes))
}

#[cfg(test)]
#[path = "cloud_proxy_tests.rs"]
mod tests;

// lint fix notes: doc_markdown (base_url 反引号)
