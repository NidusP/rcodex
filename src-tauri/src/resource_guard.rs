use std::{collections::BTreeMap, fmt};

use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

use super::Database;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    LaunchPreset,
    ApiKeyProvider,
    ApiKeyProviderRevision,
    OfficialAccount,
    OfficialCredentialRevision,
    Gateway,
    GatewayRevision,
    Profile,
    ProfileRevision,
    RouteSetRevision,
    ModelRoute,
    ModelRouteIdentity,
    GatewayServiceRevision,
    ModelCatalogRevision,
    InstanceGatewayBinding,
}

impl ResourceKind {
    fn as_db(self) -> &'static str {
        match self {
            Self::LaunchPreset => "launch_preset",
            Self::ApiKeyProvider => "api_key_provider",
            Self::ApiKeyProviderRevision => "api_key_provider_revision",
            Self::OfficialAccount => "official_account",
            Self::OfficialCredentialRevision => "official_credential_revision",
            Self::Gateway => "gateway",
            Self::GatewayRevision => "gateway_revision",
            Self::Profile => "profile",
            Self::ProfileRevision => "profile_revision",
            Self::RouteSetRevision => "route_set_revision",
            Self::ModelRoute => "model_route",
            Self::ModelRouteIdentity => "model_route_identity",
            Self::GatewayServiceRevision => "gateway_service_revision",
            Self::ModelCatalogRevision => "model_catalog_revision",
            Self::InstanceGatewayBinding => "instance_gateway_binding",
        }
    }

    fn from_db(value: &str) -> Option<Self> {
        match value {
            "launch_preset" => Some(Self::LaunchPreset),
            "api_key_provider" => Some(Self::ApiKeyProvider),
            "api_key_provider_revision" => Some(Self::ApiKeyProviderRevision),
            "official_account" => Some(Self::OfficialAccount),
            "official_credential_revision" => Some(Self::OfficialCredentialRevision),
            "gateway" => Some(Self::Gateway),
            "gateway_revision" => Some(Self::GatewayRevision),
            "profile" => Some(Self::Profile),
            "profile_revision" => Some(Self::ProfileRevision),
            "route_set_revision" => Some(Self::RouteSetRevision),
            "model_route" => Some(Self::ModelRoute),
            "model_route_identity" => Some(Self::ModelRouteIdentity),
            "gateway_service_revision" => Some(Self::GatewayServiceRevision),
            "model_catalog_revision" => Some(Self::ModelCatalogRevision),
            "instance_gateway_binding" => Some(Self::InstanceGatewayBinding),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRef {
    pub kind: ResourceKind,
    pub id: String,
    pub revision_id: Option<String>,
}

impl ResourceRef {
    pub fn logical(kind: ResourceKind, id: impl Into<String>) -> Self {
        Self {
            kind,
            id: id.into(),
            revision_id: None,
        }
    }

    pub fn revision(
        kind: ResourceKind,
        id: impl Into<String>,
        revision_id: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            id: id.into(),
            revision_id: Some(revision_id.into()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockedInstance {
    pub instance_id: String,
    pub state: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationLease {
    pub operation_id: String,
    pub operation_kind: String,
    pub state: String,
    pub resource: ResourceRef,
    pub owner_pid: Option<i64>,
    pub acquired_at: i64,
    pub recovery_detail: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<ResourceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instance_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    pub retryable: bool,
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CommandError {}

#[derive(Clone)]
pub struct ResourceGuard {
    database: Database,
}

impl ResourceGuard {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub fn assert_resources_mutable(&self, resources: &[ResourceRef]) -> Result<(), CommandError> {
        let resources = normalize_resources(resources)?;
        let connection = self.database.connection();
        let connection = connection.lock().map_err(|_| database_error())?;
        assert_resources_mutable_on(&connection, &resources)
    }

    pub fn acquire_operation_leases(
        &self,
        operation_id: &str,
        operation_kind: &str,
        owner_pid: Option<i64>,
        recovery_detail: Option<&str>,
        resources: &[ResourceRef],
        acquired_at: i64,
    ) -> Result<(), CommandError> {
        let operation_id = required_value("operation_id", operation_id)?;
        let operation_kind = required_value("operation_kind", operation_kind)?;
        let resources = normalize_resources(resources)?;
        let connection = self.database.connection();
        let connection = connection.lock().map_err(|_| database_error())?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
            .map_err(|_| database_error())?;
        acquire_operation_leases_in_transaction(
            &transaction,
            operation_id,
            operation_kind,
            owner_pid,
            recovery_detail,
            &resources,
            acquired_at,
        )?;
        transaction.commit().map_err(|_| database_error())
    }

    pub fn release_operation_leases(&self, operation_id: &str) -> Result<usize, CommandError> {
        let operation_id = required_value("operation_id", operation_id)?;
        let connection = self.database.connection();
        let connection = connection.lock().map_err(|_| database_error())?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
            .map_err(|_| database_error())?;
        let removed = release_operation_leases_in_transaction(&transaction, operation_id)?;
        transaction.commit().map_err(|_| database_error())?;
        Ok(removed)
    }

    pub fn mark_operation_recovering(&self, operation_id: &str) -> Result<usize, CommandError> {
        let operation_id = required_value("operation_id", operation_id)?;
        let connection = self.database.connection();
        let connection = connection.lock().map_err(|_| database_error())?;
        connection
            .execute(
                "UPDATE resource_operation_leases SET state = 'recovering' WHERE operation_id = ?1",
                [operation_id],
            )
            .map_err(|_| database_error())
    }

    pub fn recoverable_leases(&self) -> Result<Vec<OperationLease>, CommandError> {
        let connection = self.database.connection();
        let connection = connection.lock().map_err(|_| database_error())?;
        all_leases_on(&connection)
    }

    #[cfg(test)]
    pub fn leases_for_resources(
        &self,
        resources: &[ResourceRef],
    ) -> Result<Vec<OperationLease>, CommandError> {
        let resources = normalize_resources(resources)?;
        let connection = self.database.connection();
        let connection = connection.lock().map_err(|_| database_error())?;
        leases_for_resources_on(&connection, &resources)
    }

    #[cfg(test)]
    pub fn locks_for_resources(
        &self,
        resources: &[ResourceRef],
    ) -> Result<Vec<LockedInstance>, CommandError> {
        let resources = normalize_resources(resources)?;
        let connection = self.database.connection();
        let connection = connection.lock().map_err(|_| database_error())?;
        locked_instances_on(&connection, &resources)
    }

    #[cfg(test)]
    pub fn acquire_runtime_lock(
        &self,
        instance_id: &str,
        launch_configuration_revision_id: &str,
        dependencies: &[ResourceRef],
        acquired_at: i64,
    ) -> Result<(), CommandError> {
        let dependencies = normalize_resources(dependencies)?;
        let connection = self.database.connection();
        let connection = connection.lock().map_err(|_| database_error())?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
            .map_err(|_| database_error())?;
        acquire_runtime_lock_in_transaction(
            &transaction,
            instance_id,
            launch_configuration_revision_id,
            &dependencies,
            acquired_at,
        )?;
        transaction.commit().map_err(|_| database_error())
    }

    #[cfg(test)]
    pub fn release_runtime_lock(
        &self,
        instance_id: &str,
        released_at: i64,
    ) -> Result<(), CommandError> {
        let instance_id = required_value("instance_id", instance_id)?;
        let connection = self.database.connection();
        let connection = connection.lock().map_err(|_| database_error())?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
            .map_err(|_| database_error())?;
        let state = transaction
            .query_row(
                "SELECT state FROM codex_instances WHERE id = ?1",
                [instance_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|_| database_error())?
            .ok_or_else(|| not_found("找不到指定 Codex 实例。"))?;
        if state != "exited" && state != "failed" {
            return Err(invalid_state(
                "只有已确认退出或确认未派发的实例才能释放配置锁。",
            ));
        }
        transaction
            .execute(
                "UPDATE runtime_configuration_locks
                 SET released_at = COALESCE(released_at, ?2)
                 WHERE instance_id = ?1",
                params![instance_id, released_at],
            )
            .map_err(|_| database_error())?;
        transaction.commit().map_err(|_| database_error())
    }
}

pub(crate) fn acquire_operation_leases_in_transaction(
    transaction: &Transaction<'_>,
    operation_id: &str,
    operation_kind: &str,
    owner_pid: Option<i64>,
    recovery_detail: Option<&str>,
    resources: &[ResourceRef],
    acquired_at: i64,
) -> Result<(), CommandError> {
    let operation_id = required_value("operation_id", operation_id)?;
    let operation_kind = required_value("operation_kind", operation_kind)?;
    let resources = normalize_resources(resources)?;
    let locked = locked_instances_on(transaction, &resources)?;
    if !locked.is_empty() {
        return Err(configuration_in_use(&resources, &locked));
    }

    let conflicts = leases_for_resources_on(transaction, &resources)?;
    if let Some(conflict) = conflicts
        .iter()
        .find(|lease| lease.operation_id != operation_id)
    {
        return Err(operation_in_progress(&resources, conflict));
    }
    if conflicts
        .iter()
        .any(|lease| lease.operation_kind != operation_kind)
    {
        return Err(invalid_request(
            "同一 operation_id 不能用于不同类型的资源操作。",
            &resources,
        ));
    }

    for resource in &resources {
        transaction
            .execute(
                "INSERT OR IGNORE INTO resource_operation_leases (
                    resource_kind, resource_id, resource_revision_id,
                    operation_id, operation_kind, state, owner_pid,
                    acquired_at, recovery_detail
                ) VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?7, ?8)",
                params![
                    resource.kind.as_db(),
                    resource.id,
                    resource.revision_id,
                    operation_id,
                    operation_kind,
                    owner_pid,
                    acquired_at,
                    recovery_detail,
                ],
            )
            .map_err(|_| database_error())?;
    }
    Ok(())
}

pub(crate) fn release_operation_leases_in_transaction(
    transaction: &Transaction<'_>,
    operation_id: &str,
) -> Result<usize, CommandError> {
    let operation_id = required_value("operation_id", operation_id)?;
    transaction
        .execute(
            "DELETE FROM resource_operation_leases WHERE operation_id = ?1",
            [operation_id],
        )
        .map_err(|_| database_error())
}

#[cfg(test)]
pub(crate) fn acquire_runtime_lock_in_transaction(
    transaction: &Transaction<'_>,
    instance_id: &str,
    launch_configuration_revision_id: &str,
    dependencies: &[ResourceRef],
    acquired_at: i64,
) -> Result<(), CommandError> {
    let instance_id = required_value("instance_id", instance_id)?;
    let launch_configuration_revision_id = required_value(
        "launch_configuration_revision_id",
        launch_configuration_revision_id,
    )?;
    let dependencies = normalize_resources(dependencies)?;
    let instance = transaction
        .query_row(
            "SELECT launch_configuration_revision_id, state
             FROM codex_instances WHERE id = ?1",
            [instance_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|_| database_error())?
        .ok_or_else(|| not_found("找不到指定 Codex 实例。"))?;
    if instance.0 != launch_configuration_revision_id {
        return Err(invalid_state("实例与启动配置修订不匹配。"));
    }
    if !matches!(
        instance.1.as_str(),
        "launching" | "running" | "stopping" | "orphaned" | "degraded"
    ) {
        return Err(invalid_state("终态实例不能重新获取配置锁。"));
    }

    for resource in &dependencies {
        transaction
            .execute(
                "INSERT OR IGNORE INTO launch_configuration_dependencies (
                    launch_configuration_revision_id, resource_kind,
                    resource_id, resource_revision_id
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![
                    launch_configuration_revision_id,
                    resource.kind.as_db(),
                    resource.id,
                    resource.revision_id,
                ],
            )
            .map_err(|_| database_error())?;
        let stored_revision = transaction
            .query_row(
                "SELECT resource_revision_id
                 FROM launch_configuration_dependencies
                 WHERE launch_configuration_revision_id = ?1
                   AND resource_kind = ?2 AND resource_id = ?3",
                params![
                    launch_configuration_revision_id,
                    resource.kind.as_db(),
                    resource.id,
                ],
                |row| row.get::<_, Option<String>>(0),
            )
            .map_err(|_| database_error())?;
        if stored_revision != resource.revision_id {
            return Err(invalid_state(
                "启动配置依赖已经存在且修订不一致，不能覆盖不可变快照。",
            ));
        }
    }

    let persisted_dependencies =
        dependencies_for_configuration(transaction, launch_configuration_revision_id)?;
    if let Some(conflict) = leases_for_resources_on(transaction, &persisted_dependencies)?
        .first()
        .cloned()
    {
        return Err(operation_in_progress(&persisted_dependencies, &conflict));
    }

    let existing = transaction
        .query_row(
            "SELECT launch_configuration_revision_id, released_at
             FROM runtime_configuration_locks WHERE instance_id = ?1",
            [instance_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?)),
        )
        .optional()
        .map_err(|_| database_error())?;
    match existing {
        Some((configuration_id, None)) if configuration_id == launch_configuration_revision_id => {
            return Ok(())
        }
        Some(_) => return Err(invalid_state("实例配置锁已经释放或绑定了其他启动修订。")),
        None => {}
    }

    transaction
        .execute(
            "INSERT INTO runtime_configuration_locks (
                instance_id, launch_configuration_revision_id, acquired_at, released_at
             ) VALUES (?1, ?2, ?3, NULL)",
            params![instance_id, launch_configuration_revision_id, acquired_at],
        )
        .map_err(|_| database_error())?;
    Ok(())
}

fn assert_resources_mutable_on(
    connection: &rusqlite::Connection,
    resources: &[ResourceRef],
) -> Result<(), CommandError> {
    let locked = locked_instances_on(connection, resources)?;
    if !locked.is_empty() {
        return Err(configuration_in_use(resources, &locked));
    }
    if let Some(conflict) = leases_for_resources_on(connection, resources)?.first() {
        return Err(operation_in_progress(resources, conflict));
    }
    Ok(())
}

fn locked_instances_on(
    connection: &rusqlite::Connection,
    resources: &[ResourceRef],
) -> Result<Vec<LockedInstance>, CommandError> {
    let mut locked = BTreeMap::new();
    for resource in resources {
        let mut statement = connection
            .prepare(
                "SELECT instances.id, instances.state
                 FROM launch_configuration_dependencies dependencies
                 JOIN runtime_configuration_locks locks
                   ON locks.launch_configuration_revision_id = dependencies.launch_configuration_revision_id
                  AND locks.released_at IS NULL
                 JOIN codex_instances instances ON instances.id = locks.instance_id
                 WHERE dependencies.resource_kind = ?1
                   AND dependencies.resource_id = ?2
                   AND (?3 IS NULL OR dependencies.resource_revision_id = ?3)",
            )
            .map_err(|_| database_error())?;
        let rows = statement
            .query_map(
                params![resource.kind.as_db(), resource.id, resource.revision_id,],
                |row| {
                    Ok(LockedInstance {
                        instance_id: row.get(0)?,
                        state: row.get(1)?,
                    })
                },
            )
            .map_err(|_| database_error())?;
        for row in rows {
            let row = row.map_err(|_| database_error())?;
            if !matches!(
                row.state.as_str(),
                "launching" | "running" | "stopping" | "orphaned" | "degraded"
            ) {
                continue;
            }
            locked.insert(row.instance_id.clone(), row);
        }
    }
    Ok(locked.into_values().collect())
}

fn leases_for_resources_on(
    connection: &rusqlite::Connection,
    resources: &[ResourceRef],
) -> Result<Vec<OperationLease>, CommandError> {
    let mut leases = BTreeMap::new();
    for resource in resources {
        let lease = connection
            .query_row(
                "SELECT operation_id, operation_kind, state,
                        resource_revision_id, owner_pid, acquired_at, recovery_detail
                 FROM resource_operation_leases
                 WHERE resource_kind = ?1 AND resource_id = ?2",
                params![resource.kind.as_db(), resource.id],
                |row| {
                    Ok(OperationLease {
                        operation_id: row.get(0)?,
                        operation_kind: row.get(1)?,
                        state: row.get(2)?,
                        resource: ResourceRef {
                            kind: resource.kind,
                            id: resource.id.clone(),
                            revision_id: row.get(3)?,
                        },
                        owner_pid: row.get(4)?,
                        acquired_at: row.get(5)?,
                        recovery_detail: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(|_| database_error())?;
        if let Some(lease) = lease {
            leases.insert(
                (
                    lease.resource.kind,
                    lease.resource.id.clone(),
                    lease.operation_id.clone(),
                ),
                lease,
            );
        }
    }
    Ok(leases.into_values().collect())
}

fn all_leases_on(connection: &rusqlite::Connection) -> Result<Vec<OperationLease>, CommandError> {
    let mut statement = connection
        .prepare(
            "SELECT resource_kind, resource_id, resource_revision_id,
                    operation_id, operation_kind, state, owner_pid,
                    acquired_at, recovery_detail
             FROM resource_operation_leases
             ORDER BY acquired_at, operation_id, resource_kind, resource_id",
        )
        .map_err(|_| database_error())?;
    let rows = statement
        .query_map([], |row| {
            let kind_value = row.get::<_, String>(0)?;
            let kind = ResourceKind::from_db(&kind_value).ok_or_else(|| {
                rusqlite::Error::InvalidColumnType(
                    0,
                    "resource_kind".to_string(),
                    rusqlite::types::Type::Text,
                )
            })?;
            Ok(OperationLease {
                resource: ResourceRef {
                    kind,
                    id: row.get(1)?,
                    revision_id: row.get(2)?,
                },
                operation_id: row.get(3)?,
                operation_kind: row.get(4)?,
                state: row.get(5)?,
                owner_pid: row.get(6)?,
                acquired_at: row.get(7)?,
                recovery_detail: row.get(8)?,
            })
        })
        .map_err(|_| database_error())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|_| database_error())
}

#[cfg(test)]
fn dependencies_for_configuration(
    connection: &rusqlite::Connection,
    configuration_id: &str,
) -> Result<Vec<ResourceRef>, CommandError> {
    let mut statement = connection
        .prepare(
            "SELECT resource_kind, resource_id, resource_revision_id
             FROM launch_configuration_dependencies
             WHERE launch_configuration_revision_id = ?1
             ORDER BY resource_kind, resource_id",
        )
        .map_err(|_| database_error())?;
    let rows = statement
        .query_map([configuration_id], |row| {
            let kind_value = row.get::<_, String>(0)?;
            let kind = ResourceKind::from_db(&kind_value).ok_or_else(|| {
                rusqlite::Error::InvalidColumnType(
                    0,
                    "resource_kind".to_string(),
                    rusqlite::types::Type::Text,
                )
            })?;
            Ok(ResourceRef {
                kind,
                id: row.get(1)?,
                revision_id: row.get(2)?,
            })
        })
        .map_err(|_| database_error())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|_| database_error())
}

fn normalize_resources(resources: &[ResourceRef]) -> Result<Vec<ResourceRef>, CommandError> {
    if resources.is_empty() {
        return Err(invalid_request("资源集合不能为空。", resources));
    }
    let mut normalized = BTreeMap::new();
    for resource in resources {
        let id = resource.id.trim();
        if id.is_empty() {
            return Err(invalid_request("资源 ID 不能为空。", resources));
        }
        let revision_id = resource
            .revision_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let key = (resource.kind, id.to_string());
        let value = ResourceRef {
            kind: resource.kind,
            id: id.to_string(),
            revision_id,
        };
        if let Some(existing) = normalized.get(&key) {
            if existing != &value {
                return Err(invalid_request(
                    "同一逻辑资源不能同时指定不同修订。",
                    resources,
                ));
            }
        } else {
            normalized.insert(key, value);
        }
    }
    Ok(normalized.into_values().collect())
}

fn required_value<'a>(field: &str, value: &'a str) -> Result<&'a str, CommandError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(invalid_request(&format!("{field} 不能为空。"), &[]));
    }
    Ok(value)
}

fn configuration_in_use(resources: &[ResourceRef], locked: &[LockedInstance]) -> CommandError {
    CommandError {
        code: "configuration_in_use".to_string(),
        message: "配置正在被运行实例使用，请先停止相关实例。".to_string(),
        resources: resources.to_vec(),
        instance_ids: locked
            .iter()
            .map(|instance| instance.instance_id.clone())
            .collect(),
        operation_id: None,
        retryable: false,
    }
}

fn operation_in_progress(resources: &[ResourceRef], conflict: &OperationLease) -> CommandError {
    CommandError {
        code: "resource_operation_in_progress".to_string(),
        message: "资源正在执行另一项配置操作，请等待恢复或完成。".to_string(),
        resources: resources.to_vec(),
        instance_ids: Vec::new(),
        operation_id: Some(conflict.operation_id.clone()),
        retryable: true,
    }
}

fn invalid_request(message: &str, resources: &[ResourceRef]) -> CommandError {
    CommandError {
        code: "invalid_request".to_string(),
        message: message.to_string(),
        resources: resources.to_vec(),
        instance_ids: Vec::new(),
        operation_id: None,
        retryable: false,
    }
}

#[cfg(test)]
fn invalid_state(message: &str) -> CommandError {
    CommandError {
        code: "invalid_state".to_string(),
        message: message.to_string(),
        resources: Vec::new(),
        instance_ids: Vec::new(),
        operation_id: None,
        retryable: false,
    }
}

#[cfg(test)]
fn not_found(message: &str) -> CommandError {
    CommandError {
        code: "not_found".to_string(),
        message: message.to_string(),
        resources: Vec::new(),
        instance_ids: Vec::new(),
        operation_id: None,
        retryable: false,
    }
}

fn database_error() -> CommandError {
    CommandError {
        code: "database_error".to_string(),
        message: "本地配置数据库操作失败。".to_string(),
        resources: Vec::new(),
        instance_ids: Vec::new(),
        operation_id: None,
        retryable: true,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Barrier},
        thread,
    };

    use rusqlite::params;
    use uuid::Uuid;

    use super::*;

    fn insert_launch_fixture(database: &Database, configuration_id: &str, instance_id: &str) {
        let connection = database.connection();
        let connection = connection.lock().unwrap();
        connection
            .execute(
                "INSERT INTO launch_configuration_revisions (
                    id, credential_kind, profile_snapshot_hash, config_snapshot,
                    codex_executable, codex_version, created_at
                 ) VALUES (?1, 'api_key', 'hash', '{}', '/usr/bin/true', 'test', 1)",
                [configuration_id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO codex_instances (
                    id, launch_configuration_revision_id, credential_kind,
                    api_key_provider_revision_id, workspace_path,
                    managed_codex_home, lifecycle_dir, launch_nonce, state
                 ) VALUES (?1, ?2, 'api_key', ?3, NULL, ?4, ?5, ?6, 'launching')",
                params![
                    instance_id,
                    configuration_id,
                    format!("v4-provider:provider-a:revision:1"),
                    format!("/tmp/{instance_id}/home"),
                    format!("/tmp/{instance_id}/lifecycle"),
                    format!("nonce-{instance_id}"),
                ],
            )
            .unwrap();
    }

    fn database_with_provider(path: &std::path::Path) -> Database {
        let database = Database::open_path(path).unwrap();
        let connection = database.connection();
        let connection = connection.lock().unwrap();
        connection
            .execute(
                "INSERT INTO api_key_providers (
                    id, name, base_url, secret_ref, is_default,
                    transport_mode, configured, created_at, updated_at
                 ) VALUES ('provider-a', 'A', 'https://example.com/v1',
                           'provider:a', 1, 'direct_responses', 1, 1, 1)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO api_key_provider_revisions (
                    id, provider_id, secret_ref, base_url, wire_api,
                    transport_mode, state, created_at
                 ) VALUES ('v4-provider:provider-a:revision:1', 'provider-a',
                           'provider:a:revision:1', 'https://example.com/v1',
                           'responses', 'direct_responses', 'active', 1)",
                [],
            )
            .unwrap();
        drop(connection);
        database
    }

    #[test]
    fn v6_route_resource_kinds_round_trip_through_storage_names() {
        for kind in [
            ResourceKind::RouteSetRevision,
            ResourceKind::ModelRoute,
            ResourceKind::ModelRouteIdentity,
            ResourceKind::GatewayServiceRevision,
            ResourceKind::ModelCatalogRevision,
            ResourceKind::InstanceGatewayBinding,
        ] {
            assert_eq!(ResourceKind::from_db(kind.as_db()), Some(kind));
        }
    }

    #[test]
    fn runtime_lock_blocks_logical_and_revision_resources_until_terminal_release() {
        let path = std::env::temp_dir().join(format!("rcodex-guard-{}.sqlite3", Uuid::new_v4()));
        let database = database_with_provider(&path);
        insert_launch_fixture(&database, "config-a", "instance-a");
        let guard = ResourceGuard::new(database.clone());
        let resources = vec![
            ResourceRef::revision(ResourceKind::ApiKeyProvider, "provider-a", "revision-a"),
            ResourceRef::logical(ResourceKind::ApiKeyProviderRevision, "revision-a"),
            ResourceRef::logical(ResourceKind::RouteSetRevision, "route-set-a"),
            ResourceRef::logical(ResourceKind::ModelRoute, "model-route-a"),
            ResourceRef::logical(ResourceKind::InstanceGatewayBinding, "instance-binding-a"),
        ];
        guard
            .acquire_runtime_lock("instance-a", "config-a", &resources, 1)
            .unwrap();

        let error = guard
            .acquire_operation_leases("replace-a", "replace_api_key", None, None, &resources, 2)
            .unwrap_err();
        assert_eq!(error.code, "configuration_in_use");
        assert_eq!(error.instance_ids, vec!["instance-a"]);
        assert_eq!(
            guard
                .locks_for_resources(&[ResourceRef::revision(
                    ResourceKind::ApiKeyProvider,
                    "provider-a",
                    "revision-a"
                )])
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            guard
                .locks_for_resources(&[ResourceRef::logical(
                    ResourceKind::RouteSetRevision,
                    "route-set-a"
                )])
                .unwrap()[0]
                .instance_id,
            "instance-a"
        );

        assert_eq!(
            guard
                .release_runtime_lock("instance-a", 3)
                .unwrap_err()
                .code,
            "invalid_state"
        );
        {
            let connection = database.connection();
            let connection = connection.lock().unwrap();
            connection
                .execute(
                    "UPDATE codex_instances SET state = 'exited', exited_at = 3 WHERE id = 'instance-a'",
                    [],
                )
                .unwrap();
        }
        guard.release_runtime_lock("instance-a", 3).unwrap();
        guard
            .acquire_operation_leases("replace-a", "replace_api_key", None, None, &resources, 4)
            .unwrap();
        drop(database);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn unrelated_resources_do_not_interlock_and_recovery_is_explicit() {
        let database = Database::in_memory();
        let guard = ResourceGuard::new(database);
        let provider_a = [ResourceRef::logical(
            ResourceKind::ApiKeyProvider,
            "provider-a",
        )];
        let provider_b = [ResourceRef::logical(
            ResourceKind::ApiKeyProvider,
            "provider-b",
        )];
        guard
            .acquire_operation_leases(
                "operation-a",
                "replace_api_key",
                Some(10),
                Some("new-secret-ref-a"),
                &provider_a,
                1,
            )
            .unwrap();
        guard
            .acquire_operation_leases(
                "operation-b",
                "replace_api_key",
                Some(11),
                None,
                &provider_b,
                1,
            )
            .unwrap();
        assert_eq!(guard.recoverable_leases().unwrap().len(), 2);
        assert_eq!(guard.mark_operation_recovering("operation-a").unwrap(), 1);
        assert_eq!(
            guard.leases_for_resources(&provider_a).unwrap()[0].state,
            "recovering"
        );
        assert_eq!(guard.release_operation_leases("operation-a").unwrap(), 1);
        guard
            .acquire_operation_leases(
                "operation-a-retry",
                "replace_api_key",
                None,
                None,
                &provider_a,
                2,
            )
            .unwrap();
    }

    #[test]
    fn two_database_connections_compete_for_the_same_resource() {
        let path = std::env::temp_dir().join(format!("rcodex-race-{}.sqlite3", Uuid::new_v4()));
        let first = ResourceGuard::new(Database::open_path(&path).unwrap());
        let second = ResourceGuard::new(Database::open_path(&path).unwrap());
        let barrier = Arc::new(Barrier::new(3));
        let resource = ResourceRef::logical(ResourceKind::ApiKeyProvider, "provider-a");
        let handles = [first, second]
            .into_iter()
            .enumerate()
            .map(|(index, guard)| {
                let barrier = barrier.clone();
                let resource = resource.clone();
                thread::spawn(move || {
                    barrier.wait();
                    guard.acquire_operation_leases(
                        &format!("operation-{index}"),
                        "replace_api_key",
                        None,
                        None,
                        &[resource],
                        1,
                    )
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter_map(|result| result.as_ref().err())
                .filter(|error| error.code == "resource_operation_in_progress")
                .count(),
            1
        );
        let _ = std::fs::remove_file(path);
    }
}
