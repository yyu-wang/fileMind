-- V014__create_cloud_providers.sql
-- 新建 cloud_providers 自定义云提供商表，并给 app_config 追加 active_cloud_provider 列。
-- 用户在设置页新增的每条提供商（名称/备注/官网/base_url）都存在本表；
-- API Key 仍走系统 Keychain（以 provider_key 作为键），本表只存元数据。

CREATE TABLE cloud_providers (
    id               TEXT PRIMARY KEY,
    provider_key     TEXT NOT NULL UNIQUE,
    name             TEXT NOT NULL,
    remark           TEXT NOT NULL DEFAULT '',
    website          TEXT,
    base_url         TEXT NOT NULL,
    is_builtin       INTEGER NOT NULL DEFAULT 0,
    is_deleted       INTEGER NOT NULL DEFAULT 0,
    created_at       TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at       TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE UNIQUE INDEX idx_cloud_providers_provider_key
    ON cloud_providers(provider_key) WHERE is_deleted = 0;
CREATE INDEX idx_cloud_providers_is_deleted ON cloud_providers(is_deleted);

-- app_config.active_cloud_provider：记录当前用户选中用于推理的 provider_key。
-- SQLite 不支持 ALTER TABLE ADD COLUMN 带 DEFAULT NULL 写法（其实支持），
-- 这里保持与现有字段风格一致：允许 NULL 表示未指定。
ALTER TABLE app_config ADD COLUMN active_cloud_provider TEXT;
