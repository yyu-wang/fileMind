-- V012__add_cloud_model.sql
-- 应用配置增加「云端模型」列：云端推理使用的模型名（如 gpt-4o、deepseek-chat）。
-- RAG 问答与文件分类均为确定性任务，Temperature 由服务端固定，不对用户开放。
-- 既有行默认空值，兼容旧库。

ALTER TABLE app_config ADD COLUMN cloud_model TEXT NOT NULL DEFAULT '';
