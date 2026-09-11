-- V016__create_scanned_directories.sql
-- 记录已扫描的根目录，支撑「目录级移除」功能。
--
-- 设计说明：
--   - path 唯一：同一目录重复扫描只更新 updated_at，不产生重复行
--   - file_count 不存表：实时按 files.path LIKE 'prefix%' 统计，避免数据不一致
--   - 移除目录时删除本行 + 软删 files 中该目录下所有记录
CREATE TABLE scanned_directories (
    id          TEXT PRIMARY KEY,
    path        TEXT NOT NULL UNIQUE,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_scanned_directories_path ON scanned_directories(path);
