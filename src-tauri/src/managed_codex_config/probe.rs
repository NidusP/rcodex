//! 用本机 Codex 校验隔离目录和 profile。
//!
//! 普通诊断只记形状、计数和脱敏摘要，不写正文或 Key。失败不改已物化文件。

use std::{
    collections::BTreeSet,
    path::Path,
    process::{Command, Output},
};

use serde_json::Value as JsonValue;

use crate::launcher_protocol::append_startup_log;

use super::io::{sha256_label, validate_profile_name};
use super::{INSTANCE_GATEWAY_ENV, MAX_BUNDLED_CATALOG_BYTES};

const MAX_PROBE_DIAGNOSTIC_BYTES: usize = 4 * 1024;
const MAX_PROBE_DIAGNOSTIC_CHARS: usize = 768;

pub fn verify_catalog_models_with_codex(
    codex_executable: &Path,
    catalog_path: &Path,
    expected_public_models: &[String],
    startup_log_path: &Path,
) -> Result<(), String> {
    let codex_home = catalog_path.parent().ok_or_else(|| {
        append_preflight_log(
            startup_log_path,
            "catalog_probe result=failed stage=catalog_parent_missing",
        );
        "codex_catalog_validation_failed".to_string()
    })?;
    let path_value = toml::Value::String(catalog_path.display().to_string()).to_string();
    let output = Command::new(codex_executable)
        .args([
            "-c",
            &format!("model_catalog_json={path_value}"),
            "debug",
            "models",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_API_KEY")
        .env_remove("CODEX_ACCESS_TOKEN")
        .env_remove(INSTANCE_GATEWAY_ENV)
        .env("CODEX_HOME", codex_home)
        .env("CODEX_SQLITE_HOME", codex_home)
        .output()
        .map_err(|error| {
            append_preflight_log(
                startup_log_path,
                &format!(
                    "catalog_probe result=failed stage=spawn error_summary={}",
                    sanitize_probe_summary(error.to_string().as_bytes())
                ),
            );
            format!("codex_catalog_validation_failed:{error}")
        })?;
    if !output.status.success() {
        append_probe_output_log(startup_log_path, "catalog_probe", "command_exit", &output);
        return Err("codex_catalog_validation_failed".to_string());
    }
    let catalog = serde_json::from_slice::<JsonValue>(&output.stdout).map_err(|_| {
        append_probe_output_log(
            startup_log_path,
            "catalog_probe",
            "stdout_not_json",
            &output,
        );
        "codex_catalog_validation_failed".to_string()
    })?;
    let slugs = catalog
        .get("models")
        .and_then(JsonValue::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|model| model.get("slug").and_then(JsonValue::as_str))
                .collect::<BTreeSet<_>>()
        })
        .ok_or_else(|| {
            append_probe_output_log(
                startup_log_path,
                "catalog_probe",
                "models_field_missing",
                &output,
            );
            "codex_catalog_validation_failed".to_string()
        })?;
    if expected_public_models
        .iter()
        .any(|expected| !slugs.contains(expected.as_str()))
    {
        append_preflight_log(
            startup_log_path,
            &format!(
                "catalog_probe result=failed stage=expected_models_missing expected_models={} loaded_models={}",
                expected_public_models.len(),
                slugs.len(),
            ),
        );
        return Err("codex_catalog_models_missing".to_string());
    }
    append_preflight_log(
        startup_log_path,
        &format!(
            "catalog_probe result=success expected_models={} loaded_models={}",
            expected_public_models.len(),
            slugs.len(),
        ),
    );
    Ok(())
}

pub fn verify_managed_profile_with_codex(
    codex_executable: &Path,
    codex_home: &Path,
    profile_name: &str,
    _expected_public_models: &[String],
    startup_log_path: &Path,
) -> Result<(), String> {
    validate_profile_name(profile_name)?;
    let output = Command::new(codex_executable)
        .args([
            "--profile",
            profile_name,
            "debug",
            "prompt-input",
            "R Codex managed profile probe",
        ])
        .env("CODEX_HOME", codex_home)
        .env("CODEX_SQLITE_HOME", codex_home)
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_API_KEY")
        .env_remove("CODEX_ACCESS_TOKEN")
        .output()
        .map_err(|error| {
            append_preflight_log(
                startup_log_path,
                &format!(
                    "profile_probe result=failed stage=spawn error_summary={}",
                    sanitize_probe_summary(error.to_string().as_bytes())
                ),
            );
            format!("codex_profile_probe_failed:{error}")
        })?;
    if !output.status.success() {
        append_probe_output_log(startup_log_path, "profile_probe", "command_exit", &output);
        return Err("codex_profile_probe_failed".to_string());
    }
    if output.stdout.len() > MAX_BUNDLED_CATALOG_BYTES {
        append_probe_output_log(
            startup_log_path,
            "profile_probe",
            "stdout_too_large",
            &output,
        );
        return Err("codex_profile_probe_failed".to_string());
    }
    append_preflight_log(
        startup_log_path,
        &format!(
            "profile_probe result=success stdout_bytes={} stderr_bytes={}",
            output.stdout.len(),
            output.stderr.len(),
        ),
    );
    Ok(())
}

fn append_probe_output_log(log_path: &Path, probe: &str, stage: &str, output: &Output) {
    append_preflight_log(
        log_path,
        &format!(
            "{probe} result=failed stage={stage} status={} stdout_bytes={} stdout_sha256={} stderr_bytes={} stderr_sha256={} stderr_summary={}",
            probe_exit_status(output),
            output.stdout.len(),
            sha256_label(&output.stdout),
            output.stderr.len(),
            sha256_label(&output.stderr),
            sanitize_probe_summary(&output.stderr),
        ),
    );
}

fn append_preflight_log(log_path: &Path, event: &str) {
    let _ = append_startup_log(log_path, event);
}

fn probe_exit_status(output: &Output) -> String {
    output
        .status
        .code()
        .map(|code| format!("exit_{code}"))
        .unwrap_or_else(|| "signal".to_string())
}

pub(crate) fn sanitize_probe_summary(bytes: &[u8]) -> String {
    let captured = &bytes[..bytes.len().min(MAX_PROBE_DIAGNOSTIC_BYTES)];
    let mut safe = String::from_utf8_lossy(captured)
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    safe = redact_urls(&safe);
    let lower = safe.to_ascii_lowercase();
    if let Some(index) = [
        "bearer ",
        "authorization:",
        "authorization=",
        "x-api-key:",
        "x-api-key=",
        "openai_api_key=",
        "codex_api_key=",
        "codex_access_token=",
        "threer_instance_key=",
        "api_key=",
        "api-key=",
        "\"api_key\"",
        "\"api-key\"",
    ]
    .iter()
    .filter_map(|marker| lower.find(marker))
    .min()
    {
        safe.truncate(index);
        safe.push_str("[redacted]");
    }
    let mut summary = safe
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_PROBE_DIAGNOSTIC_CHARS)
        .collect::<String>();
    if bytes.len() > captured.len() {
        summary.push_str(" [truncated]");
    }
    if summary.is_empty() {
        "[empty]".to_string()
    } else {
        summary
    }
}

fn redact_urls(value: &str) -> String {
    let mut remaining = value;
    let mut safe = String::with_capacity(value.len());
    while let Some(start) = next_url_start(remaining) {
        safe.push_str(&remaining[..start]);
        let url = &remaining[start..];
        let end = url
            .char_indices()
            .find(|(_, character)| {
                character.is_whitespace()
                    || matches!(
                        *character,
                        '"' | '\'' | '<' | '>' | '(' | ')' | '[' | ']' | ','
                    )
            })
            .map(|(index, _)| index)
            .unwrap_or(url.len());
        safe.push_str("[url-redacted]");
        remaining = &url[end..];
    }
    safe.push_str(remaining);
    safe
}

fn next_url_start(value: &str) -> Option<usize> {
    let lower = value.to_ascii_lowercase();
    [lower.find("https://"), lower.find("http://")]
        .into_iter()
        .flatten()
        .min()
}
