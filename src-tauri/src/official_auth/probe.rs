//! 探测目录和 app-server schema 校验。
//!
//! 不启动长期会话。由 Adapter.probe 调用。

use serde_json::Value;
use std::{
    env, fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

use super::protocol::{io_error, OfficialAuthError};

pub(crate) struct ProbeDirectory {
    root: PathBuf,
    codex_home: PathBuf,
    schema_dir: PathBuf,
}

impl ProbeDirectory {
    pub(crate) fn create() -> Result<Self, OfficialAuthError> {
        let root = env::temp_dir().join(format!(
            "rcodex-codex-app-server-probe-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        let codex_home = root.join("codex-home");
        let schema_dir = root.join("schema");
        fs::create_dir_all(&codex_home).map_err(|source| io_error("create_probe_home", source))?;
        fs::create_dir_all(&schema_dir)
            .map_err(|source| io_error("create_schema_directory", source))?;
        set_owner_only_permissions(&root)?;
        set_owner_only_permissions(&codex_home)?;
        set_owner_only_permissions(&schema_dir)?;
        Ok(Self {
            root,
            codex_home,
            schema_dir,
        })
    }

    pub(crate) fn codex_home(&self) -> &Path {
        &self.codex_home
    }

    pub(crate) fn schema_dir(&self) -> &Path {
        &self.schema_dir
    }
}

impl Drop for ProbeDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[cfg(unix)]
fn set_owner_only_permissions(path: &Path) -> Result<(), OfficialAuthError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|source| io_error("set_probe_permissions", source))
}

#[cfg(not(unix))]
fn set_owner_only_permissions(_path: &Path) -> Result<(), OfficialAuthError> {
    Ok(())
}

pub(crate) fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

pub(crate) struct SchemaDocuments {
    pub(crate) client_request: Value,
    pub(crate) client_notification: Value,
    pub(crate) server_notification: Value,
    pub(crate) initialize_params: Value,
    pub(crate) initialize_response: Value,
    pub(crate) login_params: Value,
    pub(crate) login_response: Value,
    pub(crate) account_response: Value,
    pub(crate) cancel_params: Value,
    pub(crate) cancel_response: Value,
    pub(crate) logout_response: Value,
    pub(crate) login_completed: Value,
    pub(crate) account_updated: Value,
}

impl SchemaDocuments {
    pub(crate) fn load(root: &Path) -> Result<Self, OfficialAuthError> {
        Ok(Self {
            client_request: read_schema(root.join("ClientRequest.json"))?,
            client_notification: read_schema(root.join("ClientNotification.json"))?,
            server_notification: read_schema(root.join("ServerNotification.json"))?,
            initialize_params: read_schema(root.join("v1/InitializeParams.json"))?,
            initialize_response: read_schema(root.join("v1/InitializeResponse.json"))?,
            login_params: read_schema(root.join("v2/LoginAccountParams.json"))?,
            login_response: read_schema(root.join("v2/LoginAccountResponse.json"))?,
            account_response: read_schema(root.join("v2/GetAccountResponse.json"))?,
            cancel_params: read_schema(root.join("v2/CancelLoginAccountParams.json"))?,
            cancel_response: read_schema(root.join("v2/CancelLoginAccountResponse.json"))?,
            logout_response: read_schema(root.join("v2/LogoutAccountResponse.json"))?,
            login_completed: read_schema(root.join("v2/AccountLoginCompletedNotification.json"))?,
            account_updated: read_schema(root.join("v2/AccountUpdatedNotification.json"))?,
        })
    }
}

fn read_schema(path: PathBuf) -> Result<Value, OfficialAuthError> {
    let bytes = fs::read(path).map_err(|source| io_error("read_protocol_schema", source))?;
    serde_json::from_slice(&bytes).map_err(|_| OfficialAuthError::ProtocolIncompatible {
        reason: "schema is not valid JSON".to_owned(),
    })
}

pub(crate) struct SchemaCapabilities {
    pub(crate) supports_rate_limits: bool,
}

pub(crate) fn validate_auth_schema(
    documents: &SchemaDocuments,
) -> Result<SchemaCapabilities, OfficialAuthError> {
    require_method(&documents.client_request, "initialize")?;
    require_method(&documents.client_notification, "initialized")?;
    for method in [
        "account/read",
        "account/login/start",
        "account/login/cancel",
        "account/logout",
    ] {
        require_method(&documents.client_request, method)?;
    }
    for method in ["account/login/completed", "account/updated"] {
        require_method(&documents.server_notification, method)?;
    }

    require_fields(&documents.initialize_params, &["clientInfo"])?;
    require_fields(
        &documents.initialize_response,
        &["codexHome", "userAgent", "platformFamily", "platformOs"],
    )?;
    require_fields(&documents.cancel_params, &["loginId"])?;
    require_fields(&documents.cancel_response, &["status"])?;
    require_object_schema(&documents.logout_response, "logout response")?;
    require_fields(&documents.login_completed, &["success"])?;
    require_properties(&documents.login_completed, &["loginId", "error"])?;
    require_properties(&documents.account_updated, &["authMode", "planType"])?;
    require_fields(&documents.account_response, &["requiresOpenaiAuth"])?;
    require_properties(&documents.account_response, &["account"])?;

    let browser_params = require_discriminator_variant(&documents.login_params, "chatgpt")?;
    require_fields(browser_params, &["type"])?;
    let device_params =
        require_discriminator_variant(&documents.login_params, "chatgptDeviceCode")?;
    require_fields(device_params, &["type"])?;

    let browser_response = require_discriminator_variant(&documents.login_response, "chatgpt")?;
    require_fields(browser_response, &["type", "loginId", "authUrl"])?;
    let device_response =
        require_discriminator_variant(&documents.login_response, "chatgptDeviceCode")?;
    require_fields(
        device_response,
        &["type", "loginId", "verificationUrl", "userCode"],
    )?;

    let account_definition = documents
        .account_response
        .pointer("/definitions/Account")
        .ok_or_else(|| incompatible("account definition is missing"))?;
    let chatgpt_account = require_discriminator_variant(account_definition, "chatgpt")?;
    require_fields(chatgpt_account, &["type", "email", "planType"])?;

    Ok(SchemaCapabilities {
        supports_rate_limits: has_method(&documents.client_request, "account/rateLimits/read"),
    })
}

fn has_method(schema: &Value, method: &str) -> bool {
    schema
        .get("oneOf")
        .and_then(Value::as_array)
        .map(|variants| {
            variants.iter().any(|variant| {
                variant
                    .pointer("/properties/method/enum")
                    .and_then(Value::as_array)
                    .map(|values| values.iter().any(|value| value.as_str() == Some(method)))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn require_method(schema: &Value, method: &str) -> Result<(), OfficialAuthError> {
    if has_method(schema, method) {
        Ok(())
    } else {
        Err(incompatible(&format!(
            "required method {method} is missing"
        )))
    }
}

fn require_discriminator_variant<'a>(
    schema: &'a Value,
    discriminator: &str,
) -> Result<&'a Value, OfficialAuthError> {
    schema
        .get("oneOf")
        .and_then(Value::as_array)
        .and_then(|variants| {
            variants.iter().find(|variant| {
                variant
                    .pointer("/properties/type/enum")
                    .and_then(Value::as_array)
                    .map(|values| {
                        values
                            .iter()
                            .any(|value| value.as_str() == Some(discriminator))
                    })
                    .unwrap_or(false)
            })
        })
        .ok_or_else(|| {
            incompatible(&format!(
                "required discriminator {discriminator} is missing"
            ))
        })
}

fn require_object_schema(schema: &Value, name: &str) -> Result<(), OfficialAuthError> {
    if schema.get("type").and_then(Value::as_str) == Some("object") {
        Ok(())
    } else {
        Err(incompatible(&format!("{name} is not an object")))
    }
}

fn require_fields(schema: &Value, fields: &[&str]) -> Result<(), OfficialAuthError> {
    require_properties(schema, fields)?;
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .ok_or_else(|| incompatible("required field list is missing"))?;
    for field in fields {
        if !required.iter().any(|value| value.as_str() == Some(field)) {
            return Err(incompatible(&format!("required field {field} is missing")));
        }
    }
    Ok(())
}

fn require_properties(schema: &Value, fields: &[&str]) -> Result<(), OfficialAuthError> {
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| incompatible("schema properties are missing"))?;
    for field in fields {
        if !properties.contains_key(*field) {
            return Err(incompatible(&format!("schema property {field} is missing")));
        }
    }
    Ok(())
}

fn incompatible(reason: &str) -> OfficialAuthError {
    OfficialAuthError::ProtocolIncompatible {
        reason: reason.to_owned(),
    }
}
