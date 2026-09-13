-- V013__drop_temperature.sql
-- 移除 V012 引入的 temperature 列：RAG 问答为确定性任务，
-- Temperature 由服务端固定（0.2），不对用户开放，因此不需要持久化。
-- SQLite 3.35+ 原生支持 ALTER TABLE DROP COLUMN。

ALTER TABLE app_config DROP COLUMN temperature;
