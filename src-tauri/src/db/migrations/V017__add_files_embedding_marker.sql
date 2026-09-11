-- V017__add_files_embedding_marker.sql
-- files 表增加向量索引状态标记列（增量索引"跳过已建文件"的判断依据）

-- embedding_model: 最后一次向量化所用的 embedding 模型名；NULL = 从未建过索引
ALTER TABLE files ADD COLUMN embedding_model TEXT;
-- embedding_hash: 建索引那一刻的 content_hash；与当前 content_hash 不一致 = 内容已变更需重建
ALTER TABLE files ADD COLUMN embedding_hash TEXT;
