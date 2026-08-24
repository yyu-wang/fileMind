-- V007__operations_log_insert_only.sql
-- 07-T-02 缓解措施第 1 条：operations_log 表强制 INSERT-only。
--
-- 设计：
-- - BEFORE UPDATE：只允许 status 列变更（undo_batch 的 done→undone 需要），
--   修改其他列 → RAISE ABORT
-- - BEFORE DELETE：一律 RAISE ABORT（日志不可删除）
-- - 应用层已有 OperationRepo 无 DELETE 方法的约束，DB 触发器为第二道防线
--
-- 字段豁免一致性：触发器保护字段集 = log_chain::canonicalize 参与字段集，
-- status 在两边都被豁免（既不参与哈希，也是唯一允许 UPDATE 的列）。

-- 1) 仅允许 UPDATE status 列；修改其他列拒绝
CREATE TRIGGER operations_log_no_update
BEFORE UPDATE ON operations_log
FOR EACH ROW
WHEN OLD.id != NEW.id
   OR OLD.batch_id != NEW.batch_id
   OR OLD.operation_type != NEW.operation_type
   OR OLD.source_path != NEW.source_path
   OR OLD.target_path != NEW.target_path
   OR OLD.prev_hash != NEW.prev_hash
   OR OLD.current_hash != NEW.current_hash
   OR OLD.chain_hash != NEW.chain_hash
   OR OLD.created_at != NEW.created_at
BEGIN
    SELECT RAISE(ABORT, 'operations_log: 仅允许更新 status 列');
END;

-- 2) 完全禁止 DELETE
CREATE TRIGGER operations_log_no_delete
BEFORE DELETE ON operations_log
FOR EACH ROW
BEGIN
    SELECT RAISE(ABORT, 'operations_log: 禁止删除日志记录');
END;
