-- V018__normalize_embedding_model.sql
-- 把 app_config.embedding_model 归一到当前唯一支持的值 bge-large-zh-v1.5
--
-- 背景：V008 的种子 INSERT 只写了 id，embedding_model 落到了该列的默认值
-- 'bge-small-zh-v1.5'。Embedding 改为 Sidecar 进程内 ONNX 推理后，注册表
-- （python-sidecar/app/core/embedding_models.py）只保留 bge-large-zh-v1.5，
-- 旧值会让索引构建（/index/build）与 RAG 问答（/chat/stream）直接失败。
--
-- 说明：
-- - V008 已应用，按 refinery 规则不可修改；此处用新迁移修正数据（V008 的列默认
--   值仅在种子 INSERT 时生效，此后所有写入均由 ConfigRepo::save 显式绑定该列）。
-- - 不动 files.embedding_model / embedding_hash：旧值 != 新模型名，增量索引会
--   自动把受影响的文件重新向量化（见 FileRepo::list_pending_embedding）。
UPDATE app_config
SET embedding_model = 'bge-large-zh-v1.5'
WHERE embedding_model <> 'bge-large-zh-v1.5';
