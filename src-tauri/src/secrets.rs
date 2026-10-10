use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use subtle::ConstantTimeEq;
use uuid::Uuid;
use zeroize::Zeroize;

pub const INSTANCE_GATEWAY_KEY_REF_PREFIX: &str = "instance-gateway-key:";
/// Legacy binding subject retained only for retiring old sidecar state.
pub const DEFAULT_APP_COMPATIBILITY_ID: &str = "default-app-compatibility";

/// A secret API key. It intentionally implements neither `Debug` nor `Serialize`.
pub struct ApiKey(String);

impl ApiKey {
    pub fn expose(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn for_test(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl Drop for ApiKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Per-instance local Gateway admission secret. It is never serialized or
/// returned through IPC and is distinct from every upstream credential.
pub struct InstanceGatewayKey(String);

impl InstanceGatewayKey {
    pub fn generate() -> Self {
        Self(format!(
            "3rg_{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        ))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn for_test(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl Drop for InstanceGatewayKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

pub fn instance_gateway_key_ref(instance_id: &str) -> Result<String, String> {
    if instance_id.trim() != instance_id
        || instance_id.is_empty()
        || instance_id.len() > 160
        || instance_id.chars().any(char::is_control)
    {
        return Err("invalid_instance_id".to_string());
    }
    Ok(format!("{INSTANCE_GATEWAY_KEY_REF_PREFIX}{instance_id}"))
}

pub fn instance_gateway_key_hash(key: &InstanceGatewayKey) -> String {
    let digest = Sha256::digest(key.expose().as_bytes());
    format!("sha256:{}", encode_hex(&digest))
}

pub fn instance_gateway_key_matches(presented: &str, expected_hash: &str) -> bool {
    let Some(expected) = expected_hash
        .strip_prefix("sha256:")
        .and_then(decode_sha256)
    else {
        return false;
    };
    let actual = Sha256::digest(presented.as_bytes());
    bool::from(actual.as_slice().ct_eq(&expected))
}

pub fn validate_api_key(value: String) -> Result<ApiKey, String> {
    if value.trim().is_empty() || value.trim() != value || value.chars().any(char::is_control) {
        return Err("请输入有效的 API Key；不得包含首尾空格或控制字符。".to_string());
    }
    Ok(ApiKey(value))
}

pub trait ApiKeyVault: Send + Sync {
    fn load(&self, secret_ref: &str) -> Result<Option<ApiKey>, String>;
    fn save(&self, secret_ref: &str, api_key: &ApiKey) -> Result<(), String>;
    fn clear(&self, secret_ref: &str) -> Result<(), String>;
}

/// SQLite-backed provider credential store.
///
/// The control database is private application data and is the only runtime
/// source for managed Provider API keys. No operating-system credential
/// fallback exists.
pub struct DatabaseApiKeyVault {
    connection: Arc<Mutex<Connection>>,
}

impl DatabaseApiKeyVault {
    pub(crate) fn new(connection: Arc<Mutex<Connection>>) -> Self {
        Self { connection }
    }
}

impl ApiKeyVault for DatabaseApiKeyVault {
    fn load(&self, secret_ref: &str) -> Result<Option<ApiKey>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "凭据数据库锁已损坏。".to_string())?;
        let value = connection
            .query_row(
                "SELECT api_key FROM provider_credentials WHERE secret_ref=?1",
                [secret_ref],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("无法读取 Provider API Key：{error}"))?;
        Ok(value.map(ApiKey))
    }

    fn save(&self, secret_ref: &str, api_key: &ApiKey) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "凭据数据库锁已损坏。".to_string())?;
        connection
            .execute(
                "INSERT INTO provider_credentials(secret_ref, api_key, updated_at)
                 VALUES (?1, ?2, unixepoch())
                 ON CONFLICT(secret_ref) DO UPDATE SET
                   api_key=excluded.api_key, updated_at=excluded.updated_at",
                params![secret_ref, api_key.expose()],
            )
            .map_err(|error| format!("无法保存 Provider API Key：{error}"))?;
        Ok(())
    }

    fn clear(&self, secret_ref: &str) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "凭据数据库锁已损坏。".to_string())?;
        connection
            .execute(
                "DELETE FROM provider_credentials WHERE secret_ref=?1",
                [secret_ref],
            )
            .map_err(|error| format!("无法删除 Provider API Key：{error}"))?;
        Ok(())
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn decode_sha256(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut decoded = [0_u8; 32];
    for (index, byte) in decoded.iter_mut().enumerate() {
        let offset = index * 2;
        *byte = u8::from_str_radix(&value[offset..offset + 2], 16).ok()?;
    }
    Some(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use std::{collections::HashMap, sync::Mutex};

    #[derive(Default)]
    struct MemoryVault(Mutex<HashMap<String, String>>);

    impl ApiKeyVault for MemoryVault {
        fn load(&self, secret_ref: &str) -> Result<Option<ApiKey>, String> {
            Ok(self.0.lock().unwrap().get(secret_ref).cloned().map(ApiKey))
        }
        fn save(&self, secret_ref: &str, api_key: &ApiKey) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .insert(secret_ref.to_string(), api_key.expose().to_string());
            Ok(())
        }
        fn clear(&self, secret_ref: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(secret_ref);
            Ok(())
        }
    }

    #[test]
    fn vault_separates_keys_by_secret_reference() {
        let vault = MemoryVault::default();
        vault
            .save("provider:a", &validate_api_key("sk-a".to_string()).unwrap())
            .unwrap();
        vault
            .save("provider:b", &validate_api_key("sk-b".to_string()).unwrap())
            .unwrap();
        assert_eq!(vault.load("provider:a").unwrap().unwrap().expose(), "sk-a");
        assert_eq!(vault.load("provider:b").unwrap().unwrap().expose(), "sk-b");
    }

    #[test]
    fn database_vault_persists_and_clears_provider_keys() {
        let database = Database::in_memory();
        let vault = DatabaseApiKeyVault::new(database.connection());
        let key = validate_api_key("sk-database".to_string()).unwrap();
        vault.save("provider:database", &key).unwrap();
        assert_eq!(
            vault.load("provider:database").unwrap().unwrap().expose(),
            "sk-database"
        );
        vault.clear("provider:database").unwrap();
        assert!(vault.load("provider:database").unwrap().is_none());
    }

    #[test]
    fn blank_or_whitespace_padded_keys_are_rejected() {
        assert!(validate_api_key("   ".to_string()).is_err());
        assert!(validate_api_key(" sk-test".to_string()).is_err());
    }

    #[test]
    fn instance_keys_have_scoped_references_and_constant_time_hash_checks() {
        let first = InstanceGatewayKey::generate();
        let second = InstanceGatewayKey::generate();
        assert_ne!(first.expose(), second.expose());
        assert!(first.expose().starts_with("3rg_"));
        assert_eq!(first.expose().len(), 68);
        assert_eq!(
            instance_gateway_key_ref("instance-a").unwrap(),
            "instance-gateway-key:instance-a"
        );

        let known = InstanceGatewayKey::for_test("3rg_known");
        let digest = instance_gateway_key_hash(&known);
        assert!(instance_gateway_key_matches("3rg_known", &digest));
        assert!(!instance_gateway_key_matches("3rg_other", &digest));
        assert!(!instance_gateway_key_matches("3rg_known", "not-a-digest"));
    }
}
