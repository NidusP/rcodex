//! Bounded, one-shot image-understanding checks for a provider's confirmed wire API.
//!
//! No persistence, provider discovery, retries, or logging belongs here. The caller
//! binds the result to a provider revision and owns scheduling and cached evidence.

use std::time::Duration;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use reqwest::{Client, Response};
use serde_json::{json, Value};
use url::Url;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const PROBE_TIMEOUT: Duration = Duration::from_secs(25);
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
// Includes reasoning tokens on Responses models, while the requested answer is
// only two words. A tiny 16-token budget often never reaches visible output.
const MAX_OUTPUT_TOKENS: usize = 512;
const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/model-image-probe.png");
const PROMPT: &str = "Identify the two large colored regions in the attached image, from left to right. Reply with exactly two lowercase English color words separated by a comma. No explanation.";

/// A safe capability verdict; never contains upstream text, URLs, or credentials.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageProbeResult {
    pub state: String,
    pub error_code: Option<String>,
    pub http_status: Option<u16>,
}

impl ImageProbeResult {
    fn failed(code: &str, http_status: Option<u16>) -> Self {
        Self {
            state: "failed".to_string(),
            error_code: Some(code.to_string()),
            http_status,
        }
    }

    fn unsupported(http_status: Option<u16>) -> Self {
        Self {
            state: "unsupported".to_string(),
            error_code: Some("image_probe_unsupported".to_string()),
            http_status,
        }
    }
}

/// Check precisely one model and one confirmed protocol, without path fallback.
pub async fn execute(
    base_url: &str,
    api_key: &str,
    upstream_model_id: &str,
    wire_api: &str,
) -> ImageProbeResult {
    let client = match Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(PROBE_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(_) => return ImageProbeResult::failed("image_probe_client_failed", None),
    };
    execute_with_client(&client, base_url, api_key, upstream_model_id, wire_api).await
}

async fn execute_with_client(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    wire_api: &str,
) -> ImageProbeResult {
    let route = match wire_api {
        "responses" => "/responses",
        "chat_completions" => "/chat/completions",
        _ => return ImageProbeResult::failed("image_probe_protocol_unconfirmed", None),
    };
    let url = match confirmed_endpoint(base_url, route) {
        Some(url) => url,
        None => return ImageProbeResult::failed("image_probe_invalid_base_url", None),
    };
    if model.trim().is_empty() || api_key.trim().is_empty() {
        return ImageProbeResult::failed("image_probe_invalid_input", None);
    }
    let response = match client
        .post(url)
        .bearer_auth(api_key)
        .json(&request_body(model, wire_api))
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => return transport_failure(&error, None),
    };
    let status = response.status().as_u16();
    let body = match bounded_body(response).await {
        Ok(body) => body,
        Err(result) => return result,
    };
    classify_response(status, &body, wire_api)
}

fn confirmed_endpoint(base_url: &str, route: &str) -> Option<Url> {
    let base = Url::parse(base_url).ok()?;
    if !matches!(base.scheme(), "http" | "https")
        || base.host_str().is_none()
        || !base.username().is_empty()
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
    {
        return None;
    }
    Url::parse(&format!("{}{route}", base_url.trim_end_matches('/'))).ok()
}

fn request_body(model: &str, wire_api: &str) -> Value {
    let image = format!("data:image/png;base64,{}", STANDARD.encode(FIXTURE));
    match wire_api {
        "responses" => json!({
            "model": model,
            "stream": false,
            "max_output_tokens": MAX_OUTPUT_TOKENS,
            "input": [{
                "type": "message",
                "role": "user",
                "content": [
                    {"type": "input_text", "text": PROMPT},
                    {"type": "input_image", "image_url": image, "detail": "auto"}
                ]
            }]
        }),
        _ => json!({
            "model": model,
            "stream": false,
            "max_tokens": MAX_OUTPUT_TOKENS,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": PROMPT},
                    {"type": "image_url", "image_url": {"url": image, "detail": "auto"}}
                ]
            }]
        }),
    }
}

async fn bounded_body(mut response: Response) -> Result<Vec<u8>, ImageProbeResult> {
    let status = Some(response.status().as_u16());
    if response
        .content_length()
        .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
    {
        return Err(ImageProbeResult::failed(
            "image_probe_response_too_large",
            status,
        ));
    }
    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                    return Err(ImageProbeResult::failed(
                        "image_probe_response_too_large",
                        status,
                    ));
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => return Ok(body),
            Err(error) => return Err(transport_failure(&error, status)),
        }
    }
}

fn transport_failure(error: &reqwest::Error, status: Option<u16>) -> ImageProbeResult {
    ImageProbeResult::failed(
        if error.is_timeout() {
            "image_probe_timeout"
        } else {
            "image_probe_transport_failed"
        },
        status,
    )
}

fn classify_response(status: u16, body: &[u8], wire_api: &str) -> ImageProbeResult {
    // Infrastructure and authorization failures never establish a capability.
    if status == 401 || status == 403 {
        return ImageProbeResult::failed("image_probe_unauthorized", Some(status));
    }
    if status == 429 {
        return ImageProbeResult::failed("image_probe_rate_limited", Some(status));
    }
    if status >= 500 {
        return ImageProbeResult::failed("image_probe_upstream_failed", Some(status));
    }
    let parsed = serde_json::from_slice::<Value>(body).ok();
    if !(200..300).contains(&status) {
        if matches!(status, 400 | 415 | 422) && parsed.as_ref().is_some_and(explicit_image_error) {
            return ImageProbeResult::unsupported(Some(status));
        }
        return ImageProbeResult::failed("image_probe_http_failed", Some(status));
    }
    let Some(value) = parsed else {
        return ImageProbeResult::failed("image_probe_invalid_response", Some(status));
    };
    if value.get("error").is_some_and(|error| !error.is_null()) {
        return if explicit_image_error(&value) {
            ImageProbeResult::unsupported(Some(status))
        } else {
            ImageProbeResult::failed("image_probe_upstream_failed", Some(status))
        };
    }
    // A truncated result is not positive evidence even if it starts correctly.
    if value
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| status != "completed")
        || value
            .pointer("/choices/0/finish_reason")
            .and_then(Value::as_str)
            .is_some_and(|reason| reason != "stop")
    {
        return ImageProbeResult::failed("image_probe_output_incomplete", Some(status));
    }
    let text = visible_text(&value, wire_api);
    if text.is_empty() {
        return ImageProbeResult::failed("image_probe_empty_response", Some(status));
    }
    if explicit_image_refusal(&text) {
        return ImageProbeResult::unsupported(Some(status));
    }
    let words: Vec<_> = text
        .split(|character: char| !character.is_ascii_alphabetic())
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    if words != ["blue", "yellow"] {
        return ImageProbeResult::failed("image_probe_answer_mismatch", Some(status));
    }
    ImageProbeResult {
        state: "supported".to_string(),
        error_code: None,
        http_status: Some(status),
    }
}

fn visible_text(value: &Value, wire_api: &str) -> String {
    if wire_api == "chat_completions" {
        return value
            .pointer("/choices/0/message/content")
            .map(content_text)
            .unwrap_or_default();
    }
    if let Some(output) = value.get("output").and_then(Value::as_array) {
        return output
            .iter()
            .filter(|item| item.get("type").and_then(Value::as_str) == Some("message"))
            .filter_map(|item| item.get("content"))
            .map(content_text)
            .collect::<Vec<_>>()
            .join("\n");
    }
    value
        .get("output_text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn content_text(content: &Value) -> String {
    if let Some(text) = content.as_str() {
        return text.to_string();
    }
    content
        .as_array()
        .map(|parts| {
            parts
                .iter()
                .filter(|part| {
                    matches!(
                        part.get("type").and_then(Value::as_str),
                        Some("output_text" | "text" | "refusal")
                    )
                })
                .filter_map(|part| {
                    part.get("text")
                        .or_else(|| part.get("refusal"))
                        .and_then(Value::as_str)
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn explicit_image_error(value: &Value) -> bool {
    let error = value.get("error").unwrap_or(value);
    let code = error
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(
        code.as_str(),
        "unsupported_image"
            | "unsupported_images"
            | "image_input_not_supported"
            | "vision_not_supported"
    ) {
        return true;
    }
    let message = error
        .as_str()
        .or_else(|| error.get("message").and_then(Value::as_str))
        .unwrap_or_default();
    let normalized = message.to_ascii_lowercase();
    explicit_image_refusal(message)
        // These statements are accepted only inside the upstream error envelope,
        // where they explain rejection of this image request. Mentioning a
        // text-only output in an ordinary assistant answer proves nothing.
        || [
            "this is a text-only model",
            "this is a text only model",
            "model is text-only",
            "model is text only",
        ]
        .iter()
        .any(|phrase| normalized.contains(phrase))
}

fn explicit_image_refusal(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    [
        "does not support image",
        "doesn't support image",
        "do not support image",
        "does not support vision",
        "image input is not supported",
        "image inputs are not supported",
        "images are not supported",
        "image_url is not supported",
        "image inputs not allowed",
        "cannot process images",
        "cannot view images",
        "can't view images",
        "unable to view images",
        "not a vlm",
        "not a vision-language model",
        "not a vision language model",
        "不支持图片",
        "不支持图像",
        "无法查看图片",
        "无法识别图片",
    ]
    .iter()
    .any(|phrase| text.contains(phrase))
}

#[cfg(test)]
mod tests;
