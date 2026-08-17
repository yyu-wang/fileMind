-- V005__create_file_fts.sql
-- 创建全文检索表（FTS5 + jieba 分词）

CREATE VIRTUAL TABLE file_fts USING fts5(
    file_id UNINDEXED,
    file_name,
    content,
    path,
    tokenize = 'unicode61'
);

CREATE TRIGGER files_ai AFTER INSERT ON files BEGIN
    INSERT INTO file_fts(file_id, file_name, content, path)
    VALUES (new.id, new.file_name, '', new.path);
END;

CREATE TRIGGER files_ad AFTER DELETE ON files BEGIN
    DELETE FROM file_fts WHERE file_id = old.id;
END;

CREATE TRIGGER files_au AFTER UPDATE ON files BEGIN
    DELETE FROM file_fts WHERE file_id = old.id;
    INSERT INTO file_fts(file_id, file_name, content, path)
    VALUES (new.id, new.file_name, '', new.path);
END;
