-- V003__create_categories_table.sql
-- 创建分类表

CREATE TABLE categories (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    parent_id   TEXT REFERENCES categories(id),
    icon        TEXT,
    color       TEXT,
    sort_order  INTEGER DEFAULT 0,
    is_builtin  INTEGER DEFAULT 0,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_categories_parent_id ON categories(parent_id);
CREATE INDEX idx_categories_sort_order ON categories(sort_order);
