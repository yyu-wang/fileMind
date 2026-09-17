//! 云提供商的对外 IPC 契约（specta 导出）。

use serde::{Deserialize, Serialize};

/// 云提供商记录（`cloud_providers` 表的一行，不含 API Key——Key 只在 Keychain）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct CloudProviderRecord {
    /// 记录主键（UUID 字符串）。
    pub id: String,
    /// 提供商标识 slug（`[a-z0-9_-]{1,64}`），全局唯一、软删除外仍唯一。
    pub provider_key: String,
    /// 显示名称（如「Claude 官方」「公司代理中转」）。
    pub name: String,
    /// 备注（纯 UI 展示，如「公司专用账号」）。
    pub remark: String,
    /// 官网链接（可选）。
    pub website: Option<String>,
    /// 请求地址前缀（OpenAI 兼容 `base_url`）。
    pub base_url: String,
    /// 是否为迁移时内置的两条模板（OpenAI/DeepSeek）。
    pub is_builtin: bool,
    /// 创建时间（ISO 8601 UTC）。
    pub created_at: String,
    /// 更新时间（ISO 8601 UTC）。
    pub updated_at: String,
}

/// 新建 / 更新提供商时的请求体（不含 id / 时间戳）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct CloudProviderUpsertInput {
    /// 提供商标识（slug）；创建时必填，更新时用它定位到目标记录。
    pub provider_key: String,
    /// 显示名称（1-128 字符）。
    pub name: String,
    /// 备注（最长 255 字符，空串允许）。
    pub remark: Option<String>,
    /// 官网链接（必须 `https://` 或留空；`http://` 仅 `localhost`/`127.0.0.1`）。
    pub website: Option<String>,
    /// 请求地址前缀（必须是合法 URL，不能以 `/` 结尾）。
    pub base_url: String,
}
