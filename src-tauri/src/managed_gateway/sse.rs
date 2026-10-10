//! 字节级 SSE 分帧：网络分块不是字符或事件边界，必须先拼成完整行再解码。
//! 此模块只处理协议边界，不读取文件、不记录正文，也不持有 Provider 凭据。

#[derive(Debug)]
pub(super) struct SseFrame {
    pub raw: Vec<u8>,
    pub data: Option<Vec<u8>>,
    pub event: Option<String>,
}

pub(super) struct SseDecoder {
    line: Vec<u8>,
    raw: Vec<u8>,
    data: Vec<u8>,
    has_data: bool,
    event: Option<String>,
    skip_lf: bool,
    max_frame: usize,
    frame_size: usize,
}

impl SseDecoder {
    pub fn new(max_frame: usize) -> Self {
        Self {
            line: Vec::new(),
            raw: Vec::new(),
            data: Vec::new(),
            has_data: false,
            event: None,
            skip_lf: false,
            max_frame,
            frame_size: 0,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseFrame>, &'static str> {
        let mut frames = Vec::new();
        for &byte in chunk {
            // CR 已结束上一行；紧随的 LF 属于同一个换行，即使位于下个网络块。
            if self.skip_lf {
                self.skip_lf = false;
                if byte == b'\n' {
                    continue;
                }
            }
            self.frame_size = self.frame_size.saturating_add(1);
            if self.frame_size > self.max_frame {
                return Err("upstream_sse_frame_too_large");
            }
            if byte == b'\r' || byte == b'\n' {
                self.skip_lf = byte == b'\r';
                if let Some(frame) = self.end_line()? {
                    frames.push(frame);
                }
            } else {
                self.line.push(byte);
            }
        }
        Ok(frames)
    }

    fn end_line(&mut self) -> Result<Option<SseFrame>, &'static str> {
        // 只在完整行上做 UTF-8 校验，中文/emoji 任意分块都不会被替换为乱码。
        std::str::from_utf8(&self.line).map_err(|_| "invalid_upstream_sse_utf8")?;
        self.raw.extend_from_slice(&self.line);
        self.raw.push(b'\n');
        if self.line.is_empty() {
            let frame = SseFrame {
                raw: std::mem::take(&mut self.raw),
                data: self.has_data.then(|| std::mem::take(&mut self.data)),
                event: self.event.take(),
            };
            self.has_data = false;
            self.frame_size = 0;
            return Ok(Some(frame));
        }
        let (name, value) = match self.line.iter().position(|byte| *byte == b':') {
            Some(index) => (&self.line[..index], &self.line[index + 1..]),
            None => (self.line.as_slice(), &[][..]),
        };
        let value = value.strip_prefix(b" ").unwrap_or(value);
        match name {
            b"data" => {
                if self.has_data {
                    self.data.push(b'\n');
                }
                self.data.extend_from_slice(value);
                self.has_data = true;
            }
            b"event" => self.event = Some(String::from_utf8(value.to_vec()).unwrap()),
            _ => {}
        }
        self.line.clear();
        Ok(None)
    }

    /// SSE 只在空行处分发事件。断开的尾部不能被补成一个成功终态。
    pub fn finish(&self) -> Result<(), &'static str> {
        if !self.line.is_empty() || self.has_data || self.event.is_some() {
            Err("upstream_truncated_sse_event")
        } else {
            Ok(())
        }
    }
}

impl SseFrame {
    /// 只替换 data 行，保留 event/id/retry/comment 等字段；多行 data 合并为合法 JSON 行。
    pub fn with_data(&self, value: &serde_json::Value) -> Vec<u8> {
        let mut output = Vec::new();
        let mut inserted = false;
        for line in self.raw.split(|byte| *byte == b'\n') {
            if line.is_empty() {
                continue;
            }
            if line == b"data" || line.starts_with(b"data:") {
                if !inserted {
                    output.extend_from_slice(b"data: ");
                    output.extend_from_slice(value.to_string().as_bytes());
                    output.push(b'\n');
                    inserted = true;
                }
            } else {
                output.extend_from_slice(line);
                output.push(b'\n');
            }
        }
        output.push(b'\n');
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_byte_chunk_and_all_line_endings_preserve_unicode_and_multiline_data() {
        for separator in ["\n", "\r", "\r\n"] {
            let input = format!("event: response.completed{separator}data: {{\"text\":{separator}data: \"中文🧭\"}}{separator}{separator}");
            let mut decoder = SseDecoder::new(1024);
            let frames = input
                .as_bytes()
                .chunks(1)
                .flat_map(|chunk| decoder.push(chunk).unwrap())
                .collect::<Vec<_>>();
            assert_eq!(frames.len(), 1);
            assert_eq!(frames[0].event.as_deref(), Some("response.completed"));
            let data: serde_json::Value =
                serde_json::from_slice(frames[0].data.as_ref().unwrap()).unwrap();
            assert_eq!(data["text"], "中文🧭");
            decoder.finish().unwrap();
        }
    }

    #[test]
    fn bounded_decoder_rejects_unterminated_large_frames_and_incomplete_utf8() {
        let mut decoder = SseDecoder::new(12);
        assert_eq!(
            decoder.push(b"data: 123456789").unwrap_err(),
            "upstream_sse_frame_too_large"
        );
        let mut decoder = SseDecoder::new(1024);
        decoder
            .push(b"data: {\"type\":\"response.completed\"}\n")
            .unwrap();
        assert_eq!(decoder.finish(), Err("upstream_truncated_sse_event"));
        let mut decoder = SseDecoder::new(1024);
        assert_eq!(
            decoder.push(b"data: \xff\n\n").unwrap_err(),
            "invalid_upstream_sse_utf8"
        );
    }
}
