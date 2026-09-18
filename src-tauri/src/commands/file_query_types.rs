//! 文件查询命令的 IPC 响应类型（原 `file_query.rs` 拆出）。
//!
//! 与命令实现分开：这些结构体是前端契约（`tauri-specta` 生成 `src/types/ipc.ts`），
//! 字段增删的影响面只在契约层，与命令的查询逻辑无关。

use serde::{Deserialize, Serialize};

use crate::db::{OperationBatchSummary, OperationLog};
use crate::FileInfo;

/// 分页文件列表响应。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct FileListResponse {
    /// 当前页文件。
    pub files: Vec<FileInfo>,
    /// 满足条件的总条数。
    #[specta(type = specta_typescript::Number)]
    pub total: i64,
    /// 当前页码（从 0 开始）。
    #[specta(type = specta_typescript::Number)]
    pub page: i64,
    /// 每页条数（实际生效值）。
    #[specta(type = specta_typescript::Number)]
    pub page_size: i64,
}

/// 文件库统计。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct FileStats {
    /// 文件总数。
    #[specta(type = specta_typescript::Number)]
    pub total_files: i64,
    /// 已分类文件数。
    #[specta(type = specta_typescript::Number)]
    pub categorized_files: i64,
    /// 未分类文件数。
    #[specta(type = specta_typescript::Number)]
    pub uncategorized_files: i64,
    /// 疑似重复文件组数。
    #[specta(type = specta_typescript::Number)]
    pub duplicate_groups: i64,
    /// 文件总大小（字节）。
    #[specta(type = specta_typescript::Number)]
    pub total_size_bytes: i64,
}

/// 操作历史响应（API §2-2d 返回值）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct OperationHistoryResponse {
    /// 批次摘要列表。
    pub batches: Vec<OperationBatchSummary>,
    /// 当前页码（从 1 开始）。
    #[specta(type = specta_typescript::Number)]
    pub page: i64,
    /// 每页条数（实际生效值）。
    #[specta(type = specta_typescript::Number)]
    pub page_size: i64,
}

/// 批次详情响应（API §2-2d `batch_id` 分支返回值）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct BatchDetailResponse {
    /// 批次 ID。
    pub batch_id: String,
    /// 批次内所有日志行（按 `created_at` 升序）。
    pub logs: Vec<OperationLog>,
}
