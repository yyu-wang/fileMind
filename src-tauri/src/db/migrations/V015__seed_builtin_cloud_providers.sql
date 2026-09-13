-- V015__seed_builtin_cloud_providers.sql
-- 为 cloud_providers 表预置两条内建记录（OpenAI 官方 / DeepSeek 官方），
-- 配合 014 迁移完成后首次启动时注入，使升级用户 UI 直接看到历史条目。
-- 只有当同 provider_key 不存在时才插入（幂等，重复跑迁移不覆盖用户自定义）。

INSERT INTO cloud_providers (id, provider_key, name, remark, website, base_url, is_builtin)
SELECT
    lower(hex(randomblob(16))),
    'openai',
    'OpenAI 官方',
    '内置 · ChatGPT 系列模型',
    'https://openai.com',
    'https://api.openai.com/v1',
    1
WHERE NOT EXISTS (SELECT 1 FROM cloud_providers WHERE provider_key = 'openai');

INSERT INTO cloud_providers (id, provider_key, name, remark, website, base_url, is_builtin)
SELECT
    lower(hex(randomblob(16))),
    'deepseek',
    'DeepSeek 官方',
    '内置 · DeepSeek Chat / Reasoner 系列模型',
    'https://www.deepseek.com',
    'https://api.deepseek.com',
    1
WHERE NOT EXISTS (SELECT 1 FROM cloud_providers WHERE provider_key = 'deepseek');
