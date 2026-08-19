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
    #[specta(type = specta_typescript::Number)]
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

/// `operations_log` 表记录：批量操作审计日志（行级）。
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
    /// 执行状态（pending/done/failed/undone）。
    pub status: String,
    /// 操作前内容哈希。
    pub prev_hash: String,
    /// 操作后内容哈希。
    pub current_hash: String,
    /// 链式哈希：SHA256(前一条链哈希 + 本记录规范化数据)，用于防篡改。
    pub chain_hash: String,
    /// 记录时间。
    pub created_at: String,
}

/// `operations_log` 按 `batch_id` 聚合的批次摘要（API §2-2d `get_operation_history` 返回值）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct OperationBatchSummary {
    /// 批次 ID。
    pub batch_id: String,
    /// 操作类型（取批次内首条行，move/rename/delete）。
    pub op_type: String,
    /// 批次总条数。
    #[specta(type = specta_typescript::Number)]
    pub total_count: i64,
    /// 成功条数（status='done'）。
    #[specta(type = specta_typescript::Number)]
    pub success_count: i64,
    /// 失败条数（status='failed'）。
    #[specta(type = specta_typescript::Number)]
    pub failed_count: i64,
    /// 批次状态（pending/done/failed/undone）。
    pub status: String,
    /// 首条记录时间（批次创建时间近似）。
    pub created_at: String,
    /// 批次内是否含 delete 操作（含则不可撤销，T3.4 已确认 delete 不可恢复）。
    pub has_delete: bool,
    /// 是否可撤销（DB 层）：`status='done' && !has_delete`。
    /// IPC 命令层（`get_operation_history`）会再注入撤销窗口判断。
    pub can_undo: bool,
}

/// `categories` 表记录：分类体系节点。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct Category {
    /// 主键（UUID 或系统预置 slug）。
    pub id: String,
    /// 分类名（UNIQUE）。
    pub name: String,
    /// 父分类 ID（根分类为 `None`）。
    pub parent_id: Option<String>,
    /// 图标标识（前端映射）。
    pub icon: Option<String>,
    /// 颜色标识（前端映射）。
    pub color: Option<String>,
    /// 排序权重（升序）。
    #[specta(type = specta_typescript::Number)]
    pub sort_order: i64,
    /// 系统预置标记（1=内置不可删，0=用户自定义）。
    pub is_builtin: bool,
    /// 目标子目录（相对扫描根的路径）。分类时把文件移动到 `scan_root/target_dir`；
    /// 空字符串表示不移动（仅打分类标签）。
    pub target_dir: String,
    /// 入库时间。
    pub created_at: String,
    /// 最后更新时间。
    pub updated_at: String,
}

/// 分类树节点：`Category` + 递归子节点。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[allow(clippy::use_self)]
pub struct CategoryNode {
    /// 当前节点。
    #[serde(flatten)]
    pub category: Category,
    /// 子节点列表。
    pub children: Vec<CategoryNode>,
}

/// `rules` 表记录：分类规则。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct Rule {
    /// 主键（UUID）。
    pub id: String,
    /// 规则名。
    pub name: String,
    /// 规则类型（`extension`/`path_keyword`/`magic_number`/`size`/`regex`）。
    pub rule_type: String,
    /// 匹配模式（如 `"pdf,doc"` 或 `"\d{4}-\d{2}"`）。
    pub pattern: String,
    /// 匹配后归入的分类 ID。
    pub target_category: Option<String>,
    /// 优先级（数字越大越先匹配）。
    #[specta(type = specta_typescript::Number)]
    pub priority: i64,
    /// 是否启用。
    pub is_enabled: bool,
    /// 入库时间。
    pub created_at: String,
    /// 最后更新时间。
    pub updated_at: String,
}
