-- V010__add_llm_model.sql
-- 应用配置增加「LLM 模型」列：本地聊天/分类使用的推理模型名（如 qwen3.8-27b）。
-- 既有行默认 qwen3.8-27b，兼容旧库。

ALTER TABLE app_config ADD COLUMN llm_model TEXT NOT NULL DEFAULT 'qwen3.8-27b';
