-- V008__create_app_config.sql
-- 创建应用配置表（单行表，id 固定为 1）
-- 存储：数据目录、推理模式、Embedding 模型、引导完成状态、云端同意书状态

CREATE TABLE app_config (
    id                       INTEGER PRIMARY KEY CHECK (id = 1),
    data_directory           TEXT    NOT NULL DEFAULT '',
    inference_mode           TEXT    NOT NULL DEFAULT 'local',
    embedding_model          TEXT    NOT NULL DEFAULT 'bge-small-zh-v1.5',
    max_file_size_mb         INTEGER NOT NULL DEFAULT 100,
    language                 TEXT    NOT NULL DEFAULT 'zh-CN',
    onboarding_completed    INTEGER NOT NULL DEFAULT 0,
    cloud_consent_signed     INTEGER NOT NULL DEFAULT 0,
    cloud_consent_version   TEXT,
    cloud_consent_provider   TEXT,
    cloud_consent_signed_at  TEXT
);

-- 初始化单行配置（首次启动时使用 Default 值）
INSERT INTO app_config (id) VALUES (1);
