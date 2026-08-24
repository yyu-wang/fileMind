-- V009__add_category_target_dir.sql
-- 分类增加「目标子目录」列：分类时把文件移动到「扫描根/该子目录」下。
-- 空字符串 = 不移动（仅打分类标签）。
-- 既有行默认空串，兼容旧库。

ALTER TABLE categories ADD COLUMN target_dir TEXT NOT NULL DEFAULT '';
