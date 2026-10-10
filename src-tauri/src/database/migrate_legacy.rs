//! v5/v6 遗留表重建。
//!
//! 不调度其他版本，不打开连接。
//! 由 `migrate` 在旧库升到 v6 时调用。

use rusqlite::Transaction;

use super::migrate_v7_plus::add_column_if_missing;

pub(super) fn migrate_to_v5(
    transaction: &Transaction<'_>,
    backfill_legacy_presets: bool,
) -> Result<(), String> {
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS api_key_provider_revisions (
                id TEXT PRIMARY KEY,
                provider_id TEXT NOT NULL REFERENCES api_key_providers(id) ON DELETE RESTRICT,
                secret_ref TEXT NOT NULL UNIQUE,
                base_url TEXT NOT NULL,
                wire_api TEXT NOT NULL CHECK(wire_api IN ('responses', 'chat_completions')),
                transport_mode TEXT NOT NULL CHECK(transport_mode IN ('direct_responses', 'gateway_passthrough', 'gateway_chat_completions')),
                state TEXT NOT NULL CHECK(state IN ('active', 'credential_missing', 'retired')),
                created_at INTEGER NOT NULL,
                retired_at INTEGER,
                CHECK((state = 'retired') = (retired_at IS NOT NULL))
            );
            CREATE INDEX IF NOT EXISTS api_key_provider_revisions_provider_time
                ON api_key_provider_revisions(provider_id, created_at DESC);

            CREATE TABLE IF NOT EXISTS codex_accounts (
                id TEXT PRIMARY KEY,
                masked_identity TEXT,
                workspace_label TEXT,
                plan_label TEXT,
                current_credential_revision_id TEXT REFERENCES official_credential_revisions(id) ON DELETE RESTRICT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS official_credential_revisions (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL REFERENCES codex_accounts(id) ON DELETE RESTRICT,
                managed_codex_home TEXT NOT NULL UNIQUE,
                auth_state TEXT NOT NULL CHECK(auth_state IN ('authorizing', 'authorized', 'needs_reauth', 'failed', 'retired')),
                auth_checked_at INTEGER,
                auth_error_code TEXT,
                codex_version TEXT NOT NULL,
                schema_fingerprint TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                retired_at INTEGER,
                CHECK((auth_state = 'retired') = (retired_at IS NOT NULL))
            );
            CREATE INDEX IF NOT EXISTS official_credential_revisions_account_time
                ON official_credential_revisions(account_id, created_at DESC);
            CREATE TABLE IF NOT EXISTS codex_authorization_attempts (
                id TEXT PRIMARY KEY,
                account_id TEXT REFERENCES codex_accounts(id) ON DELETE RESTRICT,
                credential_revision_id TEXT REFERENCES official_credential_revisions(id) ON DELETE RESTRICT,
                state TEXT NOT NULL CHECK(state IN ('authorizing', 'awaiting_user', 'verifying', 'authorized', 'cancelled', 'failed', 'interrupted')),
                started_at INTEGER NOT NULL,
                completed_at INTEGER,
                error_code TEXT,
                codex_version TEXT NOT NULL,
                schema_fingerprint TEXT NOT NULL,
                CHECK((state IN ('authorized', 'cancelled', 'failed', 'interrupted')) = (completed_at IS NOT NULL))
            );
            CREATE INDEX IF NOT EXISTS codex_authorization_attempts_account_time
                ON codex_authorization_attempts(account_id, started_at DESC);

            CREATE TABLE IF NOT EXISTS launch_presets (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                credential_kind TEXT NOT NULL CHECK(credential_kind IN ('api_key', 'official', 'none')),
                api_key_provider_id TEXT REFERENCES api_key_providers(id) ON DELETE RESTRICT,
                codex_account_id TEXT REFERENCES codex_accounts(id) ON DELETE RESTRICT,
                gateway_policy TEXT CHECK(gateway_policy IN ('direct', 'gateway_passthrough', 'gateway_chat_completions')),
                profile_options TEXT NOT NULL DEFAULT '{}',
                default_workspace_path TEXT,
                version INTEGER NOT NULL DEFAULT 1 CHECK(version >= 1),
                is_preferred INTEGER NOT NULL DEFAULT 0 CHECK(is_preferred IN (0, 1)),
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                deleted_at INTEGER,
                CHECK(
                    (credential_kind = 'api_key' AND api_key_provider_id IS NOT NULL AND codex_account_id IS NULL)
                    OR
                    (credential_kind = 'official' AND api_key_provider_id IS NULL AND codex_account_id IS NOT NULL AND gateway_policy IS NULL)
                    OR
                    (credential_kind = 'none' AND api_key_provider_id IS NULL AND codex_account_id IS NULL)
                )
            );
            CREATE UNIQUE INDEX IF NOT EXISTS one_preferred_launch_preset
                ON launch_presets(is_preferred) WHERE is_preferred = 1;
            CREATE INDEX IF NOT EXISTS launch_presets_api_key_provider
                ON launch_presets(api_key_provider_id);
            CREATE INDEX IF NOT EXISTS launch_presets_codex_account
                ON launch_presets(codex_account_id);

            CREATE TABLE IF NOT EXISTS gateway_revisions (
                id TEXT PRIMARY KEY,
                provider_id TEXT NOT NULL REFERENCES api_key_providers(id) ON DELETE RESTRICT,
                provider_revision_id TEXT NOT NULL REFERENCES api_key_provider_revisions(id) ON DELETE RESTRICT,
                connect_seconds INTEGER NOT NULL CHECK(connect_seconds BETWEEN 1 AND 120),
                first_byte_seconds INTEGER NOT NULL CHECK(first_byte_seconds BETWEEN 1 AND 600),
                stream_idle_seconds INTEGER NOT NULL CHECK(stream_idle_seconds BETWEEN 1 AND 1800),
                non_stream_total_seconds INTEGER NOT NULL CHECK(non_stream_total_seconds BETWEEN 1 AND 7200),
                state TEXT NOT NULL CHECK(state IN ('active', 'retired')),
                created_at INTEGER NOT NULL,
                retired_at INTEGER,
                CHECK(non_stream_total_seconds >= first_byte_seconds),
                CHECK((state = 'retired') = (retired_at IS NOT NULL))
            );
            CREATE INDEX IF NOT EXISTS gateway_revisions_provider_time
                ON gateway_revisions(provider_id, created_at DESC);

            CREATE TABLE IF NOT EXISTS launch_configuration_revisions (
                id TEXT PRIMARY KEY,
                launch_preset_id TEXT REFERENCES launch_presets(id) ON DELETE RESTRICT,
                launch_preset_version INTEGER CHECK(launch_preset_version IS NULL OR launch_preset_version >= 1),
                credential_kind TEXT NOT NULL CHECK(credential_kind IN ('api_key', 'official')),
                profile_snapshot_hash TEXT NOT NULL,
                config_snapshot TEXT NOT NULL,
                codex_executable TEXT NOT NULL,
                codex_version TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                CHECK((launch_preset_id IS NULL) = (launch_preset_version IS NULL))
            );
            CREATE TABLE IF NOT EXISTS launch_configuration_dependencies (
                launch_configuration_revision_id TEXT NOT NULL REFERENCES launch_configuration_revisions(id) ON DELETE CASCADE,
                resource_kind TEXT NOT NULL CHECK(resource_kind IN ('launch_preset', 'api_key_provider', 'api_key_provider_revision', 'official_account', 'official_credential_revision', 'gateway', 'gateway_revision', 'profile', 'profile_revision')),
                resource_id TEXT NOT NULL CHECK(length(resource_id) > 0),
                resource_revision_id TEXT,
                PRIMARY KEY(launch_configuration_revision_id, resource_kind, resource_id)
            );
            CREATE INDEX IF NOT EXISTS launch_configuration_dependencies_resource
                ON launch_configuration_dependencies(resource_kind, resource_id, resource_revision_id, launch_configuration_revision_id);

            CREATE TABLE IF NOT EXISTS codex_instances (
                id TEXT PRIMARY KEY,
                launch_configuration_revision_id TEXT NOT NULL REFERENCES launch_configuration_revisions(id) ON DELETE RESTRICT,
                credential_kind TEXT NOT NULL CHECK(credential_kind IN ('api_key', 'official')),
                api_key_provider_revision_id TEXT REFERENCES api_key_provider_revisions(id) ON DELETE RESTRICT,
                official_credential_revision_id TEXT REFERENCES official_credential_revisions(id) ON DELETE RESTRICT,
                workspace_path TEXT,
                managed_codex_home TEXT NOT NULL,
                lifecycle_dir TEXT NOT NULL UNIQUE,
                launch_nonce TEXT NOT NULL UNIQUE,
                state TEXT NOT NULL CHECK(state IN ('launching', 'running', 'stopping', 'exited', 'failed', 'orphaned', 'degraded')),
                supervisor_pid INTEGER CHECK(supervisor_pid IS NULL OR supervisor_pid > 0),
                pid INTEGER CHECK(pid IS NULL OR pid > 0),
                process_started_at TEXT,
                launched_at INTEGER,
                exited_at INTEGER,
                exit_code INTEGER,
                CHECK(
                    (credential_kind = 'api_key' AND api_key_provider_revision_id IS NOT NULL AND official_credential_revision_id IS NULL)
                    OR
                    (credential_kind = 'official' AND api_key_provider_revision_id IS NULL AND official_credential_revision_id IS NOT NULL)
                ),
                CHECK((state IN ('exited', 'failed')) = (exited_at IS NOT NULL))
            );
            CREATE INDEX IF NOT EXISTS codex_instances_state_time
                ON codex_instances(state, launched_at DESC);
            CREATE INDEX IF NOT EXISTS codex_instances_launch_configuration
                ON codex_instances(launch_configuration_revision_id);
            CREATE INDEX IF NOT EXISTS codex_instances_api_key_revision
                ON codex_instances(api_key_provider_revision_id, state);
            CREATE INDEX IF NOT EXISTS codex_instances_official_revision
                ON codex_instances(official_credential_revision_id, state);

            CREATE TABLE IF NOT EXISTS runtime_configuration_locks (
                instance_id TEXT PRIMARY KEY REFERENCES codex_instances(id) ON DELETE RESTRICT,
                launch_configuration_revision_id TEXT NOT NULL REFERENCES launch_configuration_revisions(id) ON DELETE RESTRICT,
                acquired_at INTEGER NOT NULL,
                released_at INTEGER
            );
            CREATE INDEX IF NOT EXISTS runtime_configuration_locks_active_configuration
                ON runtime_configuration_locks(launch_configuration_revision_id, instance_id)
                WHERE released_at IS NULL;

            CREATE TABLE IF NOT EXISTS launch_preset_runtime_bindings (
                launch_preset_id TEXT PRIMARY KEY REFERENCES launch_presets(id) ON DELETE CASCADE,
                instance_id TEXT NOT NULL UNIQUE REFERENCES codex_instances(id) ON DELETE RESTRICT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                CHECK(updated_at >= created_at)
            );
            CREATE INDEX IF NOT EXISTS launch_preset_runtime_bindings_instance
                ON launch_preset_runtime_bindings(instance_id);

            CREATE TABLE IF NOT EXISTS resource_operation_leases (
                resource_kind TEXT NOT NULL CHECK(resource_kind IN ('launch_preset', 'api_key_provider', 'api_key_provider_revision', 'official_account', 'official_credential_revision', 'gateway', 'gateway_revision', 'profile', 'profile_revision')),
                resource_id TEXT NOT NULL CHECK(length(resource_id) > 0),
                resource_revision_id TEXT,
                operation_id TEXT NOT NULL CHECK(length(operation_id) > 0),
                operation_kind TEXT NOT NULL CHECK(length(operation_kind) > 0),
                state TEXT NOT NULL CHECK(state IN ('active', 'recovering')),
                owner_pid INTEGER CHECK(owner_pid IS NULL OR owner_pid > 0),
                acquired_at INTEGER NOT NULL,
                recovery_detail TEXT,
                PRIMARY KEY(resource_kind, resource_id)
            );
            CREATE INDEX IF NOT EXISTS resource_operation_leases_operation
                ON resource_operation_leases(operation_id, state);

            CREATE TABLE IF NOT EXISTS codex_quota_snapshots (
                id INTEGER PRIMARY KEY,
                account_id TEXT NOT NULL REFERENCES codex_accounts(id) ON DELETE CASCADE,
                official_credential_revision_id TEXT NOT NULL REFERENCES official_credential_revisions(id) ON DELETE CASCADE,
                source TEXT NOT NULL DEFAULT 'official_app_server' CHECK(source = 'official_app_server'),
                primary_used_percent REAL CHECK(primary_used_percent IS NULL OR primary_used_percent BETWEEN 0 AND 100),
                primary_window_minutes INTEGER CHECK(primary_window_minutes IS NULL OR primary_window_minutes > 0),
                primary_resets_at INTEGER,
                secondary_used_percent REAL CHECK(secondary_used_percent IS NULL OR secondary_used_percent BETWEEN 0 AND 100),
                secondary_window_minutes INTEGER CHECK(secondary_window_minutes IS NULL OR secondary_window_minutes > 0),
                secondary_resets_at INTEGER,
                refreshed_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL,
                error_code TEXT,
                CHECK(expires_at >= refreshed_at)
            );
            CREATE INDEX IF NOT EXISTS codex_quota_snapshots_account_time
                ON codex_quota_snapshots(account_id, refreshed_at DESC);
            CREATE INDEX IF NOT EXISTS codex_quota_snapshots_revision_time
                ON codex_quota_snapshots(official_credential_revision_id, refreshed_at DESC);

            CREATE TRIGGER IF NOT EXISTS runtime_lock_matches_instance_launch_configuration
            BEFORE INSERT ON runtime_configuration_locks
            WHEN NOT EXISTS (
                SELECT 1 FROM codex_instances
                WHERE id = NEW.instance_id
                  AND launch_configuration_revision_id = NEW.launch_configuration_revision_id
            )
            BEGIN
                SELECT RAISE(ABORT, 'runtime lock launch configuration mismatch');
            END;

            CREATE TRIGGER IF NOT EXISTS codex_account_current_revision_matches_account
            BEFORE UPDATE OF current_credential_revision_id ON codex_accounts
            WHEN NEW.current_credential_revision_id IS NOT NULL AND NOT EXISTS (
                SELECT 1 FROM official_credential_revisions
                WHERE id = NEW.current_credential_revision_id AND account_id = NEW.id
            )
            BEGIN
                SELECT RAISE(ABORT, 'account current revision mismatch');
            END;",
        )
        .map_err(|error| format!("无法创建 schema v5：{error}"))?;

    add_column_if_missing(
        transaction,
        "api_key_providers",
        "current_revision_id",
        "ALTER TABLE api_key_providers ADD COLUMN current_revision_id TEXT REFERENCES api_key_provider_revisions(id) ON DELETE RESTRICT",
    )?;
    add_column_if_missing(
        transaction,
        "api_key_providers",
        "updated_at",
        "ALTER TABLE api_key_providers ADD COLUMN updated_at INTEGER",
    )?;

    // Only pre-v5 databases lack the revision contract. A current revision
    // being NULL is a valid tombstone after a provider has been removed, so
    // do not recreate or repoint it on every later application startup.
    if backfill_legacy_presets {
        transaction
            .execute_batch(
                "INSERT OR IGNORE INTO api_key_provider_revisions (
                id, provider_id, secret_ref, base_url, wire_api, transport_mode,
                state, created_at, retired_at
            )
            SELECT
                'v4-provider:' || id || ':revision:1',
                id,
                secret_ref,
                base_url,
                CASE transport_mode
                    WHEN 'gateway_chat_completions' THEN 'chat_completions'
                    ELSE 'responses'
                END,
                transport_mode,
                CASE configured WHEN 1 THEN 'active' ELSE 'credential_missing' END,
                created_at,
                NULL
            FROM api_key_providers;

            UPDATE api_key_providers
            SET current_revision_id = 'v4-provider:' || id || ':revision:1'
            WHERE current_revision_id IS NULL;

            UPDATE api_key_providers
            SET updated_at = created_at
            WHERE updated_at IS NULL;

            CREATE TRIGGER IF NOT EXISTS api_key_provider_current_revision_matches_provider
            BEFORE UPDATE OF current_revision_id ON api_key_providers
            WHEN NEW.current_revision_id IS NOT NULL AND NOT EXISTS (
                SELECT 1 FROM api_key_provider_revisions
                WHERE id = NEW.current_revision_id AND provider_id = NEW.id
            )
            BEGIN
                SELECT RAISE(ABORT, 'provider current revision mismatch');
            END;",
            )
            .map_err(|error| format!("无法回填 schema v5 Provider 修订：{error}"))?;
        transaction
            .execute_batch(
                "INSERT OR IGNORE INTO launch_presets (
                    id, name, credential_kind, api_key_provider_id, codex_account_id,
                    gateway_policy, profile_options, default_workspace_path, version,
                    is_preferred, created_at, updated_at
                )
                SELECT
                    'v4-provider:' || id || ':preset', name, 'api_key', id, NULL,
                    CASE transport_mode
                        WHEN 'direct_responses' THEN 'direct'
                        WHEN 'gateway_chat_completions' THEN 'gateway_chat_completions'
                        ELSE 'gateway_passthrough'
                    END, '{}', NULL, 1, is_default, created_at, created_at
                FROM api_key_providers;",
            )
            .map_err(|error| format!("无法回填旧版启动预设：{error}"))?;
    }

    Ok(())
}

pub(super) fn migrate_to_v6(transaction: &Transaction<'_>) -> Result<(), String> {
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS launch_preset_compositions (
                launch_preset_id TEXT PRIMARY KEY REFERENCES launch_presets(id) ON DELETE CASCADE,
                official_account_id TEXT REFERENCES codex_accounts(id) ON DELETE RESTRICT,
                state TEXT NOT NULL CHECK(state IN ('ready', 'needs_model_selection', 'incompatible')),
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                CHECK(updated_at >= created_at)
            );
            CREATE INDEX IF NOT EXISTS launch_preset_compositions_official_account
                ON launch_preset_compositions(official_account_id)
                WHERE official_account_id IS NOT NULL;

            CREATE TABLE IF NOT EXISTS launch_preset_api_providers (
                launch_preset_id TEXT NOT NULL REFERENCES launch_presets(id) ON DELETE CASCADE,
                provider_id TEXT NOT NULL REFERENCES api_key_providers(id) ON DELETE RESTRICT,
                position INTEGER NOT NULL CHECK(position >= 0),
                PRIMARY KEY(launch_preset_id, provider_id),
                UNIQUE(launch_preset_id, position)
            );
            CREATE INDEX IF NOT EXISTS launch_preset_api_providers_provider
                ON launch_preset_api_providers(provider_id, launch_preset_id);

            CREATE TABLE IF NOT EXISTS model_route_identities (
                id TEXT PRIMARY KEY CHECK(length(trim(id)) > 0),
                public_model_id TEXT NOT NULL UNIQUE
                    CHECK(length(public_model_id) BETWEEN 1 AND 128 AND instr(public_model_id, '/') = 0),
                source_kind TEXT NOT NULL CHECK(source_kind IN ('official', 'api_provider')),
                official_account_id TEXT REFERENCES codex_accounts(id) ON DELETE RESTRICT,
                api_key_provider_id TEXT REFERENCES api_key_providers(id) ON DELETE RESTRICT,
                upstream_model_id TEXT NOT NULL CHECK(length(trim(upstream_model_id)) > 0),
                created_at INTEGER NOT NULL,
                retired_at INTEGER,
                CHECK(
                    (source_kind = 'official' AND official_account_id IS NOT NULL AND api_key_provider_id IS NULL)
                    OR
                    (source_kind = 'api_provider' AND official_account_id IS NULL AND api_key_provider_id IS NOT NULL)
                ),
                CHECK(retired_at IS NULL OR retired_at >= created_at)
            );
            CREATE UNIQUE INDEX IF NOT EXISTS model_route_identities_official_source
                ON model_route_identities(official_account_id, upstream_model_id)
                WHERE source_kind = 'official';
            CREATE UNIQUE INDEX IF NOT EXISTS model_route_identities_provider_source
                ON model_route_identities(api_key_provider_id, upstream_model_id)
                WHERE source_kind = 'api_provider';

            CREATE TABLE IF NOT EXISTS launch_preset_model_selections (
                launch_preset_id TEXT NOT NULL REFERENCES launch_presets(id) ON DELETE CASCADE,
                route_identity_id TEXT NOT NULL REFERENCES model_route_identities(id) ON DELETE RESTRICT,
                position INTEGER NOT NULL CHECK(position >= 0),
                display_name_override TEXT,
                PRIMARY KEY(launch_preset_id, route_identity_id),
                UNIQUE(launch_preset_id, position),
                CHECK(display_name_override IS NULL OR length(trim(display_name_override)) > 0)
            );
            CREATE INDEX IF NOT EXISTS launch_preset_model_selections_identity
                ON launch_preset_model_selections(route_identity_id, launch_preset_id);

            CREATE TABLE IF NOT EXISTS provider_model_catalog_revisions (
                id TEXT PRIMARY KEY CHECK(length(trim(id)) > 0),
                provider_id TEXT NOT NULL REFERENCES api_key_providers(id) ON DELETE RESTRICT,
                provider_revision_id TEXT NOT NULL REFERENCES api_key_provider_revisions(id) ON DELETE RESTRICT,
                normalized_by TEXT NOT NULL CHECK(length(trim(normalized_by)) > 0),
                payload_sha256 TEXT NOT NULL CHECK(length(trim(payload_sha256)) > 0),
                state TEXT NOT NULL CHECK(state IN ('ready', 'failed')),
                discovered_at INTEGER NOT NULL,
                stale_after INTEGER,
                error_code TEXT,
                CHECK(stale_after IS NULL OR stale_after >= discovered_at),
                CHECK(
                    (state = 'ready' AND error_code IS NULL)
                    OR (state = 'failed' AND error_code IS NOT NULL)
                )
            );
            CREATE INDEX IF NOT EXISTS provider_model_catalog_revisions_provider_time
                ON provider_model_catalog_revisions(provider_id, discovered_at DESC);
            CREATE INDEX IF NOT EXISTS provider_model_catalog_revisions_revision_time
                ON provider_model_catalog_revisions(provider_revision_id, discovered_at DESC);

            CREATE TABLE IF NOT EXISTS provider_model_catalog_entries (
                catalog_revision_id TEXT NOT NULL REFERENCES provider_model_catalog_revisions(id) ON DELETE CASCADE,
                upstream_model_id TEXT NOT NULL CHECK(length(trim(upstream_model_id)) > 0),
                display_name TEXT NOT NULL CHECK(length(trim(display_name)) > 0),
                capabilities_json TEXT NOT NULL CHECK(length(trim(capabilities_json)) > 0),
                position INTEGER NOT NULL CHECK(position >= 0),
                PRIMARY KEY(catalog_revision_id, upstream_model_id),
                UNIQUE(catalog_revision_id, position)
            );

            CREATE TABLE IF NOT EXISTS gateway_service_revisions (
                id TEXT PRIMARY KEY CHECK(length(trim(id)) > 0),
                protocol_version INTEGER NOT NULL CHECK(protocol_version >= 1),
                bind_host TEXT NOT NULL DEFAULT '127.0.0.1' CHECK(bind_host = '127.0.0.1'),
                config_snapshot TEXT NOT NULL CHECK(length(trim(config_snapshot)) > 0),
                config_sha256 TEXT NOT NULL CHECK(length(trim(config_sha256)) > 0),
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS route_set_revisions (
                id TEXT PRIMARY KEY CHECK(length(trim(id)) > 0),
                launch_preset_id TEXT NOT NULL REFERENCES launch_presets(id) ON DELETE RESTRICT,
                launch_preset_version INTEGER NOT NULL CHECK(launch_preset_version >= 1),
                official_account_id TEXT REFERENCES codex_accounts(id) ON DELETE RESTRICT,
                official_credential_revision_id TEXT REFERENCES official_credential_revisions(id) ON DELETE RESTRICT,
                gateway_service_revision_id TEXT NOT NULL REFERENCES gateway_service_revisions(id) ON DELETE RESTRICT,
                requires_openai_auth INTEGER NOT NULL CHECK(requires_openai_auth IN (0, 1)),
                catalog_path TEXT,
                catalog_sha256 TEXT,
                state TEXT NOT NULL CHECK(state IN ('materializing', 'ready', 'failed')),
                created_at INTEGER NOT NULL,
                failure_code TEXT,
                CHECK(
                    (requires_openai_auth = 0 AND official_account_id IS NULL AND official_credential_revision_id IS NULL)
                    OR
                    (requires_openai_auth = 1 AND official_account_id IS NOT NULL AND official_credential_revision_id IS NOT NULL)
                ),
                CHECK(
                    state != 'ready'
                    OR (catalog_path IS NOT NULL AND length(trim(catalog_path)) > 0
                        AND catalog_sha256 IS NOT NULL AND length(trim(catalog_sha256)) > 0
                        AND failure_code IS NULL)
                ),
                CHECK(state != 'failed' OR failure_code IS NOT NULL)
            );
            CREATE INDEX IF NOT EXISTS route_set_revisions_preset_version
                ON route_set_revisions(launch_preset_id, launch_preset_version, created_at DESC);
            CREATE INDEX IF NOT EXISTS route_set_revisions_official_revision
                ON route_set_revisions(official_credential_revision_id, state)
                WHERE official_credential_revision_id IS NOT NULL;
            CREATE INDEX IF NOT EXISTS route_set_revisions_gateway_service
                ON route_set_revisions(gateway_service_revision_id, state);

            CREATE TABLE IF NOT EXISTS route_set_provider_revisions (
                route_set_revision_id TEXT NOT NULL REFERENCES route_set_revisions(id) ON DELETE CASCADE,
                provider_id TEXT NOT NULL REFERENCES api_key_providers(id) ON DELETE RESTRICT,
                provider_revision_id TEXT NOT NULL REFERENCES api_key_provider_revisions(id) ON DELETE RESTRICT,
                position INTEGER NOT NULL CHECK(position >= 0),
                connect_seconds INTEGER NOT NULL CHECK(connect_seconds BETWEEN 1 AND 120),
                first_byte_seconds INTEGER NOT NULL CHECK(first_byte_seconds BETWEEN 1 AND 600),
                stream_idle_seconds INTEGER NOT NULL CHECK(stream_idle_seconds BETWEEN 1 AND 1800),
                non_stream_total_seconds INTEGER NOT NULL CHECK(non_stream_total_seconds BETWEEN 1 AND 7200),
                PRIMARY KEY(route_set_revision_id, provider_id),
                UNIQUE(route_set_revision_id, provider_revision_id),
                UNIQUE(route_set_revision_id, position),
                CHECK(non_stream_total_seconds >= first_byte_seconds)
            );
            CREATE INDEX IF NOT EXISTS route_set_provider_revisions_provider
                ON route_set_provider_revisions(provider_id, provider_revision_id, route_set_revision_id);

            CREATE TABLE IF NOT EXISTS model_routes (
                id TEXT PRIMARY KEY CHECK(length(trim(id)) > 0),
                route_set_revision_id TEXT NOT NULL REFERENCES route_set_revisions(id) ON DELETE CASCADE,
                route_identity_id TEXT NOT NULL REFERENCES model_route_identities(id) ON DELETE RESTRICT,
                public_model_id TEXT NOT NULL CHECK(length(public_model_id) BETWEEN 1 AND 128 AND instr(public_model_id, '/') = 0),
                display_name TEXT NOT NULL CHECK(length(trim(display_name)) > 0),
                route_kind TEXT NOT NULL CHECK(route_kind IN ('official', 'api_provider')),
                official_account_id TEXT REFERENCES codex_accounts(id) ON DELETE RESTRICT,
                official_credential_revision_id TEXT REFERENCES official_credential_revisions(id) ON DELETE RESTRICT,
                api_key_provider_id TEXT REFERENCES api_key_providers(id) ON DELETE RESTRICT,
                api_key_provider_revision_id TEXT REFERENCES api_key_provider_revisions(id) ON DELETE RESTRICT,
                upstream_model_id TEXT NOT NULL CHECK(length(trim(upstream_model_id)) > 0),
                wire_api TEXT NOT NULL CHECK(wire_api IN ('responses', 'chat_completions')),
                adapter_kind TEXT NOT NULL CHECK(length(trim(adapter_kind)) > 0),
                adapter_version TEXT NOT NULL CHECK(length(trim(adapter_version)) > 0),
                capabilities_json TEXT NOT NULL CHECK(length(trim(capabilities_json)) > 0),
                position INTEGER NOT NULL CHECK(position >= 0),
                UNIQUE(route_set_revision_id, route_identity_id),
                UNIQUE(route_set_revision_id, public_model_id),
                UNIQUE(route_set_revision_id, position),
                CHECK(
                    (route_kind = 'official'
                        AND official_account_id IS NOT NULL
                        AND official_credential_revision_id IS NOT NULL
                        AND api_key_provider_id IS NULL
                        AND api_key_provider_revision_id IS NULL)
                    OR
                    (route_kind = 'api_provider'
                        AND official_account_id IS NULL
                        AND official_credential_revision_id IS NULL
                        AND api_key_provider_id IS NOT NULL
                        AND api_key_provider_revision_id IS NOT NULL)
                )
            );
            CREATE INDEX IF NOT EXISTS model_routes_source_provider
                ON model_routes(api_key_provider_id, api_key_provider_revision_id, route_set_revision_id)
                WHERE route_kind = 'api_provider';
            CREATE INDEX IF NOT EXISTS model_routes_source_official
                ON model_routes(official_account_id, official_credential_revision_id, route_set_revision_id)
                WHERE route_kind = 'official';

            CREATE TABLE IF NOT EXISTS launch_configuration_route_sets (
                launch_configuration_revision_id TEXT PRIMARY KEY REFERENCES launch_configuration_revisions(id) ON DELETE CASCADE,
                route_set_revision_id TEXT NOT NULL UNIQUE REFERENCES route_set_revisions(id) ON DELETE RESTRICT
            );

            CREATE TABLE IF NOT EXISTS instance_gateway_bindings (
                instance_id TEXT PRIMARY KEY REFERENCES codex_instances(id) ON DELETE RESTRICT,
                route_set_revision_id TEXT NOT NULL REFERENCES route_set_revisions(id) ON DELETE RESTRICT,
                gateway_service_revision_id TEXT NOT NULL REFERENCES gateway_service_revisions(id) ON DELETE RESTRICT,
                instance_key_ref TEXT NOT NULL UNIQUE CHECK(length(trim(instance_key_ref)) > 0),
                instance_key_hash TEXT NOT NULL UNIQUE CHECK(length(trim(instance_key_hash)) > 0),
                state TEXT NOT NULL CHECK(state IN ('active', 'revoked')),
                created_at INTEGER NOT NULL,
                revoked_at INTEGER,
                CHECK((state = 'revoked') = (revoked_at IS NOT NULL)),
                CHECK(revoked_at IS NULL OR revoked_at >= created_at)
            );
            CREATE INDEX IF NOT EXISTS instance_gateway_bindings_route_set_state
                ON instance_gateway_bindings(route_set_revision_id, state, instance_id);
            CREATE INDEX IF NOT EXISTS instance_gateway_bindings_gateway_state
                ON instance_gateway_bindings(gateway_service_revision_id, state, instance_id);",
        )
        .map_err(|error| format!("无法创建 schema v6 路由表：{error}"))?;

    transaction
        .execute_batch(
            "CREATE TRIGGER IF NOT EXISTS route_set_provider_revision_matches_provider
            BEFORE INSERT ON route_set_provider_revisions
            WHEN NOT EXISTS (
                SELECT 1 FROM api_key_provider_revisions
                WHERE id = NEW.provider_revision_id AND provider_id = NEW.provider_id
            )
            BEGIN
                SELECT RAISE(ABORT, 'route set provider revision mismatch');
            END;

            CREATE TRIGGER IF NOT EXISTS provider_catalog_revision_matches_provider
            BEFORE INSERT ON provider_model_catalog_revisions
            WHEN NOT EXISTS (
                SELECT 1 FROM api_key_provider_revisions
                WHERE id = NEW.provider_revision_id AND provider_id = NEW.provider_id
            )
            BEGIN
                SELECT RAISE(ABORT, 'model catalog provider revision mismatch');
            END;

            CREATE TRIGGER IF NOT EXISTS route_set_official_revision_matches_account
            BEFORE INSERT ON route_set_revisions
            WHEN NEW.requires_openai_auth = 1 AND NOT EXISTS (
                SELECT 1 FROM official_credential_revisions
                WHERE id = NEW.official_credential_revision_id
                  AND account_id = NEW.official_account_id
            )
            BEGIN
                SELECT RAISE(ABORT, 'route set official credential revision mismatch');
            END;

            CREATE TRIGGER IF NOT EXISTS model_route_identity_snapshot_matches
            BEFORE INSERT ON model_routes
            WHEN NOT EXISTS (
                SELECT 1 FROM model_route_identities identity
                WHERE identity.id = NEW.route_identity_id
                  AND identity.public_model_id = NEW.public_model_id
                  AND identity.source_kind = NEW.route_kind
                  AND identity.upstream_model_id = NEW.upstream_model_id
                  AND (
                    (NEW.route_kind = 'official' AND identity.official_account_id = NEW.official_account_id)
                    OR
                    (NEW.route_kind = 'api_provider' AND identity.api_key_provider_id = NEW.api_key_provider_id)
                  )
            )
            BEGIN
                SELECT RAISE(ABORT, 'model route identity mismatch');
            END;

            CREATE TRIGGER IF NOT EXISTS model_route_source_belongs_to_route_set
            BEFORE INSERT ON model_routes
            WHEN (
                NEW.route_kind = 'official' AND NOT EXISTS (
                    SELECT 1 FROM route_set_revisions route_set
                    WHERE route_set.id = NEW.route_set_revision_id
                      AND route_set.requires_openai_auth = 1
                      AND route_set.official_account_id = NEW.official_account_id
                      AND route_set.official_credential_revision_id = NEW.official_credential_revision_id
                )
            ) OR (
                NEW.route_kind = 'api_provider' AND NOT EXISTS (
                    SELECT 1 FROM route_set_provider_revisions provider
                    WHERE provider.route_set_revision_id = NEW.route_set_revision_id
                      AND provider.provider_id = NEW.api_key_provider_id
                      AND provider.provider_revision_id = NEW.api_key_provider_revision_id
                )
            )
            BEGIN
                SELECT RAISE(ABORT, 'model route source not in route set');
            END;

            CREATE TRIGGER IF NOT EXISTS launch_preset_selection_belongs_to_composition
            BEFORE INSERT ON launch_preset_model_selections
            WHEN NOT EXISTS (
                SELECT 1
                FROM launch_preset_compositions composition
                JOIN model_route_identities identity ON identity.id = NEW.route_identity_id
                WHERE composition.launch_preset_id = NEW.launch_preset_id
                  AND (
                    (identity.source_kind = 'official'
                        AND identity.official_account_id = composition.official_account_id)
                    OR
                    (identity.source_kind = 'api_provider' AND EXISTS (
                        SELECT 1 FROM launch_preset_api_providers provider
                        WHERE provider.launch_preset_id = NEW.launch_preset_id
                          AND provider.provider_id = identity.api_key_provider_id
                    ))
                  )
            )
            BEGIN
                SELECT RAISE(ABORT, 'model selection source not in preset composition');
            END;

            CREATE TRIGGER IF NOT EXISTS instance_gateway_binding_matches_route_set
            BEFORE INSERT ON instance_gateway_bindings
            WHEN NOT EXISTS (
                SELECT 1 FROM route_set_revisions
                WHERE id = NEW.route_set_revision_id
                  AND gateway_service_revision_id = NEW.gateway_service_revision_id
                  AND state = 'ready'
            )
            BEGIN
                SELECT RAISE(ABORT, 'instance gateway binding route set mismatch');
            END;",
        )
        .map_err(|error| format!("无法创建 schema v6 路由约束：{error}"))?;

    transaction
        .execute_batch(
            "INSERT OR IGNORE INTO launch_preset_compositions (
                launch_preset_id, official_account_id, state, created_at, updated_at
            )
            SELECT
                id,
                CASE credential_kind WHEN 'official' THEN codex_account_id ELSE NULL END,
                'needs_model_selection',
                created_at,
                updated_at
            FROM launch_presets;

            INSERT OR IGNORE INTO launch_preset_api_providers (
                launch_preset_id, provider_id, position
            )
            SELECT id, api_key_provider_id, 0
            FROM launch_presets
            WHERE credential_kind = 'api_key' AND api_key_provider_id IS NOT NULL;",
        )
        .map_err(|error| format!("无法回填 schema v6 预设组合：{error}"))?;

    rebuild_resource_tables_for_v6(transaction)?;
    Ok(())
}

fn rebuild_resource_tables_for_v6(transaction: &Transaction<'_>) -> Result<(), String> {
    const RESOURCE_KINDS: &str = "'launch_preset', 'api_key_provider', 'api_key_provider_revision', 'official_account', 'official_credential_revision', 'gateway', 'gateway_revision', 'profile', 'profile_revision', 'route_set_revision', 'model_route', 'model_route_identity', 'gateway_service_revision', 'model_catalog_revision', 'instance_gateway_binding'";
    let statements = format!(
        "DROP INDEX IF EXISTS launch_configuration_dependencies_resource;
        CREATE TABLE launch_configuration_dependencies_v6 (
            launch_configuration_revision_id TEXT NOT NULL REFERENCES launch_configuration_revisions(id) ON DELETE CASCADE,
            resource_kind TEXT NOT NULL CHECK(resource_kind IN ({RESOURCE_KINDS})),
            resource_id TEXT NOT NULL CHECK(length(resource_id) > 0),
            resource_revision_id TEXT,
            PRIMARY KEY(launch_configuration_revision_id, resource_kind, resource_id)
        );
        INSERT INTO launch_configuration_dependencies_v6
            SELECT launch_configuration_revision_id, resource_kind, resource_id, resource_revision_id
            FROM launch_configuration_dependencies;
        DROP TABLE launch_configuration_dependencies;
        ALTER TABLE launch_configuration_dependencies_v6 RENAME TO launch_configuration_dependencies;
        CREATE INDEX launch_configuration_dependencies_resource
            ON launch_configuration_dependencies(resource_kind, resource_id, resource_revision_id, launch_configuration_revision_id);

        DROP INDEX IF EXISTS resource_operation_leases_operation;
        CREATE TABLE resource_operation_leases_v6 (
            resource_kind TEXT NOT NULL CHECK(resource_kind IN ({RESOURCE_KINDS})),
            resource_id TEXT NOT NULL CHECK(length(resource_id) > 0),
            resource_revision_id TEXT,
            operation_id TEXT NOT NULL CHECK(length(operation_id) > 0),
            operation_kind TEXT NOT NULL CHECK(length(operation_kind) > 0),
            state TEXT NOT NULL CHECK(state IN ('active', 'recovering')),
            owner_pid INTEGER CHECK(owner_pid IS NULL OR owner_pid > 0),
            acquired_at INTEGER NOT NULL,
            recovery_detail TEXT,
            PRIMARY KEY(resource_kind, resource_id)
        );
        INSERT INTO resource_operation_leases_v6
            SELECT resource_kind, resource_id, resource_revision_id, operation_id,
                   operation_kind, state, owner_pid, acquired_at, recovery_detail
            FROM resource_operation_leases;
        DROP TABLE resource_operation_leases;
        ALTER TABLE resource_operation_leases_v6 RENAME TO resource_operation_leases;
        CREATE INDEX resource_operation_leases_operation
            ON resource_operation_leases(operation_id, state);"
    );
    transaction
        .execute_batch(&statements)
        .map_err(|error| format!("无法扩展 schema v6 资源种类：{error}"))
}
