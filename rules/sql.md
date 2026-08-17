# SQL / SQLite 数据库规范

> 来源：06_工程化基础规范.html §6

## 命名约定

| 对象 | 约定 | 示例 |
|------|------|------|
| 表名 | snake_case，复数 | `files`, `categories`, `operations_log` |
| 列名 | snake_case | `content_hash`, `created_at` |
| 主键 | 统一 `id TEXT PRIMARY KEY`（UUID） | `id` |
| 外键 | `{ref_table_singular}_id` | `file_id`, `category_id` |
| 索引 | `idx_{table}_{columns}` | `idx_files_content_hash` |
| FTS 表 | `{table}_fts` | `file_fts` |
| 时间戳 | `{action}_at`，TEXT ISO 8601 | `created_at`, `updated_at` |
| 布尔值 | `is_{state}`，INTEGER 0/1 | `is_deleted`, `is_cloud` |

## 迁移脚本规范

### 文件命名
```
src-tauri/src/db/migrations/V{N}__{description}.sql
```
- 序号递增（V001, V002, ...），不跳号
- 一旦提交不可修改（refinery 用文件名哈希追踪）

### 脚本格式
```sql
-- V001__create_files_table.sql
-- 创建文件元数据表

CREATE TABLE files (
    id           TEXT PRIMARY KEY,
    path         TEXT NOT NULL UNIQUE,
    file_name    TEXT NOT NULL,
    file_size    INTEGER NOT NULL,
    content_hash TEXT,
    category_id  TEXT REFERENCES categories(id),
    is_deleted   INTEGER DEFAULT 0,
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_files_content_hash ON files(content_hash);
```

### 迁移规则
- DDL 变更和 DML 数据迁移分在不同迁移脚本中
- 每个迁移必须包含 `-- {description}` 注释头
- Embedding 模型版本变更时新建 LanceDB 表（`documents_{model}_v{version}`），不修改旧表

## 索引规范
- 所有外键列建索引
- 高频查询的过滤列建索引
- FTS5 虚拟表用触发器同步
- 不建冗余索引（SQLite 查询计划器自动选择）
