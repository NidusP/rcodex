//! 工作区写入：创建/选择启动方案、登录渠道、composition 摘要。
//!
//! 不准备 runtime binding，不改隔离 home。运行中的实例仍可写入，下次启动才生效。
//! 由启动方案 IPC 和 Store 内部调用。

use std::collections::BTreeSet;

use rusqlite::{params, OptionalExtension, TransactionBehavior};
use uuid::Uuid;

use crate::codex_control::EntryKind;
use crate::routing::RouteKind;
use crate::support::unix_now_seconds;

use super::models::{
    CreateManagedLaunchPresetInput, ManagedLaunchStore, ManagedPresetCompositionSummary,
    ManagedPresetModelSummary, ManagedPresetSaved, ProviderSnapshot,
};
use super::persistence::query_strings;
use super::preset_validation::{
    assert_providers_have_confirmed_protocol, normalize_public_model_ids, upsert_identity,
    validate_model, validate_sources,
};
use super::validation::{normalize_optional_path, validate_name};

pub(super) fn login_provider_id_from_profile_options(raw: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| {
            value
                .get("loginProviderId")
                .and_then(|item| item.as_str())
                .map(|item| item.trim().to_string())
        })
        .filter(|item| !item.is_empty())
}

pub(super) fn merge_login_provider_profile_options(
    raw: &str,
    provider_id: Option<&str>,
) -> Result<String, String> {
    let mut options = match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(serde_json::Value::Object(map)) => serde_json::Value::Object(map),
        Ok(_) | Err(_) if raw.trim().is_empty() || raw.trim() == "{}" => serde_json::json!({}),
        Ok(_) => return Err("managed_profile_options_invalid".to_string()),
        Err(_) => return Err("managed_profile_options_invalid".to_string()),
    };
    let map = options
        .as_object_mut()
        .ok_or_else(|| "managed_profile_options_invalid".to_string())?;
    match provider_id.map(str::trim).filter(|item| !item.is_empty()) {
        Some(id) => {
            map.insert(
                "loginProviderId".to_string(),
                serde_json::Value::String(id.to_string()),
            );
        }
        None => {
            map.remove("loginProviderId");
        }
    }
    serde_json::to_string(&options)
        .map_err(|error| format!("managed_profile_options_encode_failed:{error}"))
}

pub(super) fn effective_login_provider_id(
    profile_options: &str,
    providers: &[ProviderSnapshot],
) -> Result<String, String> {
    if let Some(id) = login_provider_id_from_profile_options(profile_options) {
        if providers.iter().any(|provider| provider.provider_id == id) {
            return Ok(id);
        }
        return Err("login_provider_not_in_workspace".to_string());
    }
    if providers.len() == 1 {
        return Ok(providers[0].provider_id.clone());
    }
    Err("login_provider_required".to_string())
}

pub(super) fn qualified_model_display_name(provider_name: &str, display_name: &str) -> String {
    let provider_name = provider_name.trim();
    let prefixes = [format!("{provider_name}/"), format!("{provider_name} /")];
    let mut model_name = display_name.trim();
    loop {
        if let Some(stripped) = prefixes
            .iter()
            .find_map(|prefix| model_name.strip_prefix(prefix))
        {
            model_name = stripped.trim();
        } else {
            break;
        }
    }
    format!("{provider_name}/{model_name}")
}

impl ManagedLaunchStore {
    /// Seed the one product-owned App workspace before the UI takes its first
    /// snapshot. It deliberately has no credentials or models: selecting a
    /// channel remains an explicit user action in the workspace UI.
    pub(super) fn ensure_initial_workspace(&self) -> Result<(), String> {
        let now = unix_now_seconds();
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "managed_launch_database_lock_failed".to_string())?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("initial_workspace_transaction_failed:{error}"))?;
        let exists: bool = transaction
            .query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM launch_presets
                   WHERE entry_kind='managed_app' AND deleted_at IS NULL
                 )",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("initial_workspace_lookup_failed:{error}"))?;
        if !exists {
            // The schema permits exactly one preferred preset. A fresh
            // workspace has none; clear a stale preference before creating
            // the product-owned App workspace.
            transaction
                .execute(
                    "UPDATE launch_presets SET is_preferred=0 WHERE is_preferred=1",
                    [],
                )
                .map_err(|error| format!("initial_workspace_preference_reset_failed:{error}"))?;
            let id = Uuid::new_v4().to_string();
            transaction
                .execute(
                    "INSERT INTO launch_presets (
                        id, entry_kind, name, credential_kind, api_key_provider_id,
                        codex_account_id, gateway_policy, profile_options,
                        default_workspace_path, version, is_preferred,
                        created_at, updated_at
                     ) VALUES (?1, 'managed_app', 'R Codex', 'none', NULL, NULL,
                        NULL, '{}', NULL, 1, 1, ?2, ?2)",
                    params![id, now],
                )
                .map_err(|error| format!("initial_workspace_insert_failed:{error}"))?;
            transaction
                .execute(
                    "INSERT INTO launch_preset_compositions (
                        launch_preset_id, official_account_id, state, created_at, updated_at
                     ) VALUES (?1, NULL, 'needs_model_selection', ?2, ?2)",
                    params![id, now],
                )
                .map_err(|error| format!("initial_workspace_composition_insert_failed:{error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("initial_workspace_commit_failed:{error}"))
    }

    pub fn create_preset(
        &self,
        mut input: CreateManagedLaunchPresetInput,
    ) -> Result<ManagedPresetSaved, String> {
        if input.entry_kind.is_none() {
            input.entry_kind = Some("managed_app".to_string());
        }
        if !EntryKind::is_managed_workspace(input.entry_kind.as_deref()) {
            return Err("invalid_launch_target".to_string());
        }
        let name = validate_name(&input.name)?;
        let provider_ids = validate_sources(
            input.official_account_id.as_deref(),
            input.api_key_provider_ids,
            input.models.is_empty(),
        )?;
        // Removing the final channel is a valid editing state. Keep the
        // workspace record, but mark it incomplete so it cannot be launched
        // until the user selects a provider and model again.
        let composition_state = if input.models.is_empty() {
            "needs_model_selection"
        } else {
            "ready"
        };
        let mut auto_review_public_model_id = input
            .auto_review_public_model_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        let now = unix_now_seconds();
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "managed_launch_database_lock_failed".to_string())?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("managed_preset_transaction_failed:{error}"))?;

        assert_providers_have_confirmed_protocol(&transaction, &provider_ids)?;
        let public_id_remaps = normalize_public_model_ids(&transaction, &mut input.models)?;
        if let Some(public_model_id) = auto_review_public_model_id.as_mut() {
            if let Some(remapped_id) = public_id_remaps.get(public_model_id) {
                *public_model_id = remapped_id.clone();
            }
        }

        // Editing updates one managed_app preset. create_new inserts another
        // isolated instance; launch_preset_id targets a specific row. The
        // default lookup still prefers the selected (is_preferred) workspace
        // so older clients keep writing the current instance.
        if input.create_new && input.launch_preset_id.is_some() {
            return Err("invalid_launch_preset".to_string());
        }
        let existing_workspace: Option<(String, i64)> = if input.create_new {
            None
        } else if let Some(preset_id) = input
            .launch_preset_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(
                transaction
                    .query_row(
                        "SELECT id, version FROM launch_presets
                         WHERE id=?1 AND deleted_at IS NULL
                           AND (?2 IS NULL OR entry_kind = ?2)",
                        params![preset_id, input.entry_kind.as_deref()],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()
                    .map_err(|error| format!("managed_workspace_lookup_failed:{error}"))?
                    .ok_or_else(|| "launch_preset_not_found".to_string())?,
            )
        } else {
            transaction
                .query_row(
                    "SELECT id, version FROM launch_presets
                     WHERE deleted_at IS NULL
                       AND (?1 IS NULL OR entry_kind = ?1)
                     ORDER BY is_preferred DESC, created_at ASC
                     LIMIT 1",
                    [input.entry_kind.as_deref()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|error| format!("managed_workspace_lookup_failed:{error}"))?
        };
        if let Some((workspace_id, _)) = existing_workspace.as_ref() {
            // Running instances keep the launch snapshot. Workspace edits are
            // allowed and apply the next time this isolated instance starts.
            let leased: bool = transaction
                .query_row(
                    "SELECT EXISTS(
                       SELECT 1 FROM resource_operation_leases
                       WHERE resource_kind='launch_preset' AND resource_id=?1
                    )",
                    [workspace_id],
                    |row| row.get(0),
                )
                .map_err(|error| format!("managed_workspace_lease_check_failed:{error}"))?;
            if leased {
                return Err("resource_operation_in_progress".to_string());
            }
        }
        let previous_provider_ids: Vec<String> = match existing_workspace.as_ref() {
            Some((workspace_id, _)) => {
                let mut statement = transaction
                    .prepare(
                        "SELECT provider_id FROM launch_preset_api_providers
                         WHERE launch_preset_id=?1 ORDER BY position",
                    )
                    .map_err(|error| format!("managed_provider_lookup_failed:{error}"))?;
                let rows = statement
                    .query_map([workspace_id.as_str()], |row| row.get(0))
                    .map_err(|error| format!("managed_provider_lookup_failed:{error}"))?;
                rows.collect::<Result<Vec<String>, _>>()
                    .map_err(|error| format!("managed_provider_lookup_failed:{error}"))?
            }
            None => Vec::new(),
        };

        for provider_id in &provider_ids {
            let available: bool = transaction
                .query_row(
                    "SELECT EXISTS(
                       SELECT 1 FROM api_key_providers provider
                       JOIN api_key_provider_revisions revision
                         ON revision.id=provider.current_revision_id
                       WHERE provider.id=?1 AND revision.state='active'
                    )",
                    [provider_id],
                    |row| row.get(0),
                )
                .map_err(|error| format!("provider_lookup_failed:{error}"))?;
            if !available {
                return Err("credential_missing".to_string());
            }
        }
        if let Some(account_id) = input.official_account_id.as_deref() {
            let available: bool = transaction
                .query_row(
                    "SELECT EXISTS(
                       SELECT 1 FROM codex_accounts account WHERE account.id=?1
                       AND NOT EXISTS (
                         SELECT 1 FROM official_credential_revisions deleted
                         WHERE deleted.account_id=account.id
                           AND deleted.auth_error_code='official_account_deleted'
                       )
                    )",
                    [account_id],
                    |row| row.get(0),
                )
                .map_err(|error| format!("official_account_lookup_failed:{error}"))?;
            if !available {
                return Err("credential_missing".to_string());
            }
        }

        let selected_providers = provider_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let mut public_ids = BTreeSet::new();
        let mut source_models = BTreeSet::new();
        for model in &input.models {
            validate_model(model)?;
            if !public_ids.insert(model.public_model_id.as_str())
                || !source_models.insert((
                    model.source_kind.as_db(),
                    model.source_id.as_str(),
                    model.upstream_model_id.as_str(),
                ))
            {
                return Err("duplicate_model_selection".to_string());
            }
            match model.source_kind {
                RouteKind::ApiProvider if selected_providers.contains(model.source_id.as_str()) => {
                }
                RouteKind::Official
                    if input.official_account_id.as_deref() == Some(model.source_id.as_str()) => {}
                _ => return Err("route_source_not_selected".to_string()),
            }
        }

        let auto_review_model = auto_review_public_model_id
            .as_deref()
            .map(|public_model_id| {
                input
                    .models
                    .iter()
                    .find(|model| model.public_model_id == public_model_id)
                    .ok_or_else(|| "auto_review_model_not_selected".to_string())
            })
            .transpose()?;
        if let Some(model) = auto_review_model {
            if model.source_kind != RouteKind::ApiProvider {
                return Err("auto_review_model_requires_api_provider".to_string());
            }
            let wire_api = transaction
                .query_row(
                    "SELECT revision.wire_api
                     FROM api_key_providers provider
                     JOIN api_key_provider_revisions revision
                       ON revision.id=provider.current_revision_id
                     WHERE provider.id=?1 AND revision.state='active'",
                    [model.source_id.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|error| format!("managed_provider_transport_read_failed:{error}"))?;
            if wire_api.as_deref() != Some("responses") {
                return Err("auto_review_model_requires_responses".to_string());
            }
        }

        transaction
            .execute("UPDATE launch_presets SET is_preferred=0", [])
            .map_err(|error| format!("preferred_preset_update_failed:{error}"))?;
        let _ = input.transport_mode.as_deref();
        let gateway_policy = "gateway_passthrough";
        let (credential_kind, projected_provider, projected_account, gateway_policy) =
            if input.models.is_empty()
                && input.official_account_id.is_none()
                && provider_ids.is_empty()
            {
                ("none", None, None, None)
            } else if let Some(account_id) = input.official_account_id.as_deref() {
                ("official", None, Some(account_id), None)
            } else {
                (
                    "api_key",
                    provider_ids.first().map(String::as_str),
                    None,
                    Some(gateway_policy),
                )
            };
        let (id, version) = if let Some((workspace_id, current_version)) = existing_workspace {
            let next_version = current_version
                .checked_add(1)
                .ok_or_else(|| "managed_workspace_version_overflow".to_string())?;
            transaction
                .execute(
                    "UPDATE launch_presets
                     SET entry_kind=?1, name=?2, credential_kind=?3, api_key_provider_id=?4,
                         codex_account_id=?5, gateway_policy=?6,
                         default_workspace_path=?7, version=?8, is_preferred=1,
                         updated_at=?9
                     WHERE id=?10 AND deleted_at IS NULL",
                    params![
                        input.entry_kind,
                        name,
                        credential_kind,
                        projected_provider,
                        projected_account,
                        gateway_policy,
                        normalize_optional_path(input.default_workspace_path.as_deref()),
                        next_version,
                        now,
                        workspace_id,
                    ],
                )
                .map_err(|error| format!("managed_preset_update_failed:{error}"))?;
            for table in [
                "launch_preset_role_models",
                "launch_preset_model_selections",
                "launch_preset_api_providers",
                "launch_preset_compositions",
            ] {
                transaction
                    .execute(
                        &format!("DELETE FROM {table} WHERE launch_preset_id=?1"),
                        [&workspace_id],
                    )
                    .map_err(|error| format!("managed_composition_replace_failed:{error}"))?;
            }
            (workspace_id, next_version)
        } else {
            let workspace_id = Uuid::new_v4().to_string();
            transaction
                .execute(
                    "INSERT INTO launch_presets (
                        id, entry_kind, name, credential_kind, api_key_provider_id,
                        codex_account_id, gateway_policy, profile_options,
                        default_workspace_path, version, is_preferred,
                        created_at, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '{}', ?8, 1, 1, ?9, ?9)",
                    params![
                        workspace_id,
                        input.entry_kind,
                        name,
                        credential_kind,
                        projected_provider,
                        projected_account,
                        gateway_policy,
                        normalize_optional_path(input.default_workspace_path.as_deref()),
                        now,
                    ],
                )
                .map_err(|error| format!("managed_preset_insert_failed:{error}"))?;
            (workspace_id, 1)
        };
        let profile_options: String = transaction
            .query_row(
                "SELECT COALESCE(profile_options, '{}') FROM launch_presets WHERE id=?1",
                [&id],
                |row| row.get(0),
            )
            .map_err(|_| "managed_preset_lookup_failed".to_string())?;
        let login_provider = login_provider_id_from_profile_options(&profile_options)
            .filter(|provider| provider_ids.contains(provider));
        // A workspace with one channel shows that channel as the login even
        // before it is stored. Adding another channel must keep that choice.
        let login_provider = login_provider.or_else(|| {
            if previous_provider_ids.len() == 1
                && provider_ids
                    .iter()
                    .any(|id| id == &previous_provider_ids[0])
            {
                Some(previous_provider_ids[0].clone())
            } else {
                None
            }
        });
        let profile_options =
            merge_login_provider_profile_options(&profile_options, login_provider.as_deref())?;
        transaction
            .execute(
                "UPDATE launch_presets SET profile_options=?1 WHERE id=?2",
                params![profile_options, id],
            )
            .map_err(|_| "managed_preset_update_failed".to_string())?;
        transaction
            .execute(
                "INSERT INTO launch_preset_compositions (
                    launch_preset_id, official_account_id, state, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?4)",
                params![id, input.official_account_id, composition_state, now],
            )
            .map_err(|error| format!("managed_composition_insert_failed:{error}"))?;
        for (position, provider_id) in provider_ids.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO launch_preset_api_providers (
                        launch_preset_id, provider_id, position
                     ) VALUES (?1, ?2, ?3)",
                    params![id, provider_id, position as i64],
                )
                .map_err(|error| format!("managed_provider_binding_insert_failed:{error}"))?;
        }
        let mut auto_review_identity_id = None;
        for (position, model) in input.models.iter().enumerate() {
            let identity_id = upsert_identity(&transaction, model, now)?;
            transaction
                .execute(
                    "INSERT INTO launch_preset_model_selections (
                        launch_preset_id, route_identity_id, position, display_name_override,
                        context_window, auto_compact_token_limit, reasoning_levels_json,
                        default_reasoning_level
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        id,
                        identity_id,
                        position as i64,
                        model.display_name,
                        model.context_window.map(|value| value as i64),
                        model.auto_compact_token_limit.map(|value| value as i64),
                        (!model.reasoning_levels.is_empty())
                            .then(|| serde_json::to_string(&model.reasoning_levels))
                            .transpose()
                            .map_err(|_| "managed_reasoning_levels_encode_failed".to_string())?,
                        model.default_reasoning_level,
                    ],
                )
                .map_err(|error| format!("managed_model_selection_insert_failed:{error}"))?;
            if auto_review_public_model_id.as_deref() == Some(model.public_model_id.as_str()) {
                auto_review_identity_id = Some(identity_id);
            }
        }
        if let Some(identity_id) = auto_review_identity_id {
            transaction
                .execute(
                    "INSERT INTO launch_preset_role_models (
                        launch_preset_id, role, route_identity_id
                     ) VALUES (?1, 'auto_review', ?2)",
                    params![id, identity_id],
                )
                .map_err(|error| format!("managed_role_model_insert_failed:{error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("managed_preset_commit_failed:{error}"))?;
        Ok(ManagedPresetSaved { id, version })
    }

    pub fn select_preset(&self, launch_preset_id: &str) -> Result<ManagedPresetSaved, String> {
        let now = unix_now_seconds();
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "managed_launch_database_lock_failed".to_string())?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("managed_preset_transaction_failed:{error}"))?;
        let (entry_kind, version, is_preferred): (Option<String>, i64, i64) = transaction
            .query_row(
                "SELECT entry_kind, version, is_preferred
                 FROM launch_presets
                 WHERE id=?1 AND deleted_at IS NULL",
                [launch_preset_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|error| format!("managed_preset_lookup_failed:{error}"))?
            .ok_or_else(|| "launch_preset_not_found".to_string())?;
        if !EntryKind::is_managed_workspace(entry_kind.as_deref()) {
            return Err("invalid_launch_target".to_string());
        }
        if is_preferred == 1 {
            return Ok(ManagedPresetSaved {
                id: launch_preset_id.to_string(),
                version,
            });
        }
        transaction
            .execute("UPDATE launch_presets SET is_preferred=0", [])
            .map_err(|error| format!("preferred_preset_update_failed:{error}"))?;
        let updated = transaction
            .execute(
                "UPDATE launch_presets SET is_preferred=1, updated_at=?1
                 WHERE id=?2 AND deleted_at IS NULL",
                params![now, launch_preset_id],
            )
            .map_err(|error| format!("preferred_preset_update_failed:{error}"))?;
        if updated == 0 {
            return Err("launch_preset_not_found".to_string());
        }
        transaction
            .commit()
            .map_err(|error| format!("managed_preset_commit_failed:{error}"))?;
        Ok(ManagedPresetSaved {
            id: launch_preset_id.to_string(),
            version,
        })
    }

    pub fn set_managed_login_provider(
        &self,
        launch_preset_id: &str,
        expected_version: i64,
        provider_id: Option<&str>,
    ) -> Result<ManagedPresetSaved, String> {
        let now = unix_now_seconds();
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "managed_launch_database_lock_failed".to_string())?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("managed_preset_transaction_failed:{error}"))?;
        let (entry_kind, version, profile_options): (Option<String>, i64, String) = transaction
            .query_row(
                "SELECT entry_kind, version, COALESCE(profile_options, '{}')
                 FROM launch_presets
                 WHERE id=?1 AND deleted_at IS NULL",
                [launch_preset_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|error| format!("managed_preset_lookup_failed:{error}"))?
            .ok_or_else(|| "launch_preset_not_found".to_string())?;
        if !EntryKind::is_managed_workspace(entry_kind.as_deref()) {
            return Err("login_provider_requires_managed_app".to_string());
        }
        if version != expected_version {
            return Err("version_conflict".to_string());
        }
        // Login channel is workspace config. A running instance keeps its
        // launch snapshot until the user restarts it.
        let leased: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM resource_operation_leases
             WHERE resource_kind='launch_preset' AND resource_id=?1)",
                [launch_preset_id],
                |row| row.get(0),
            )
            .map_err(|_| "managed_workspace_lease_check_failed".to_string())?;
        if leased {
            return Err("resource_operation_in_progress".to_string());
        }
        if let Some(id) = provider_id {
            let bound: i64 = transaction
                .query_row(
                    "SELECT COUNT(*) FROM launch_preset_api_providers WHERE launch_preset_id=?1 AND provider_id=?2",
                    rusqlite::params![launch_preset_id, id],
                    |row| row.get(0),
                )
                .map_err(|error| format!("managed_preset_lookup_failed:{error}"))?;
            if bound == 0 {
                return Err("login_provider_not_in_workspace".to_string());
            }
        }
        let next_version = version
            .checked_add(1)
            .ok_or_else(|| "managed_workspace_version_overflow".to_string())?;
        let merged = merge_login_provider_profile_options(&profile_options, provider_id)?;
        transaction
            .execute(
                "UPDATE launch_presets
                 SET profile_options=?1, version=?2, updated_at=?3
                 WHERE id=?4 AND deleted_at IS NULL",
                rusqlite::params![merged, next_version, now, launch_preset_id],
            )
            .map_err(|error| format!("managed_preset_update_failed:{error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("managed_preset_commit_failed:{error}"))?;
        Ok(ManagedPresetSaved {
            id: launch_preset_id.to_string(),
            version: next_version,
        })
    }

    pub fn compositions(&self) -> Result<Vec<ManagedPresetCompositionSummary>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "managed_launch_database_lock_failed".to_string())?;
        let mut statement = connection
            .prepare(
                "SELECT composition.launch_preset_id, composition.official_account_id,
                        composition.state,
                        (SELECT identity.public_model_id
                         FROM launch_preset_role_models role_model
                         JOIN model_route_identities identity
                           ON identity.id=role_model.route_identity_id
                         WHERE role_model.launch_preset_id=composition.launch_preset_id
                           AND role_model.role='auto_review')
                 FROM launch_preset_compositions composition ORDER BY composition.created_at",
            )
            .map_err(|error| format!("managed_composition_read_failed:{error}"))?;
        let headers = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(|error| format!("managed_composition_read_failed:{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("managed_composition_decode_failed:{error}"))?;
        drop(statement);

        let mut result = Vec::with_capacity(headers.len());
        for (preset_id, official_account_id, state, auto_review_public_model_id) in headers {
            let providers = query_strings(
                &connection,
                "SELECT provider_id FROM launch_preset_api_providers
                 WHERE launch_preset_id=?1 ORDER BY position",
                &preset_id,
            )?;
            let mut model_statement = connection
                .prepare(
                    "SELECT selection.route_identity_id, identity.source_kind,
                            COALESCE(identity.official_account_id, identity.api_key_provider_id),
                            identity.upstream_model_id, identity.public_model_id,
                            COALESCE(selection.display_name_override, identity.public_model_id),
                            selection.context_window, selection.auto_compact_token_limit,
                            selection.reasoning_levels_json, selection.default_reasoning_level
                     FROM launch_preset_model_selections selection
                     JOIN model_route_identities identity ON identity.id=selection.route_identity_id
                     WHERE selection.launch_preset_id=?1 ORDER BY selection.position",
                )
                .map_err(|error| format!("managed_model_read_failed:{error}"))?;
            let models = model_statement
                .query_map([&preset_id], |row| {
                    let kind = row.get::<_, String>(1)?;
                    let source_kind = RouteKind::from_db(&kind).ok_or_else(|| {
                        rusqlite::Error::InvalidColumnType(
                            1,
                            "source_kind".to_string(),
                            rusqlite::types::Type::Text,
                        )
                    })?;
                    Ok(ManagedPresetModelSummary {
                        route_identity_id: row.get(0)?,
                        source_kind,
                        source_id: row.get(2)?,
                        upstream_model_id: row.get(3)?,
                        public_model_id: row.get(4)?,
                        display_name: row.get(5)?,
                        context_window: row.get::<_, Option<i64>>(6)?.map(|value| value as u64),
                        auto_compact_token_limit: row
                            .get::<_, Option<i64>>(7)?
                            .map(|value| value as u64),
                        reasoning_levels: row
                            .get::<_, Option<String>>(8)?
                            .map(|value| {
                                serde_json::from_str(&value).map_err(|_| {
                                    rusqlite::Error::InvalidColumnType(
                                        8,
                                        "reasoning_levels_json".to_string(),
                                        rusqlite::types::Type::Text,
                                    )
                                })
                            })
                            .transpose()?
                            .unwrap_or_default(),
                        default_reasoning_level: row.get(9)?,
                    })
                })
                .map_err(|error| format!("managed_model_read_failed:{error}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("managed_model_decode_failed:{error}"))?;
            result.push(ManagedPresetCompositionSummary {
                launch_preset_id: preset_id,
                official_account_id,
                api_key_provider_ids: providers,
                models,
                auto_review_public_model_id,
                state,
            });
        }
        Ok(result)
    }
}
