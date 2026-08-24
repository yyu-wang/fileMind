-- 增量扫描（T10.1）：为 files 表新增磁盘修改时间列。
--
-- 旧行 mtime 为 NULL → 首次增量扫描对既有文件退化为「重算 hash + 写回 mtime」，
-- 之后 (size, mtime) 均一致即可跳过重算（一次性迁移成本，可接受）。
ALTER TABLE files ADD COLUMN mtime TEXT;
