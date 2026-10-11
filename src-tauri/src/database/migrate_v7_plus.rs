//! v7 到 v23 的增量迁移。
//!
//! 不处理 v5/v6 重建，不打开连接。
//! 由 `migrate` 按版本号调用。

use rusqlite::Transaction;

pub(super) fn migrate_to_v7(transaction: &Transaction<'_>) -> Result<(), String> {
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS gateway_request_diagnostics (
                id INTEGER PRIMARY KEY,
                instance_id TEXT REFERENCES codex_instances(id) ON DELETE CASCADE,
                session_id TEXT,
                created_at INTEGER NOT NULL,
                route TEXT NOT NULL,
                stream INTEGER NOT NULL CHECK(stream IN (0, 1)),
                wire_api TEXT NOT NULL CHECK(wire_api IN ('responses', 'chat_completions')),
                public_model_id TEXT NOT NULL,
                upstream_model_id TEXT NOT NULL,
                provider_id TEXT NOT NULL,
                provider_revision_id TEXT NOT NULL,
                http_status INTEGER,
                response_content_type TEXT,
                first_byte_ms INTEGER,
                total_latency_ms INTEGER,
                outcome TEXT NOT NULL CHECK(outcome IN ('success', 'timeout', 'interrupted', 'format_error', 'failed')),
                error_code TEXT,
                input_length INTEGER NOT NULL DEFAULT 0,
                output_length INTEGER NOT NULL DEFAULT 0,
                input_tokens INTEGER,
                output_tokens INTEGER,
                total_tokens INTEGER,
                input_hash TEXT,
                output_hash TEXT,
                tool_call_types TEXT,
                tool_result_hashes TEXT
            );
            CREATE INDEX IF NOT EXISTS gateway_request_diagnostics_instance_time
                ON gateway_request_diagnostics(instance_id, created_at DESC, id DESC);
            CREATE INDEX IF NOT EXISTS gateway_request_diagnostics_provider_time
                ON gateway_request_diagnostics(provider_id, created_at DESC, id DESC);
            CREATE TABLE IF NOT EXISTS conversation_summaries (
                id INTEGER PRIMARY KEY,
                instance_id TEXT NOT NULL REFERENCES codex_instances(id) ON DELETE CASCADE,
                session_id TEXT,
                created_at INTEGER NOT NULL,
                input_length INTEGER NOT NULL DEFAULT 0,
                output_length INTEGER NOT NULL DEFAULT 0,
                input_hash TEXT,
                output_hash TEXT,
                tool_call_types TEXT,
                tool_result_hashes TEXT
            );
            CREATE INDEX IF NOT EXISTS conversation_summaries_instance_time
                ON conversation_summaries(instance_id, created_at DESC, id DESC);",
        )
        .map_err(|error| format!("无法迁移请求级诊断表：{error}"))
}

pub(super) fn migrate_to_v8(transaction: &Transaction<'_>) -> Result<(), String> {
    add_column_if_missing(
        transaction,
        "gateway_request_diagnostics",
        "input_tokens",
        "ALTER TABLE gateway_request_diagnostics ADD COLUMN input_tokens INTEGER",
    )?;
    add_column_if_missing(
        transaction,
        "gateway_request_diagnostics",
        "output_tokens",
        "ALTER TABLE gateway_request_diagnostics ADD COLUMN output_tokens INTEGER",
    )?;
    add_column_if_missing(
        transaction,
        "gateway_request_diagnostics",
        "total_tokens",
        "ALTER TABLE gateway_request_diagnostics ADD COLUMN total_tokens INTEGER",
    )?;
    Ok(())
}

pub(super) fn migrate_to_v9(transaction: &Transaction<'_>) -> Result<(), String> {
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS launch_preset_role_models (
                launch_preset_id TEXT NOT NULL REFERENCES launch_presets(id) ON DELETE CASCADE,
                role TEXT NOT NULL CHECK(role IN ('auto_review')),
                route_identity_id TEXT NOT NULL REFERENCES model_route_identities(id) ON DELETE RESTRICT,
                PRIMARY KEY(launch_preset_id, role)
            );
            CREATE INDEX IF NOT EXISTS launch_preset_role_models_identity
                ON launch_preset_role_models(route_identity_id, launch_preset_id);
            CREATE TRIGGER IF NOT EXISTS launch_preset_role_model_must_be_selected_insert
            BEFORE INSERT ON launch_preset_role_models
            WHEN NOT EXISTS (
                SELECT 1 FROM launch_preset_model_selections selection
                WHERE selection.launch_preset_id=NEW.launch_preset_id
                  AND selection.route_identity_id=NEW.route_identity_id
            )
            BEGIN
                SELECT RAISE(ABORT, 'preset role model must be selected');
            END;
            CREATE TRIGGER IF NOT EXISTS launch_preset_role_model_must_be_selected_update
            BEFORE UPDATE OF launch_preset_id, route_identity_id ON launch_preset_role_models
            WHEN NOT EXISTS (
                SELECT 1 FROM launch_preset_model_selections selection
                WHERE selection.launch_preset_id=NEW.launch_preset_id
                  AND selection.route_identity_id=NEW.route_identity_id
            )
            BEGIN
                SELECT RAISE(ABORT, 'preset role model must be selected');
            END;
            CREATE TRIGGER IF NOT EXISTS launch_preset_selected_role_model_cannot_be_removed
            BEFORE DELETE ON launch_preset_model_selections
            WHEN EXISTS (
                SELECT 1 FROM launch_preset_role_models role_model
                WHERE role_model.launch_preset_id=OLD.launch_preset_id
                  AND role_model.route_identity_id=OLD.route_identity_id
            )
            BEGIN
                SELECT RAISE(ABORT, 'selected preset role model cannot be removed');
            END;",
        )
        .map_err(|error| format!("无法迁移运行方案角色绑定：{error}"))
}

pub(super) fn migrate_to_v10(transaction: &Transaction<'_>) -> Result<(), String> {
    add_column_if_missing(
        transaction,
        "launch_presets",
        "deleted_at",
        "ALTER TABLE launch_presets ADD COLUMN deleted_at INTEGER",
    )?;
    transaction
        .execute(
            "CREATE INDEX IF NOT EXISTS launch_presets_deleted_at
             ON launch_presets(deleted_at)",
            [],
        )
        .map_err(|error| format!("无法创建启动预设删除索引：{error}"))?;
    Ok(())
}

pub(super) fn migrate_to_v11(transaction: &Transaction<'_>) -> Result<(), String> {
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS launch_preset_runtime_bindings (
                launch_preset_id TEXT PRIMARY KEY REFERENCES launch_presets(id) ON DELETE CASCADE,
                instance_id TEXT NOT NULL UNIQUE REFERENCES codex_instances(id) ON DELETE RESTRICT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                CHECK(updated_at >= created_at)
            );
            CREATE INDEX IF NOT EXISTS launch_preset_runtime_bindings_instance
                ON launch_preset_runtime_bindings(instance_id);",
        )
        .map_err(|error| format!("无法迁移启动方案运行实例绑定：{error}"))
}

pub(super) fn migrate_to_v12(transaction: &Transaction<'_>) -> Result<(), String> {
    add_column_if_missing(
        transaction,
        "launch_presets",
        "entry_kind",
        "ALTER TABLE launch_presets ADD COLUMN entry_kind TEXT",
    )?;
    transaction
        .execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS launch_presets_entry_kind
             ON launch_presets(entry_kind) WHERE entry_kind IS NOT NULL;",
        )
        .map_err(|error| format!("无法创建启动入口配置索引：{error}"))
}

pub(super) fn migrate_to_v13(transaction: &Transaction<'_>) -> Result<(), String> {
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS compatibility_gateway_bindings (
                compatibility_id TEXT PRIMARY KEY,
                launch_preset_id TEXT NOT NULL REFERENCES launch_presets(id) ON DELETE RESTRICT,
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
            CREATE INDEX IF NOT EXISTS compatibility_gateway_bindings_state
                ON compatibility_gateway_bindings(state, created_at DESC);
            CREATE INDEX IF NOT EXISTS compatibility_gateway_bindings_preset
                ON compatibility_gateway_bindings(launch_preset_id, state);",
        )
        .map_err(|error| format!("无法迁移默认 App 兼容绑定：{error}"))
}

pub(super) fn migrate_to_v14(transaction: &Transaction<'_>) -> Result<(), String> {
    // An empty workspace is an intentional editing state after the last
    // provider/model is removed. Extend the preset projection to represent
    // that state without retaining a foreign-key reference to a deleted
    // provider. No launch configuration is ever materialized for `none`.
    transaction
        .execute_batch(
            "CREATE TABLE launch_presets_v14 (
                id TEXT PRIMARY KEY,
                entry_kind TEXT,
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
            INSERT INTO launch_presets_v14 (
                id, entry_kind, name, credential_kind, api_key_provider_id,
                codex_account_id, gateway_policy, profile_options,
                default_workspace_path, version, is_preferred,
                created_at, updated_at, deleted_at
            ) SELECT id, entry_kind, name, credential_kind, api_key_provider_id,
                codex_account_id, gateway_policy, profile_options,
                default_workspace_path, version, is_preferred,
                created_at, updated_at, deleted_at
                FROM launch_presets;
            DROP TABLE launch_presets;
            ALTER TABLE launch_presets_v14 RENAME TO launch_presets;
            CREATE UNIQUE INDEX IF NOT EXISTS one_preferred_launch_preset
                ON launch_presets(is_preferred) WHERE is_preferred = 1;
            CREATE UNIQUE INDEX IF NOT EXISTS launch_presets_entry_kind
                ON launch_presets(entry_kind) WHERE entry_kind IS NOT NULL;
            CREATE INDEX IF NOT EXISTS launch_presets_deleted_at
                ON launch_presets(deleted_at);
            CREATE INDEX IF NOT EXISTS launch_presets_api_key_provider
                ON launch_presets(api_key_provider_id);
            CREATE INDEX IF NOT EXISTS launch_presets_codex_account
                ON launch_presets(codex_account_id);",
        )
        .map_err(|error| format!("无法迁移空工作区凭据状态：{error}"))
}

pub(super) fn migrate_to_v15(transaction: &Transaction<'_>) -> Result<(), String> {
    add_column_if_missing(
        transaction,
        "gateway_request_diagnostics",
        "error_detail",
        "ALTER TABLE gateway_request_diagnostics ADD COLUMN error_detail TEXT",
    )
}

pub(super) fn migrate_to_v16(transaction: &Transaction<'_>) -> Result<(), String> {
    add_column_if_missing(
        transaction,
        "launch_preset_model_selections",
        "context_window",
        "ALTER TABLE launch_preset_model_selections ADD COLUMN context_window INTEGER",
    )?;
    add_column_if_missing(
        transaction,
        "launch_preset_model_selections",
        "auto_compact_token_limit",
        "ALTER TABLE launch_preset_model_selections ADD COLUMN auto_compact_token_limit INTEGER",
    )
}

pub(super) fn migrate_to_v17(transaction: &Transaction<'_>) -> Result<(), String> {
    migrate_to_v12(transaction)?;
    migrate_to_v13(transaction)?;
    // Retire only 3R's bookkeeping. Never inspect old homes or signal their processes.
    transaction.execute_batch(
        "CREATE TEMP TABLE retired_compat_instances AS
         SELECT instance.id FROM codex_instances instance
         JOIN launch_configuration_revisions configuration ON configuration.id=instance.launch_configuration_revision_id
         LEFT JOIN launch_presets preset ON preset.id=configuration.launch_preset_id
         WHERE COALESCE(json_extract(configuration.config_snapshot, '$.launchTarget'), preset.entry_kind)
           IN ('compat_app', 'compat_cli');
         UPDATE runtime_configuration_locks SET released_at=unixepoch()
         WHERE released_at IS NULL AND instance_id IN (SELECT id FROM retired_compat_instances);
         UPDATE codex_instances SET state='exited', exited_at=COALESCE(exited_at, unixepoch())
         WHERE id IN (SELECT id FROM retired_compat_instances) AND state NOT IN ('exited','failed');
         UPDATE compatibility_gateway_bindings SET state='revoked', revoked_at=MAX(created_at, unixepoch())
         WHERE state='active';"
    ).map_err(|error| format!("无法停用旧兼容模式记录：{error}"))?;
    let has_bindings: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='instance_gateway_bindings')",
        [], |row| row.get(0),
    ).map_err(|error| format!("无法检查旧兼容模式绑定：{error}"))?;
    if has_bindings {
        transaction.execute_batch(
            "UPDATE instance_gateway_bindings SET state='revoked', revoked_at=MAX(created_at, unixepoch())
             WHERE state='active' AND instance_id IN (SELECT id FROM retired_compat_instances);"
        ).map_err(|error| format!("无法停用旧兼容模式绑定：{error}"))?;
    }
    transaction
        .execute_batch("DROP TABLE retired_compat_instances;")
        .map_err(|error| format!("无法结束旧兼容模式迁移：{error}"))
}

pub(super) fn migrate_to_v18(transaction: &Transaction<'_>) -> Result<(), String> {
    add_column_if_missing(
        transaction,
        "launch_preset_model_selections",
        "reasoning_levels_json",
        "ALTER TABLE launch_preset_model_selections ADD COLUMN reasoning_levels_json TEXT",
    )?;
    add_column_if_missing(
        transaction,
        "launch_preset_model_selections",
        "default_reasoning_level",
        "ALTER TABLE launch_preset_model_selections ADD COLUMN default_reasoning_level TEXT",
    )
}

pub(super) fn migrate_to_v19(transaction: &Transaction<'_>) -> Result<(), String> {
    add_column_if_missing(
        transaction,
        "provider_connectivity_reports",
        "wire_api",
        "ALTER TABLE provider_connectivity_reports ADD COLUMN wire_api TEXT",
    )?;
    add_column_if_missing(
        transaction,
        "provider_connectivity_reports",
        "catalog_revision_id",
        "ALTER TABLE provider_connectivity_reports ADD COLUMN catalog_revision_id TEXT",
    )
}

pub(super) fn migrate_to_v20(transaction: &Transaction<'_>) -> Result<(), String> {
    // Only rename the exact system-generated preset. User-authored preset
    // names remain untouched, even when they happen to mention the old brand.
    transaction
        .execute(
            "UPDATE launch_presets SET name='R Codex' WHERE name='3R Codex'",
            [],
        )
        .map_err(|error| format!("无法更新 R Codex 启动方案显示名：{error}"))?;
    Ok(())
}

pub(super) fn migrate_to_v21(transaction: &Transaction<'_>) -> Result<(), String> {
    // Multiple isolated App instances are separate managed_app presets.
    // The historical unique index allowed only one row per entry_kind.
    transaction
        .execute("DROP INDEX IF EXISTS launch_presets_entry_kind", [])
        .map_err(|error| format!("无法删除启动方案入口唯一索引：{error}"))?;
    Ok(())
}

pub(super) fn migrate_to_v22(
    transaction: &Transaction<'_>,
    fresh_migration: bool,
) -> Result<(), String> {
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS provider_credentials (
                secret_ref TEXT PRIMARY KEY,
                api_key TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );",
        )
        .map_err(|error| format!("无法创建 Provider 凭据表：{error}"))?;
    if fresh_migration {
        // Existing metadata may point at credentials that lived in Keychain.
        // Do not enumerate or migrate those values: every Provider must be
        // explicitly re-entered into the new SQLite-backed store.
        transaction
            .execute(
                "UPDATE api_key_providers SET configured=0 WHERE configured=1",
                [],
            )
            .map_err(|error| format!("无法重置 Provider 凭据状态：{error}"))?;
        transaction
            .execute(
                "UPDATE api_key_provider_revisions SET state='credential_missing' WHERE state='active'",
                [],
            )
            .map_err(|error| format!("无法标记缺少凭据的 Provider revision：{error}"))?;
    }
    Ok(())
}

pub(super) fn migrate_to_v23(transaction: &Transaction<'_>) -> Result<(), String> {
    // DirectResponses is retired. Keep the columns and CHECK tokens so this
    // is a data rewrite, not a table rebuild. Launch always uses Gateway;
    // Chat vs Responses stays on the provider revision wire_api.
    for (table, column, from, to) in [
        (
            "launch_presets",
            "gateway_policy",
            "direct",
            "gateway_passthrough",
        ),
        (
            "api_key_providers",
            "transport_mode",
            "direct_responses",
            "gateway_passthrough",
        ),
        (
            "api_key_provider_revisions",
            "transport_mode",
            "direct_responses",
            "gateway_passthrough",
        ),
    ] {
        let table_exists: bool = transaction
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .map_err(|error| format!("无法检查 {table} 是否存在：{error}"))?;
        if !table_exists {
            continue;
        }
        let sql = format!("UPDATE {table} SET {column}=?1 WHERE {column}=?2");
        transaction
            .execute(&sql, [to, from])
            .map_err(|error| format!("无法迁移 {table}.{column}：{error}"))?;
    }
    Ok(())
}

pub(super) fn add_column_if_missing(
    transaction: &Transaction<'_>,
    table: &str,
    column: &str,
    statement: &str,
) -> Result<(), String> {
    // Partial fixtures (and CREATE IF NOT EXISTS later in migrate) may not have
    // every later table yet. pragma_table_info on a missing table is empty, so
    // ALTER would fail with "no such table".
    let table_exists: bool = transaction
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        )
        .map_err(|error| format!("无法检查 {table} 是否存在：{error}"))?;
    if !table_exists {
        return Ok(());
    }
    let query = format!("SELECT COUNT(*) > 0 FROM pragma_table_info('{table}') WHERE name = ?1");
    let exists: bool = transaction
        .query_row(&query, [column], |row| row.get(0))
        .map_err(|error| format!("无法检查 {table}.{column}：{error}"))?;
    if exists {
        return Ok(());
    }
    transaction
        .execute(statement, [])
        .map_err(|error| format!("无法新增 {table}.{column}：{error}"))?;
    Ok(())
}
