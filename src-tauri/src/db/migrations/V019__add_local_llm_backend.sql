-- V019__add_local_llm_backend.sql
-- 应用配置增加「本地生成后端」与「内置生成模型」两列，支撑「部署机器没装 Ollama
-- 也能知识问答」：内置 llama.cpp 引擎跑 GGUF 权重（T3），由这两列决定是否启用、
-- 以及启用时加载哪份权重。
--
-- local_llm_backend：本地生成走哪个后端，取值 'ollama'（默认，沿用既有行为）或
--   'builtin'（Sidecar 内置 llama.cpp 引擎）。
-- local_llm_model：内置后端的 GGUF 模型标识（模型目录名，非文件名）。默认值须与
--   python-sidecar/app/services/model_specs.py 的 LLM_MODEL_NAME 以及
--   src-tauri/src/commands/config.rs 的 Default 实现保持一致（三处静态默认值，
--   迁移脚本一旦应用不可再改，故此处显式写明）。
--
-- llm_model 列语义不变（Ollama / 云端模型名），与内置后端的 GGUF 标识分属两个命名
-- 空间，不共用一列：否则切换后端时同一列会在两种命名之间漂移。

ALTER TABLE app_config ADD COLUMN local_llm_backend TEXT NOT NULL DEFAULT 'ollama';
ALTER TABLE app_config ADD COLUMN local_llm_model TEXT NOT NULL DEFAULT 'qwen2.5-3b-instruct';
