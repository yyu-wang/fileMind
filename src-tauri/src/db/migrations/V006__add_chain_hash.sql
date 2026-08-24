-- V006__add_chain_hash.sql
-- 为操作日志表增加链式哈希列（T3.5）：
-- 每条记录存 SHA256(前一条链哈希 + 本记录规范化数据)，形成防篡改链。
ALTER TABLE operations_log ADD COLUMN chain_hash TEXT NOT NULL DEFAULT '';
