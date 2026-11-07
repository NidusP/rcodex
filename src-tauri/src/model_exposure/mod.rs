//! 用户模型透出偏好和按配置隔离的图片探针证据。
//!
//! 不发送网络请求，不读取凭据。空间写入和启动可在原事务里调用只读校验。

mod models;
mod store;

pub(crate) use models::{ExposureChange, ModelExposureSummary, ProbeContext};
pub(crate) use store::{assert_model_exposed, image_supported, ModelExposureStore};

/// Change whenever the fixture, request shape, or success criterion changes.
pub(crate) const IMAGE_PROBE_CONTRACT_VERSION: &str = "image-v1";

/// Match the product's ordinary conversation model filter, including image
/// generator aliases. This only controls exposure; it never infers vision.
pub(crate) fn is_selectable_model_id(model_id: &str) -> bool {
    let value = model_id.trim().to_ascii_lowercase();
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return false;
    }
    if crate::routing::is_internal_codex_model_id(&value.replace('_', "-")) {
        return false;
    }
    if ["imagine", "dall-e", "dalle", "gpt-image"]
        .iter()
        .any(|part| value.contains(part))
    {
        return false;
    }
    for (offset, _) in value.match_indices("imagen") {
        let before = value[..offset].chars().next_back();
        let after = value[offset + 6..].chars().next();
        if !before.is_some_and(|c| c.is_ascii_alphanumeric())
            && !after.is_some_and(|c| c.is_ascii_alphabetic())
        {
            return false;
        }
    }
    for (offset, _) in value.match_indices("image") {
        let before = value[..offset].chars().next_back();
        let suffix = &value[offset + 5..];
        if !before.is_some_and(|c| c.is_ascii_alphanumeric())
            && (suffix.is_empty()
                || suffix
                    .strip_prefix('-')
                    .and_then(|tail| tail.chars().next())
                    .is_some_and(|c| c.is_ascii_digit()))
        {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests;
