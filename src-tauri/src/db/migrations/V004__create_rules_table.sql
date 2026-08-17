-- V004__create_rules_table.sql
-- 创建分类规则表

CREATE TABLE rules (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    rule_type    TEXT NOT NULL,
    pattern      TEXT NOT NULL,
    target_category TEXT REFERENCES categories(id),
    priority     INTEGER DEFAULT 100,
    is_enabled   INTEGER DEFAULT 1,
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_rules_priority ON rules(priority);
CREATE INDEX idx_rules_is_enabled ON rules(is_enabled);
CREATE INDEX idx_rules_rule_type ON rules(rule_type);
