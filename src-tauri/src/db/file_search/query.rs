//! 查询串清洗与 FTS5 查询构造（原 `file_search.rs` 拆出）。

/// 清洗用户查询：保留 Unicode 字母数字（含中文/日文/韩文）、空白与下划线。
///
/// 安全说明：
/// - 移除 FTS5 特殊字符（``*`` ``"`` ``(`` ``)`` ``OR`` 等），防止语法注入
/// - ``is_alphanumeric`` 走 Unicode ``Alphabetic`` / ``Numeric`` 属性，CJK 字符
///   在该属性中为 true，因此中文查询能透传到 FTS5 MATCH
/// - ``is_whitespace`` 允许任意 Unicode 空白（含全角空格、制表符），便于多 token 查询
/// - 保留下划线 ``_``：常见于文件名（如 ``my_report.pdf``），FTS5 视为普通字符
pub(super) fn sanitize_query(query: &str) -> String {
    query
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '_')
        .collect::<String>()
        .trim()
        .to_string()
}

/// 构造 FTS5 查询字符串：对长 CJK 查询截断到前 2 字做前缀匹配。
///
/// 背景：sqlite FTS5 的 ``unicode61`` 分词器把连续 CJK 字符合并为一个 token。
/// 若整句作为前缀搜索（如 ``"分类整理的流程是什么"*``），需要文档里存在
/// 完整的长 token 才能命中——实际文档极少出现整句，导致检索为零。
/// 截断到前 2 字后（``"分类"*``），以词缀匹配即可找到包含该前缀的所有 token。
///
/// 策略：
/// - 包含 CJK 字符的查询（codepoint > 127）：取前 2 字做前缀
/// - 纯 ASCII 查询：保持整句前缀匹配（英文空格天然分词，整句效果好）
pub(super) fn build_fts_query(sanitized: &str) -> String {
    if sanitized.is_empty() {
        return String::new();
    }
    let has_cjk = sanitized.chars().any(|c| c as u32 > 127);
    if has_cjk {
        let prefix: String = sanitized.chars().take(2).collect();
        format!("\"{prefix}\"*")
    } else {
        format!("\"{sanitized}\"*")
    }
}
