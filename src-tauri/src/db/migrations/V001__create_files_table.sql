-- V001__create_files_table.sql
-- 创建文件元数据表

CREATE TABLE files (
    id           TEXT PRIMARY KEY,
    path         TEXT NOT NULL UNIQUE,
    file_name    TEXT NOT NULL,
    file_size    INTEGER NOT NULL,
    content_hash TEXT,
    category     TEXT,
    is_deleted   INTEGER DEFAULT 0,
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_files_content_hash ON files(content_hash);
CREATE INDEX idx_files_category ON files(category);
CREATE INDEX idx_files_is_deleted ON files(is_deleted);
