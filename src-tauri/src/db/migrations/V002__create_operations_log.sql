-- V002__create_operations_log.sql
-- 创建文件操作日志表（含链式哈希）

CREATE TABLE operations_log (
    id            TEXT PRIMARY KEY,
    batch_id      TEXT NOT NULL,
    operation_type TEXT NOT NULL,
    source_path   TEXT NOT NULL,
    target_path   TEXT,
    status        TEXT NOT NULL DEFAULT 'pending',
    prev_hash     TEXT NOT NULL,
    current_hash  TEXT NOT NULL,
    created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_operations_batch_id ON operations_log(batch_id);
CREATE INDEX idx_operations_created_at ON operations_log(created_at);
