-- V012__add_cloud_model_and_temperature.sql
-- 应用配置增加「云端模型」列与「Temperature」列。
-- 云端模型：云端推理使用的模型名（如 gpt-4o、deepseek-chat）。
-- Temperature：云端采样温度（0.0-1.0），RAG 问答推荐 0.2。
-- 既有行默认空值 / 0.2，兼容旧库。

ALTER TABLE app_config ADD COLUMN cloud_model TEXT NOT NULL DEFAULT '';
ALTER TABLE app_config ADD COLUMN temperature REAL NOT NULL DEFAULT 0.2;
