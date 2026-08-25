//! SSE 帧解析器：跨网络块累积字节缓冲，按 `\n\n` 切帧，解析 `event:`/`data:` 行。
//!
//! 纯函数设计，可独立单测：跨块切分、单批多帧、畸形帧跳过、空缓冲无帧。

use serde_json::Value;

/// 帧分隔符：sidecar `_sse()` 输出的 `event: x\ndata: y\n\n`。
const FRAME_SEPARATOR: &[u8] = b"\n\n";
/// 缓冲上限（BE-m7）：上游异常（持续输出但从不发帧分隔符）时防无界增长；
/// 超限清空缓冲，后续帧解析不受影响。1MB 对单帧 token/citation 载荷绰绰有余。
const MAX_BUFFER_LEN: usize = 1024 * 1024;
/// `event:` 行前缀。
const EVENT_PREFIX: &str = "event:";
/// `data:` 行前缀。
const DATA_PREFIX: &str = "data:";

/// 解析后的 SSE 帧：事件名 + JSON 载荷。
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::module_name_repetitions)]
pub struct SseFrame {
    /// 事件名（如 `search_start` / `token` / `citation` / `done` / `error`）。
    pub event: String,
    /// 事件载荷（`data:` 行 JSON 反序列化结果）。
    pub data: Value,
}

/// 流式 SSE 解析器。
///
/// 使用字节缓冲（而非字符串拼接），避免跨网络块的 UTF-8 多字节字符
/// 在解码时被截断——只有完整帧才做 UTF-8 解码与 JSON 解析。
#[derive(Debug, Default)]
#[allow(clippy::module_name_repetitions)]
pub struct SseParser {
    /// 尚未构成完整帧的字节尾部。
    buffer: Vec<u8>,
}

impl SseParser {
    /// 追加一段网络字节，返回本次新增的完整帧。
    ///
    /// 畸形帧（缺 `event:` 行、缺 `data:` 行或 data 非 JSON）被跳过，
    /// 不影响后续帧解析；未完成的字节保留在内部缓冲，等待后续块补齐。
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseFrame> {
        self.buffer.extend_from_slice(chunk);
        // BE-m7：SSE 规范允许 CR / CRLF / LF 三种行尾，统一规范化为 \n 再切帧。
        // \r 是 ASCII，不可能出现在 UTF-8 多字节序列中间，替换不破坏字符边界。
        normalize_crlf(&mut self.buffer);
        let mut frames = Vec::new();
        while let Some(end) = find_frame_end(&self.buffer) {
            let raw = self.buffer.drain(..end).collect::<Vec<u8>>();
            if let Some(frame) = parse_frame(&raw) {
                frames.push(frame);
            }
        }
        // BE-m7：无分隔符的异常流防无界缓冲：超限清空（丢弃的是未成帧的垃圾，
        // 正常帧在上方循环已全部取出，不受影响）
        if self.buffer.len() > MAX_BUFFER_LEN {
            self.buffer.clear();
        }
        frames
    }
}

/// 缓冲内 `\r\n` / 孤立 `\r` 统一为 `\n`（BE-m7，SSE 规范三种行尾兼容）。
///
/// 末尾孤立的 `\r` 保留不转换：可能是跨网络块的 `\r\n` 前半，下一块到达后
/// 整缓冲再次规范化时自然消歧（`\r\n` 合并为一个 `\n`；`\r`+其他字节则各自成行）。
fn normalize_crlf(buffer: &mut Vec<u8>) {
    let mut write = 0;
    let mut read = 0;
    let len = buffer.len();
    while read < len {
        let byte = buffer[read];
        if byte == b'\r' && read + 1 < len {
            // 非末尾 \r：\r\n 合并为一个 \n，孤立 \r 转为 \n
            buffer[write] = b'\n';
            read += if buffer[read + 1] == b'\n' { 2 } else { 1 };
        } else {
            // 普通字节或末尾孤立 \r（保留原样待下一块消歧）
            buffer[write] = byte;
            read += 1;
        }
        write += 1;
    }
    buffer.truncate(write);
}

/// 定位缓冲区中第一个帧分隔符 `\n\n`，返回分隔符之后的索引。
#[must_use]
fn find_frame_end(buffer: &[u8]) -> Option<usize> {
    buffer
        .windows(FRAME_SEPARATOR.len())
        .position(|w| w == FRAME_SEPARATOR)
        .map(|start| start + FRAME_SEPARATOR.len())
}

/// 解析单个完整帧（含尾部 `\n\n`）为 [`SseFrame`]；畸形帧返回 `None`。
#[must_use]
fn parse_frame(raw: &[u8]) -> Option<SseFrame> {
    let text = std::str::from_utf8(raw).ok()?;
    let mut event: Option<String> = None;
    let mut data_lines: Vec<String> = Vec::new();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix(EVENT_PREFIX) {
            event = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix(DATA_PREFIX) {
            data_lines.push(value.trim().to_string());
        }
    }
    let event = event?;
    if data_lines.is_empty() {
        return None;
    }
    // SSE 规范：多行 `data:` 以 `\n` 拼接为单一载荷。
    let data = serde_json::from_str(&data_lines.join("\n")).ok()?;
    Some(SseFrame { event, data })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

    use super::*;

    #[test]
    fn parses_single_frame() {
        let mut parser = SseParser::default();
        let frames = parser.feed("event: token\ndata: {\"content\":\"你\"}\n\n".as_bytes());
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].event, "token");
        assert_eq!(frames[0].data, serde_json::json!({ "content": "你" }));
    }

    #[test]
    fn parses_multiple_frames_in_one_batch() {
        let mut parser = SseParser::default();
        let frames = parser.feed(
            b"event: search_start\ndata: {\"q\":\"a\"}\n\nevent: done\ndata: {\"ok\":true}\n\n",
        );
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].event, "search_start");
        assert_eq!(frames[1].event, "done");
        assert_eq!(frames[1].data, serde_json::json!({ "ok": true }));
    }

    #[test]
    fn parses_frame_split_across_chunks() {
        // 「你」= 3 字节 UTF-8（E4 BD A0），先喂前 2 字节，再喂余下字节，验证不截断。
        let mut parser = SseParser::default();
        let first = parser.feed(b"event: token\ndata: {\"content\":\"\xe4\xbd");
        assert!(first.is_empty(), "缺分隔符不应产出帧");
        let second = parser.feed(b"\xa0\"}\n\n");
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].event, "token");
        assert_eq!(second[0].data, serde_json::json!({ "content": "你" }));
    }

    #[test]
    fn separator_split_across_chunks() {
        let mut parser = SseParser::default();
        assert!(parser
            .feed(b"event: done\ndata: {\"ok\":true}\n")
            .is_empty());
        let frames = parser.feed(b"\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].event, "done");
    }

    #[test]
    fn retains_partial_without_separator() {
        let mut parser = SseParser::default();
        assert!(parser
            .feed(b"event: token\ndata: {\"content\":\"part")
            .is_empty());
        let frames = parser.feed(b"ial\"}\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].data, serde_json::json!({ "content": "partial" }));
    }

    #[test]
    fn skips_malformed_frames() {
        let mut parser = SseParser::default();
        // 缺 event 行
        assert!(parser.feed(b"data: {}\n\n").is_empty());
        // data 非 JSON
        assert!(parser.feed(b"event: token\ndata: not-json\n\n").is_empty());
        // 后续合法帧不受影响
        let frames = parser.feed(b"event: done\ndata: {\"ok\":true}\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].event, "done");
    }

    #[test]
    fn joins_multiple_data_lines() {
        let mut parser = SseParser::default();
        let frames = parser.feed(b"event: msg\ndata: {\"a\":\ndata: 1}\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].data, serde_json::json!({ "a": 1 }));
    }

    #[test]
    fn parses_crlf_separated_frames() {
        // BE-m7：SSE 规范允许 \r\n 行尾，\r\n\r\n 规范化为 \n\n 后切帧
        let mut parser = SseParser::default();
        let frames = parser.feed(b"event: token\r\ndata: {\"content\":\"ok\"}\r\n\r\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].event, "token");
        assert_eq!(frames[0].data, serde_json::json!({ "content": "ok" }));
    }

    #[test]
    fn parses_mixed_line_endings() {
        // 混合行尾（CRLF + LF）均按行分隔处理
        let mut parser = SseParser::default();
        let frames = parser.feed(b"event: msg\r\ndata: {\"a\":\ndata: 1}\r\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].event, "msg");
        assert_eq!(frames[0].data, serde_json::json!({ "a": 1 }));
    }

    #[test]
    fn lone_cr_treated_as_line_break() {
        // 孤立 \r（SSE 规范允许的行尾之一）同样作为行分隔
        let mut parser = SseParser::default();
        let frames = parser.feed(b"event: done\rdata: {\"ok\":true}\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].event, "done");
    }

    #[test]
    fn crlf_separator_split_across_chunks() {
        // \r\n 跨块分割：前块末尾孤立 \r 保留，后块首字节 \n 与其合并为 \n
        let mut parser = SseParser::default();
        assert!(parser
            .feed(b"event: done\ndata: {\"ok\":true}\r")
            .is_empty());
        let frames = parser.feed(b"\n\r\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].event, "done");
    }

    #[test]
    fn oversized_buffer_without_separator_is_dropped() {
        // BE-m7：垃圾 + 半帧超限后缓冲清空——再喂帧尾时，若缓冲未清会拼出完整帧；
        // 已清空则帧尾独立无 event 行、不出帧
        let mut parser = SseParser::default();
        let junk = vec![b'x'; MAX_BUFFER_LEN - 10];
        assert!(parser.feed(&junk).is_empty());
        // 追加带换行的半帧（event/data 行完整但无帧分隔符）→ 总长超限触发清空
        assert!(parser
            .feed(b"\nevent: done\ndata: {\"ok\":true}")
            .is_empty());
        // 帧尾补分隔符：清空后半帧不存在，不应拼出帧
        assert!(parser.feed(b"\n\n").is_empty());
        // 后续完整帧正常解析（丢弃不影响新帧）
        let frames = parser.feed(b"event: done\ndata: {\"ok\":true}\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].event, "done");
    }

    #[test]
    fn empty_chunk_yields_nothing() {
        let mut parser = SseParser::default();
        assert!(parser.feed(b"").is_empty());
    }
}
