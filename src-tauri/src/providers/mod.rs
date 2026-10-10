//! API Key Provider 存储：渠道 CRUD、修订和上游凭据解析。
//!
//! 不探测连通性、不记用量、不读系统钥匙串；凭据只从本机 SQLite + vault 按修订读取。
//! 由 `commands/providers`、Gateway 启动装配和受管理配置投影调用。

mod models;
mod operations;
mod store;
mod upstream;

#[cfg(test)]
mod tests;

pub use models::{
    CreateProviderInput, GatewayUpstream, ProviderSummary, TransportMode, UpdateProviderInput,
};
pub use store::ProviderStore;

pub(crate) use operations::{normalize_api_base_url, validate_base_url};

#[cfg(test)]
#[allow(unused_imports)]
use crate::database::Database;
#[cfg(test)]
#[allow(unused_imports)]
use crate::secrets::{ApiKey, ApiKeyVault};
#[cfg(test)]
#[allow(unused_imports)]
use crate::support::unix_now_seconds;
#[cfg(test)]
#[allow(unused_imports)]
pub use models::ProviderGatewayTimeoutInput;
#[cfg(test)]
#[allow(unused_imports)]
use models::{ProviderRecord, OPENAI_BASE_URL};
#[cfg(test)]
#[allow(unused_imports)]
use operations::{provider_operation_resources, revision_secret_ref};
#[cfg(test)]
#[allow(unused_imports)]
use rusqlite::params;
#[cfg(test)]
#[allow(unused_imports)]
use std::sync::Arc;
#[cfg(test)]
#[allow(unused_imports)]
use uuid::Uuid;
