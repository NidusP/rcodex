use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::{
    database::Database,
    routing::{
        CapabilitySet, GatewayServiceRevision, GatewayTimeoutSnapshot, ModelRoute,
        ModelRouteIdentity, RouteKind, RouteSetProviderRevision, RouteSetRevision, RouteSetState,
        RouteSource, WireApi,
    },
};

#[derive(Clone)]
pub struct RouteStore {
    connection: Arc<Mutex<Connection>>,
}

impl RouteStore {
    pub fn from_database(database: Database) -> Self {
        Self {
            connection: database.connection(),
        }
    }

    #[cfg(test)]
    pub fn save_identity(&self, identity: &ModelRouteIdentity) -> Result<(), String> {
        identity.validate().map_err(|error| error.to_string())?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "route_store_database_lock_failed".to_string())?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("route_store_transaction_failed:{error}"))?;

        if let Some(existing) = identity_by_id(&transaction, &identity.id)? {
            if existing == *identity {
                transaction
                    .commit()
                    .map_err(|error| format!("route_store_commit_failed:{error}"))?;
                return Ok(());
            }
            return Err("model_route_identity_immutable".to_string());
        }

        let (official_account_id, api_key_provider_id) = match identity.source_kind {
            RouteKind::Official => (Some(identity.source_id.as_str()), None),
            RouteKind::ApiProvider => (None, Some(identity.source_id.as_str())),
        };
        transaction
            .execute(
                "INSERT INTO model_route_identities (
                    id, public_model_id, source_kind, official_account_id,
                    api_key_provider_id, upstream_model_id, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    identity.id,
                    identity.public_model_id,
                    identity.source_kind.as_db(),
                    official_account_id,
                    api_key_provider_id,
                    identity.upstream_model_id,
                    crate::support::unix_now_seconds(),
                ],
            )
            .map_err(|error| format!("model_route_identity_conflict:{error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("route_store_commit_failed:{error}"))
    }

    pub fn insert_ready_route_set(&self, route_set: &RouteSetRevision) -> Result<(), String> {
        route_set
            .validate_ready()
            .map_err(|error| error.to_string())?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "route_store_database_lock_failed".to_string())?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("route_store_transaction_failed:{error}"))?;

        let exists: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM route_set_revisions WHERE id=?1)",
                [&route_set.id],
                |row| row.get(0),
            )
            .map_err(|error| format!("route_store_read_failed:{error}"))?;
        if exists {
            return Err("route_set_revision_immutable".to_string());
        }

        save_gateway_service_revision(&transaction, &route_set.gateway_service)?;
        for route in &route_set.routes {
            let identity = identity_by_id(&transaction, &route.route_identity_id)?
                .ok_or_else(|| "model_route_identity_not_found".to_string())?;
            if !route_matches_identity(route, &identity) {
                return Err("model_route_identity_mismatch".to_string());
            }
        }

        transaction
            .execute(
                "INSERT INTO route_set_revisions (
                    id, launch_preset_id, launch_preset_version,
                    official_account_id, official_credential_revision_id,
                    gateway_service_revision_id, requires_openai_auth,
                    catalog_path, catalog_sha256, state, created_at, failure_code
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'ready', ?10, NULL)",
                params![
                    route_set.id,
                    route_set.launch_preset_id,
                    route_set.launch_preset_version,
                    route_set.official_account_id,
                    route_set.official_credential_revision_id,
                    route_set.gateway_service.id,
                    route_set.requires_openai_auth,
                    route_set.catalog_path,
                    route_set.catalog_sha256,
                    route_set.created_at,
                ],
            )
            .map_err(|error| format!("route_set_insert_failed:{error}"))?;

        for provider in &route_set.providers {
            transaction
                .execute(
                    "INSERT INTO route_set_provider_revisions (
                        route_set_revision_id, provider_id, provider_revision_id,
                        position, connect_seconds, first_byte_seconds,
                        stream_idle_seconds, non_stream_total_seconds
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        route_set.id,
                        provider.provider_id,
                        provider.provider_revision_id,
                        provider.position,
                        provider.timeouts.connect_seconds,
                        provider.timeouts.first_byte_seconds,
                        provider.timeouts.stream_idle_seconds,
                        provider.timeouts.non_stream_total_seconds,
                    ],
                )
                .map_err(|error| format!("route_set_provider_insert_failed:{error}"))?;
        }

        for route in &route_set.routes {
            let capabilities_json = serde_json::to_string(&route.capabilities)
                .map_err(|error| format!("route_capabilities_encode_failed:{error}"))?;
            let (
                official_account_id,
                official_credential_revision_id,
                api_key_provider_id,
                api_key_provider_revision_id,
            ) = match &route.source {
                RouteSource::Official {
                    account_id,
                    credential_revision_id,
                } => (
                    Some(account_id.as_str()),
                    Some(credential_revision_id.as_str()),
                    None,
                    None,
                ),
                RouteSource::ApiProvider {
                    provider_id,
                    provider_revision_id,
                } => (
                    None,
                    None,
                    Some(provider_id.as_str()),
                    Some(provider_revision_id.as_str()),
                ),
            };
            transaction
                .execute(
                    "INSERT INTO model_routes (
                        id, route_set_revision_id, route_identity_id,
                        public_model_id, display_name, route_kind,
                        official_account_id, official_credential_revision_id,
                        api_key_provider_id, api_key_provider_revision_id,
                        upstream_model_id, wire_api, adapter_kind,
                        adapter_version, capabilities_json, position
                     ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                        ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16
                     )",
                    params![
                        route.id,
                        route_set.id,
                        route.route_identity_id,
                        route.public_model_id,
                        route.display_name,
                        route.source.kind().as_db(),
                        official_account_id,
                        official_credential_revision_id,
                        api_key_provider_id,
                        api_key_provider_revision_id,
                        route.upstream_model_id,
                        route.wire_api.as_db(),
                        route.adapter_kind,
                        route.adapter_version,
                        capabilities_json,
                        route.position,
                    ],
                )
                .map_err(|error| format!("model_route_insert_failed:{error}"))?;
        }

        transaction
            .commit()
            .map_err(|error| format!("route_store_commit_failed:{error}"))
    }

    pub fn ready_route_set(&self, id: &str) -> Result<Option<RouteSetRevision>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "route_store_database_lock_failed".to_string())?;
        load_ready_route_set(&connection, id)
    }
}

fn identity_by_id(connection: &Connection, id: &str) -> Result<Option<ModelRouteIdentity>, String> {
    connection
        .query_row(
            "SELECT id, public_model_id, source_kind,
                    COALESCE(official_account_id, api_key_provider_id),
                    upstream_model_id
             FROM model_route_identities WHERE id=?1",
            [id],
            |row| {
                let source_kind = row.get::<_, String>(2)?;
                let source_kind = RouteKind::from_db(&source_kind).ok_or_else(|| {
                    rusqlite::Error::InvalidColumnType(
                        2,
                        "source_kind".to_string(),
                        rusqlite::types::Type::Text,
                    )
                })?;
                Ok(ModelRouteIdentity {
                    id: row.get(0)?,
                    public_model_id: row.get(1)?,
                    source_kind,
                    source_id: row.get(3)?,
                    upstream_model_id: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|error| format!("route_identity_read_failed:{error}"))
}

fn save_gateway_service_revision(
    connection: &Connection,
    revision: &GatewayServiceRevision,
) -> Result<(), String> {
    let existing = connection
        .query_row(
            "SELECT protocol_version, config_snapshot, config_sha256, created_at
             FROM gateway_service_revisions WHERE id=?1",
            [&revision.id],
            |row| {
                Ok(GatewayServiceRevision {
                    id: revision.id.clone(),
                    protocol_version: row.get(0)?,
                    config_snapshot: row.get(1)?,
                    config_sha256: row.get(2)?,
                    created_at: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(|error| format!("gateway_revision_read_failed:{error}"))?;
    if let Some(existing) = existing {
        return if existing == *revision {
            Ok(())
        } else {
            Err("gateway_service_revision_immutable".to_string())
        };
    }
    connection
        .execute(
            "INSERT INTO gateway_service_revisions (
                id, protocol_version, bind_host, config_snapshot,
                config_sha256, created_at
             ) VALUES (?1, ?2, '127.0.0.1', ?3, ?4, ?5)",
            params![
                revision.id,
                revision.protocol_version,
                revision.config_snapshot,
                revision.config_sha256,
                revision.created_at,
            ],
        )
        .map_err(|error| format!("gateway_revision_insert_failed:{error}"))?;
    Ok(())
}

fn load_ready_route_set(
    connection: &Connection,
    id: &str,
) -> Result<Option<RouteSetRevision>, String> {
    let header = connection
        .query_row(
            "SELECT route_set.id, route_set.launch_preset_id,
                    route_set.launch_preset_version,
                    route_set.official_account_id,
                    route_set.official_credential_revision_id,
                    route_set.requires_openai_auth, route_set.catalog_path,
                    route_set.catalog_sha256, route_set.state, route_set.created_at,
                    gateway.id, gateway.protocol_version,
                    gateway.config_snapshot, gateway.config_sha256, gateway.created_at
             FROM route_set_revisions route_set
             JOIN gateway_service_revisions gateway
               ON gateway.id=route_set.gateway_service_revision_id
             WHERE route_set.id=?1 AND route_set.state='ready'",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u64>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, bool>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, i64>(9)?,
                    GatewayServiceRevision {
                        id: row.get(10)?,
                        protocol_version: row.get(11)?,
                        config_snapshot: row.get(12)?,
                        config_sha256: row.get(13)?,
                        created_at: row.get(14)?,
                    },
                ))
            },
        )
        .optional()
        .map_err(|error| format!("route_set_read_failed:{error}"))?;
    let Some(header) = header else {
        return Ok(None);
    };
    let state =
        RouteSetState::from_db(&header.8).ok_or_else(|| "route_set_state_invalid".to_string())?;

    let mut provider_statement = connection
        .prepare(
            "SELECT provider_id, provider_revision_id, position,
                    connect_seconds, first_byte_seconds,
                    stream_idle_seconds, non_stream_total_seconds
             FROM route_set_provider_revisions
             WHERE route_set_revision_id=?1 ORDER BY position",
        )
        .map_err(|error| format!("route_set_provider_read_failed:{error}"))?;
    let providers = provider_statement
        .query_map([id], |row| {
            Ok(RouteSetProviderRevision {
                provider_id: row.get(0)?,
                provider_revision_id: row.get(1)?,
                position: row.get(2)?,
                timeouts: GatewayTimeoutSnapshot {
                    connect_seconds: row.get(3)?,
                    first_byte_seconds: row.get(4)?,
                    stream_idle_seconds: row.get(5)?,
                    non_stream_total_seconds: row.get(6)?,
                },
            })
        })
        .map_err(|error| format!("route_set_provider_read_failed:{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("route_set_provider_decode_failed:{error}"))?;

    let mut route_statement = connection
        .prepare(
            "SELECT id, route_identity_id, public_model_id, display_name,
                    route_kind, official_account_id,
                    official_credential_revision_id, api_key_provider_id,
                    api_key_provider_revision_id, upstream_model_id,
                    wire_api, adapter_kind, adapter_version,
                    capabilities_json, position
             FROM model_routes WHERE route_set_revision_id=?1 ORDER BY position",
        )
        .map_err(|error| format!("model_route_read_failed:{error}"))?;
    let routes = route_statement
        .query_map([id], |row| {
            let route_kind = row.get::<_, String>(4)?;
            let route_kind = RouteKind::from_db(&route_kind).ok_or_else(|| {
                rusqlite::Error::InvalidColumnType(
                    4,
                    "route_kind".to_string(),
                    rusqlite::types::Type::Text,
                )
            })?;
            let wire_api = row.get::<_, String>(10)?;
            let wire_api = WireApi::from_db(&wire_api).ok_or_else(|| {
                rusqlite::Error::InvalidColumnType(
                    10,
                    "wire_api".to_string(),
                    rusqlite::types::Type::Text,
                )
            })?;
            let capabilities_json = row.get::<_, String>(13)?;
            let capabilities =
                serde_json::from_str::<CapabilitySet>(&capabilities_json).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        13,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
            let source = match route_kind {
                RouteKind::Official => RouteSource::Official {
                    account_id: row.get(5)?,
                    credential_revision_id: row.get(6)?,
                },
                RouteKind::ApiProvider => RouteSource::ApiProvider {
                    provider_id: row.get(7)?,
                    provider_revision_id: row.get(8)?,
                },
            };
            Ok(ModelRoute {
                id: row.get(0)?,
                route_identity_id: row.get(1)?,
                public_model_id: row.get(2)?,
                display_name: row.get(3)?,
                source,
                upstream_model_id: row.get(9)?,
                wire_api,
                adapter_kind: row.get(11)?,
                adapter_version: row.get(12)?,
                capabilities,
                position: row.get(14)?,
            })
        })
        .map_err(|error| format!("model_route_read_failed:{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("model_route_decode_failed:{error}"))?;

    let route_set = RouteSetRevision {
        id: header.0,
        launch_preset_id: header.1,
        launch_preset_version: header.2,
        official_account_id: header.3,
        official_credential_revision_id: header.4,
        gateway_service: header.10,
        requires_openai_auth: header.5,
        catalog_path: header.6,
        catalog_sha256: header.7,
        state,
        created_at: header.9,
        providers,
        routes,
    };
    route_set
        .validate_ready()
        .map_err(|error| format!("persisted_route_set_invalid:{error}"))?;
    Ok(Some(route_set))
}

fn route_matches_identity(route: &ModelRoute, identity: &ModelRouteIdentity) -> bool {
    route.public_model_id == identity.public_model_id
        && route.upstream_model_id == identity.upstream_model_id
        && route.source.kind() == identity.source_kind
        && match &route.source {
            RouteSource::Official { account_id, .. } => account_id == &identity.source_id,
            RouteSource::ApiProvider { provider_id, .. } => provider_id == &identity.source_id,
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_provider_and_preset(database: &Database) {
        let connection = database.connection();
        let connection = connection.lock().unwrap();
        connection
            .execute_batch(
                "INSERT INTO api_key_providers (
                    id, name, base_url, secret_ref, is_default,
                    transport_mode, configured, created_at, current_revision_id, updated_at
                 ) VALUES (
                    'provider-a', 'Provider A', 'https://example.com/v1',
                    'provider:a:revision:1', 1, 'gateway_passthrough', 1, 1,
                    NULL, 1
                 );
                 INSERT INTO api_key_provider_revisions (
                    id, provider_id, secret_ref, base_url, wire_api,
                    transport_mode, state, created_at
                 ) VALUES (
                    'provider-revision-a', 'provider-a', 'provider:a:revision:1',
                    'https://example.com/v1', 'responses',
                    'gateway_passthrough', 'active', 1
                 );
                 UPDATE api_key_providers
                    SET current_revision_id='provider-revision-a'
                    WHERE id='provider-a';
                 INSERT INTO launch_presets (
                    id, name, credential_kind, api_key_provider_id,
                    codex_account_id, gateway_policy, profile_options,
                    default_workspace_path, version, is_preferred,
                    created_at, updated_at
                 ) VALUES (
                    'preset-a', 'Preset A', 'api_key', 'provider-a', NULL,
                    'gateway_passthrough', '{}', NULL, 1, 1, 1, 1
                 );",
            )
            .unwrap();
    }

    fn seed_official_account(database: &Database) {
        let connection = database.connection();
        let connection = connection.lock().unwrap();
        connection
            .execute_batch(
                "INSERT INTO codex_accounts (
                    id, masked_identity, workspace_label, plan_label,
                    current_credential_revision_id, created_at, updated_at
                 ) VALUES (
                    'account-a', 'a***@example.com', NULL, 'Pro', NULL, 1, 1
                 );
                 INSERT INTO official_credential_revisions (
                    id, account_id, managed_codex_home, auth_state,
                    auth_checked_at, auth_error_code, codex_version,
                    schema_fingerprint, created_at, retired_at
                 ) VALUES (
                    'credential-a', 'account-a', '/managed/account-a',
                    'authorized', 1, NULL, 'test', 'schema-a', 1, NULL
                 );
                 UPDATE codex_accounts
                    SET current_credential_revision_id='credential-a'
                    WHERE id='account-a';",
            )
            .unwrap();
    }

    fn identity() -> ModelRouteIdentity {
        ModelRouteIdentity {
            id: "identity-a".to_string(),
            public_model_id: "3r-deepseek-stable".to_string(),
            source_kind: RouteKind::ApiProvider,
            source_id: "provider-a".to_string(),
            upstream_model_id: "deepseek-chat".to_string(),
        }
    }

    fn official_identity() -> ModelRouteIdentity {
        ModelRouteIdentity {
            id: "identity-official".to_string(),
            public_model_id: "rcodex-official".to_string(),
            source_kind: RouteKind::Official,
            source_id: "account-a".to_string(),
            upstream_model_id: "gpt-official".to_string(),
        }
    }

    fn route_set() -> RouteSetRevision {
        RouteSetRevision {
            id: "route-set-a".to_string(),
            launch_preset_id: "preset-a".to_string(),
            launch_preset_version: 1,
            official_account_id: None,
            official_credential_revision_id: None,
            gateway_service: GatewayServiceRevision {
                id: "gateway-service-a".to_string(),
                protocol_version: 1,
                config_snapshot: "{}".to_string(),
                config_sha256: "sha256:gateway".to_string(),
                created_at: 2,
            },
            requires_openai_auth: false,
            catalog_path: "/managed/models.json".to_string(),
            catalog_sha256: "sha256:catalog".to_string(),
            state: RouteSetState::Ready,
            created_at: 3,
            providers: vec![RouteSetProviderRevision {
                provider_id: "provider-a".to_string(),
                provider_revision_id: "provider-revision-a".to_string(),
                position: 0,
                timeouts: GatewayTimeoutSnapshot {
                    connect_seconds: 10,
                    first_byte_seconds: 30,
                    stream_idle_seconds: 45,
                    non_stream_total_seconds: 120,
                },
            }],
            routes: vec![ModelRoute {
                id: "model-route-a".to_string(),
                route_identity_id: "identity-a".to_string(),
                public_model_id: "3r-deepseek-stable".to_string(),
                display_name: "DeepSeek".to_string(),
                source: RouteSource::ApiProvider {
                    provider_id: "provider-a".to_string(),
                    provider_revision_id: "provider-revision-a".to_string(),
                },
                upstream_model_id: "deepseek-chat".to_string(),
                wire_api: WireApi::Responses,
                adapter_kind: "responses_passthrough".to_string(),
                adapter_version: "1".to_string(),
                capabilities: CapabilitySet {
                    input_modalities: vec!["text".to_string()],
                    supports_tools: true,
                    ..CapabilitySet::default()
                },
                position: 0,
            }],
        }
    }

    #[test]
    fn ready_route_set_round_trips_and_resolves_only_exact_public_id() {
        let database = Database::in_memory();
        seed_provider_and_preset(&database);
        let store = RouteStore::from_database(database);
        store.save_identity(&identity()).unwrap();
        let expected = route_set();
        store.insert_ready_route_set(&expected).unwrap();

        assert_eq!(store.ready_route_set(&expected.id).unwrap(), Some(expected));
        let stored_routes = store
            .ready_route_set("route-set-a")
            .unwrap()
            .unwrap()
            .routes;
        assert!(stored_routes
            .iter()
            .any(|route| route.public_model_id == "3r-deepseek-stable"));
        assert!(!stored_routes
            .iter()
            .any(|route| route.public_model_id == "3r-deepseek"));
    }

    #[test]
    fn route_set_and_identity_rows_are_immutable() {
        let database = Database::in_memory();
        seed_provider_and_preset(&database);
        let store = RouteStore::from_database(database);
        let original = identity();
        store.save_identity(&original).unwrap();
        store.save_identity(&original).unwrap();
        let mut changed = original;
        changed.upstream_model_id = "different".to_string();
        assert_eq!(
            store.save_identity(&changed).unwrap_err(),
            "model_route_identity_immutable"
        );

        let route_set = route_set();
        store.insert_ready_route_set(&route_set).unwrap();
        assert_eq!(
            store.insert_ready_route_set(&route_set).unwrap_err(),
            "route_set_revision_immutable"
        );
    }

    #[test]
    fn route_identity_must_match_the_selected_source_and_upstream_model() {
        let database = Database::in_memory();
        seed_provider_and_preset(&database);
        let store = RouteStore::from_database(database);
        store.save_identity(&identity()).unwrap();
        let mut route_set = route_set();
        route_set.routes[0].upstream_model_id = "other-model".to_string();
        assert_eq!(
            store.insert_ready_route_set(&route_set).unwrap_err(),
            "model_route_identity_mismatch"
        );
        assert!(store.ready_route_set("route-set-a").unwrap().is_none());
    }

    #[test]
    fn mixed_route_set_round_trips_with_disjoint_credential_columns() {
        let database = Database::in_memory();
        seed_provider_and_preset(&database);
        seed_official_account(&database);
        let store = RouteStore::from_database(database.clone());
        store.save_identity(&identity()).unwrap();
        store.save_identity(&official_identity()).unwrap();

        let mut expected = route_set();
        expected.requires_openai_auth = true;
        expected.official_account_id = Some("account-a".to_string());
        expected.official_credential_revision_id = Some("credential-a".to_string());
        expected.routes.push(ModelRoute {
            id: "model-route-official".to_string(),
            route_identity_id: "identity-official".to_string(),
            public_model_id: "rcodex-official".to_string(),
            display_name: "GPT · Official".to_string(),
            source: RouteSource::Official {
                account_id: "account-a".to_string(),
                credential_revision_id: "credential-a".to_string(),
            },
            upstream_model_id: "gpt-official".to_string(),
            wire_api: WireApi::Responses,
            adapter_kind: "official_passthrough".to_string(),
            adapter_version: "1".to_string(),
            capabilities: CapabilitySet::default(),
            position: 1,
        });
        store.insert_ready_route_set(&expected).unwrap();
        assert_eq!(
            store.ready_route_set("route-set-a").unwrap(),
            Some(expected)
        );

        let connection = database.connection();
        let connection = connection.lock().unwrap();
        let api_columns = connection
            .query_row(
                "SELECT official_account_id, official_credential_revision_id,
                        api_key_provider_id, api_key_provider_revision_id
                 FROM model_routes WHERE id='model-route-a'",
                [],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            api_columns,
            (
                None,
                None,
                Some("provider-a".to_string()),
                Some("provider-revision-a".to_string())
            )
        );
        let official_columns = connection
            .query_row(
                "SELECT official_account_id, official_credential_revision_id,
                        api_key_provider_id, api_key_provider_revision_id
                 FROM model_routes WHERE id='model-route-official'",
                [],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            official_columns,
            (
                Some("account-a".to_string()),
                Some("credential-a".to_string()),
                None,
                None
            )
        );
    }
}
