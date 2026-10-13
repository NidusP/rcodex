//! One-shot recovery when an upstream rejects image input.
//!
//! Images are sent as usual. A 400 whose text says this model cannot see images
//! is not a catalog update and not a channel change. The image blocks in that
//! same request are replaced with a text notice and the request is sent once
//! more. Any other failure stays a failure.

use serde_json::{json, Map, Value};

pub(super) const IMAGE_UNAVAILABLE_NOTICE: &str = "这里有一张图，当前模型看不到";

pub(super) fn upstream_rejected_image(status: u16, detail: Option<&str>) -> bool {
    if status != 400 {
        return false;
    }
    let Some(detail) = detail.map(str::trim).filter(|text| !text.is_empty()) else {
        return false;
    };
    let message = detail.to_ascii_lowercase();
    const HINTS: &[&str] = &[
        "not a vlm",
        "vision language",
        "text-only",
        "text only",
        "only support text",
        "only supports text",
        "does not support image",
        "doesn't support image",
        "do not support image",
        "don't support image",
        "not support image",
        "unsupported image",
        "不支持图片",
        "不支持图像",
        "仅支持文本",
        "只支持文本",
    ];
    HINTS.iter().any(|hint| message.contains(hint))
}

pub(super) fn replace_unsupported_images(value: &mut Value) -> usize {
    match value {
        Value::Array(items) => items.iter_mut().map(replace_unsupported_images).sum(),
        Value::Object(object) => {
            if let Some(text_type) = image_block_text_type(object) {
                *value = json!({
                    "type": text_type,
                    "text": IMAGE_UNAVAILABLE_NOTICE,
                });
                1
            } else {
                object.values_mut().map(replace_unsupported_images).sum()
            }
        }
        _ => 0,
    }
}

fn image_block_text_type(object: &Map<String, Value>) -> Option<&'static str> {
    match object.get("type").and_then(Value::as_str) {
        Some("input_image") => Some("input_text"),
        Some("image_url") if object.get("image_url").is_some() => Some("text"),
        Some("image") if object.get("source").is_some() || object.get("image_url").is_some() => {
            Some("text")
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn siliconflow_vlm_rejection_matches_and_balance_does_not() {
        let detail =
            "The model is not a VLM (Vision Language Model). Please use text-only prompts.";
        assert!(upstream_rejected_image(400, Some(detail)));
        assert!(!upstream_rejected_image(400, Some("Insufficient Balance")));
        assert!(!upstream_rejected_image(402, Some(detail)));
        assert!(!upstream_rejected_image(400, Some("Invalid API key")));
    }

    #[test]
    fn image_blocks_become_text_and_other_fields_stay() {
        let mut body = json!({
            "model": "deepseek-ai/DeepSeek-V4-Flash",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "look"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,aaaa", "detail": "high"}}
                ]
            }],
            "input": [{
                "type": "message",
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "look"},
                    {"type": "input_image", "image_url": "data:image/png;base64,bbbb"}
                ]
            }]
        });
        assert_eq!(replace_unsupported_images(&mut body), 2);
        assert_eq!(body["messages"][0]["content"][0]["text"], "look");
        assert_eq!(body["messages"][0]["content"][1]["type"], "text");
        assert_eq!(
            body["messages"][0]["content"][1]["text"],
            IMAGE_UNAVAILABLE_NOTICE
        );
        assert!(body["messages"][0]["content"][1].get("image_url").is_none());
        assert_eq!(body["input"][0]["content"][1]["type"], "input_text");
        assert_eq!(body["model"], "deepseek-ai/DeepSeek-V4-Flash");
        assert_eq!(replace_unsupported_images(&mut body), 0);
    }
}
