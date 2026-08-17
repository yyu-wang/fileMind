//! 数据库表对应的持久化模型。

use serde::{Deserialize, Serialize};

/// `files` 表记录：扫描得到的文件元数据。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct FileRecord {
    /// 主键（UUID）。
    pub id: String,
    /// 文件绝对路径。
    pub path: String,
    /// 文件名。
    pub file_name: String,
    /// 文件大小（字节）。
    pub file_size: i64,
    /// 内容哈希（SHA-256，未计算时为 `None`）。
    pub content_hash: Option<String>,
    /// AI 分类结果。
    pub category: Option<String>,
    /// 软删除标记。
    pub is_deleted: bool,
    /// 入库时间。
    pub created_at: String,
    /// 最后更新时间。
    pub updated_at: String,
}

/// `operation_logs` 表记录：批量操作审计日志。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct OperationLog {
    /// 主键（UUID）。
    pub id: String,
    /// 所属批次 ID。
    pub batch_id: String,
    /// 操作类型（move/rename/delete）。
    pub operation_type: String,
    /// 源路径。
    pub source_path: String,
    /// 目标路径。
    pub target_path: String,
    /// 执行状态。
    pub status: String,
    /// 操作前内容哈希。
    pub prev_hash: String,
    /// 操作后内容哈希。
    pub current_hash: String,
    /// 记录时间。
    pub created_at: String,
}
