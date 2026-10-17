//! 准备启动：校验 composition、固化 RouteSet、物化隔离配置。
//!
//! 不写工作区渠道列表，不自己 spawn Codex。home / Electron / binding 记录交给兄弟模块。
//! `prepare_launch_with_target` 是 CLI 与 App 启动的共同入口。

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::OptionalExtension;
use url::Url;
use uuid::Uuid;

use crate::codex_control::InstanceState;
use crate::launcher_protocol::LaunchTarget;
#[cfg(all(not(test), target_os = "macos"))]
use crate::managed_codex_config::materialize_isolated_computer_use_mcp;
#[cfg(all(not(test), target_os = "windows"))]
use crate::managed_codex_config::materialize_windows_computer_use_from_cli;
use crate::managed_codex_config::{
    build_version_matched_catalog_for_version, codex_catalog_compatibility,
    keep_catalog_reasoning_efforts_selectable, keep_managed_permission_selector_available,
    materialize_managed_app_config, materialize_managed_codex_config_with_auto_review,
    read_bundled_catalog, seed_default_codex_preferences, verify_catalog_models_with_codex,
    verify_managed_profile_with_codex,
};
use crate::model_family_profile::{
    allowed_reasoning_levels_for_model, default_auto_compact_token_limit,
    family_context_limits_for_route, family_image_support_for_route,
    family_reasoning_levels_for_route,
};
use crate::routing::{
    looks_like_gpt_model_id, CapabilitySet, CompactionSupport, GatewayServiceRevision,
    GatewayTimeoutSnapshot, ModelRoute, RouteKind, RouteSetProviderRevision, RouteSetRevision,
    RouteSetState, RouteSource, WireApi,
};
use crate::secrets::{instance_gateway_key_hash, instance_gateway_key_ref, InstanceGatewayKey};
use crate::support::unix_now_seconds;

use super::models::{
    ManagedLaunchStore, ManagedLaunchTransport, PreparedManagedLaunch, ProviderSnapshot,
};
use super::storage::migrate_managed_app_sqlite_into_home;
use super::validation::{
    ensure_owned_path, resolve_workspace, set_private_directory, sha256_label,
};
use super::workspace::{effective_login_provider_id, qualified_model_display_name};

impl ManagedLaunchStore {
    pub fn is_ready_managed_preset(&self, id: &str) -> Result<bool, String> {
        self.connection
            .lock()
            .map_err(|_| "managed_launch_database_lock_failed".to_string())?
            .query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM launch_preset_compositions composition
                   JOIN launch_presets preset ON preset.id=composition.launch_preset_id
                   WHERE composition.launch_preset_id=?1
                     AND preset.entry_kind='managed_app' AND preset.deleted_at IS NULL
                     AND composition.state='ready'
                     AND EXISTS (
                       SELECT 1 FROM launch_preset_model_selections selection
                       WHERE selection.launch_preset_id=composition.launch_preset_id
                     )
                )",
                [id],
                |row| row.get(0),
            )
            .map_err(|error| format!("managed_preset_lookup_failed:{error}"))
    }

    pub fn launch_transport(&self, id: &str) -> Result<ManagedLaunchTransport, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "managed_launch_database_lock_failed".to_string())?;
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM launch_presets WHERE id=?1 AND deleted_at IS NULL)",
                [id],
                |row| row.get(0),
            )
            .map_err(|error| format!("managed_launch_transport_read_failed:{error}"))?;
        if !exists {
            return Err("managed_preset_not_found".to_string());
        }
        Ok(ManagedLaunchTransport::Gateway)
    }

    #[cfg(test)]
    pub fn prepare_launch(
        &self,
        launch_preset_id: &str,
        expected_version: i64,
        requested_workspace: Option<&str>,
        codex_executable: &Path,
        codex_version: &str,
        gateway_base_url: Option<&Url>,
        current_codex_home: &Path,
    ) -> Result<PreparedManagedLaunch, String> {
        self.prepare_launch_with_target(
            launch_preset_id,
            expected_version,
            requested_workspace,
            codex_executable,
            codex_version,
            gateway_base_url,
            current_codex_home,
            "cli",
        )
    }

    pub fn prepare_launch_with_target(
        &self,
        launch_preset_id: &str,
        expected_version: i64,
        requested_workspace: Option<&str>,
        codex_executable: &Path,
        codex_version: &str,
        gateway_base_url: Option<&Url>,
        _current_codex_home: &Path,
        launch_target: &str,
    ) -> Result<PreparedManagedLaunch, String> {
        if LaunchTarget::from_wire(launch_target).is_none() {
            return Err("invalid_launch_target".to_string());
        }
        let (
            preset_version,
            default_workspace,
            official_account_id,
            state,
            stored_gateway_policy,
            profile_options,
        ) = {
            let connection = self
                .connection
                .lock()
                .map_err(|_| "managed_launch_database_lock_failed".to_string())?;
            connection
                .query_row(
                    "SELECT preset.version, preset.default_workspace_path,
                            composition.official_account_id, composition.state,
                            COALESCE(preset.gateway_policy, 'gateway_passthrough'),
                            COALESCE(preset.profile_options, '{}')
                     FROM launch_presets preset
                     JOIN launch_preset_compositions composition
                       ON composition.launch_preset_id=preset.id
                     WHERE preset.id=?1 AND preset.deleted_at IS NULL",
                    [launch_preset_id],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                        ))
                    },
                )
                .optional()
                .map_err(|error| format!("managed_preset_lookup_failed:{error}"))?
                .ok_or_else(|| "launch_preset_not_found".to_string())?
        };
        if preset_version != expected_version {
            return Err("version_conflict".to_string());
        }
        if state != "ready" {
            return Err("launch_preset_needs_model_selection".to_string());
        }
        let gateway_policy = stored_gateway_policy;
        if !matches!(
            gateway_policy.as_str(),
            "direct" | "gateway_passthrough" | "gateway_chat_completions"
        ) {
            return Err("invalid_gateway_policy".to_string());
        }
        // The managed App owns directory selection inside its UI. Never
        // pre-bind an App launch to a caller-supplied or preset workspace;
        // CLI launches retain the explicit per-launch directory behavior.
        let workspace_path = if LaunchTarget::wire_is_managed_app(&launch_target) {
            None
        } else {
            match requested_workspace {
                Some(path) => resolve_workspace(Some(path))?,
                None => resolve_workspace(default_workspace.as_deref())?,
            }
        };

        let provider_snapshots = self.provider_snapshots(launch_preset_id)?;
        if provider_snapshots.is_empty() {
            return Err("credential_missing".to_string());
        }
        let login_provider_id = effective_login_provider_id(&profile_options, &provider_snapshots)?;
        let provider_by_id = provider_snapshots
            .iter()
            .map(|provider| (provider.provider_id.as_str(), provider))
            .collect::<BTreeMap<_, _>>();
        let composition = self
            .compositions()?
            .into_iter()
            .find(|composition| composition.launch_preset_id == launch_preset_id)
            .ok_or_else(|| "launch_preset_not_found".to_string())?;
        let auto_review_public_model_id = composition.auto_review_public_model_id;
        let selections = composition.models;
        if selections.is_empty() {
            return Err("route_set_has_no_models".to_string());
        }
        // A verified official login may be used as the App's client identity
        // while every selected model is still routed to an API Provider. This
        // is the safe Cockpit-style combination. Forwarding an actual
        // official model through the R Codex Gateway remains intentionally closed.
        if official_account_id.is_some()
            && selections
                .iter()
                .any(|selection| selection.source_kind == RouteKind::Official)
        {
            return Err("official_hybrid_route_experimental_disabled".to_string());
        }
        let official_credential_revision_id =
            if let Some(account_id) = official_account_id.as_deref() {
                let connection = self
                    .connection
                    .lock()
                    .map_err(|_| "managed_launch_database_lock_failed".to_string())?;
                let credential_revision_id: Option<String> = connection
                    .query_row(
                        "SELECT revision.id
                     FROM codex_accounts account
                     JOIN official_credential_revisions revision
                       ON revision.id=account.current_credential_revision_id
                     WHERE account.id=?1 AND revision.auth_state='authorized'",
                        [account_id],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|error| format!("official_account_lookup_failed:{error}"))?;
                if credential_revision_id.is_none() {
                    return Err("official_account_not_authorized".to_string());
                }
                credential_revision_id
            } else {
                None
            };

        let is_managed_app = LaunchTarget::wire_is_managed_app(&launch_target);
        let existing_app_runtime = if is_managed_app {
            self.managed_app_runtime_binding(launch_preset_id, launch_target)?
        } else {
            None
        };
        if let Some((_, _, _, state)) = existing_app_runtime.as_ref() {
            if is_managed_app && !InstanceState::is_finished(state.as_str()) {
                return Err("managed_app_in_use".to_string());
            }
        }
        let instance_id = existing_app_runtime
            .as_ref()
            .map(|binding| binding.0.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let launch_nonce = Uuid::new_v4().to_string();
        let lifecycle_dir = existing_app_runtime
            .as_ref()
            .map(|binding| binding.2.clone())
            .unwrap_or_else(|| self.instances_root.join(&instance_id));
        ensure_owned_path(&self.instances_root, &lifecycle_dir)?;
        // Each managed App gets its own Electron user-data directory inside
        // its lifecycle directory. Codex's single-instance lock follows that
        // directory, so isolated Apps can run together. The directory stays
        // with this preset's runtime binding across relaunches.
        let electron_user_data_dir = if is_managed_app {
            Some(self.managed_app_electron_data_dir(&lifecycle_dir)?)
        } else {
            None
        };
        let status_file = lifecycle_dir.join("status.json");
        std::fs::create_dir_all(&lifecycle_dir)
            .map_err(|error| format!("managed_lifecycle_dir_create_failed:{error}"))?;
        set_private_directory(&lifecycle_dir)?;
        let (managed_codex_home, launch_codex_home, home_layout_marker) = if is_managed_app {
            self.prepare_managed_app_home_layout(
                &instance_id,
                &lifecycle_dir,
                existing_app_runtime
                    .as_ref()
                    .map(|binding| binding.1.as_path()),
            )?
        } else {
            let managed_codex_home = lifecycle_dir.join("codex-home");
            ensure_owned_path(&self.instances_root, &managed_codex_home)?;
            std::fs::create_dir_all(&managed_codex_home)
                .map_err(|error| format!("managed_codex_home_create_failed:{error}"))?;
            set_private_directory(&managed_codex_home)?;
            let launch_codex_home =
                self.short_codex_home_link(&instance_id, &managed_codex_home)?;
            (managed_codex_home, launch_codex_home, None)
        };
        if is_managed_app {
            let legacy_sqlite_home = self.managed_app_sqlite_path(launch_preset_id)?;
            migrate_managed_app_sqlite_into_home(&legacy_sqlite_home, &managed_codex_home)?;
        }
        let sqlite_home = is_managed_app.then(|| launch_codex_home.clone());
        if let Some(electron_user_data_dir) = electron_user_data_dir.as_deref() {
            std::fs::create_dir_all(electron_user_data_dir)
                .map_err(|error| format!("managed_electron_data_dir_create_failed:{error}"))?;
            set_private_directory(electron_user_data_dir)?;
        }
        // Isolated Electron state starts empty. Do not copy the user's default
        // Codex App preferences, cookies, or login databases.
        let bundled_catalog = read_bundled_catalog(codex_executable)?;
        let compact_contract =
            codex_catalog_compatibility(codex_version, &bundled_catalog).compact_contract;
        let compact_mode = compact_contract.compact_mode_for_launch(launch_target);
        let mut routes = Vec::with_capacity(selections.len());
        for (position, selection) in selections.into_iter().enumerate() {
            let provider = provider_by_id
                .get(selection.source_id.as_str())
                .ok_or_else(|| "route_source_not_selected".to_string())?;
            if selection.source_kind != RouteKind::ApiProvider {
                return Err("official_hybrid_route_experimental_disabled".to_string());
            }
            let wire_api = provider.wire_api;
            let mut route = ModelRoute {
                id: Uuid::new_v4().to_string(),
                route_identity_id: selection.route_identity_id,
                public_model_id: selection.public_model_id,
                display_name: qualified_model_display_name(&provider.name, &selection.display_name),
                source: RouteSource::ApiProvider {
                    provider_id: provider.provider_id.clone(),
                    provider_revision_id: provider.revision_id.clone(),
                },
                upstream_model_id: selection.upstream_model_id,
                wire_api,
                adapter_kind: match wire_api {
                    WireApi::Responses => "responses_passthrough",
                    WireApi::ChatCompletions => "chat_completions_to_responses",
                }
                .to_string(),
                adapter_version: "1".to_string(),
                capabilities: CapabilitySet {
                    input_modalities: vec!["text".to_string()],
                    // Responses keeps Codex tool calls. Chat converts function
                    // and custom tools in chat_bridge; web_search stays off.
                    supports_tools: true,
                    ..CapabilitySet::default()
                },
                position: position as u32,
            };
            // Store the family ladder in the immutable RouteSet snapshot. The
            // catalog generator reuses the same source of truth, so a later
            // launch cannot accidentally restore GPT-only `max` or `ultra`.
            let official_family_levels =
                allowed_reasoning_levels_for_model(&route.upstream_model_id);
            route.capabilities.reasoning_levels = if official_family_levels.is_empty() {
                Vec::new()
            } else if !selection.reasoning_levels.is_empty() {
                selection.reasoning_levels
            } else {
                family_reasoning_levels_for_route(&route)
            };
            route.capabilities.default_reasoning_level = selection
                .default_reasoning_level
                .filter(|default| route.capabilities.reasoning_levels.contains(default))
                .or_else(|| {
                    let profile_default =
                        crate::model_family_profile::resolve_model_family(&route.upstream_model_id)
                            .default_reasoning_level;
                    profile_default.filter(|default| {
                        route.capabilities.reasoning_levels.contains(default)
                            && (official_family_levels.contains(&default) || default == "high")
                    })
                });
            if family_image_support_for_route(&route) {
                route.capabilities.supports_images = true;
                route.capabilities.input_modalities = vec!["text".to_string(), "image".to_string()];
            }
            let (family_window, family_compact) = family_context_limits_for_route(&route);
            let context_window = selection.context_window.or(family_window);
            let auto_compact_token_limit = selection
                .auto_compact_token_limit
                .or_else(|| {
                    family_compact.filter(|compact| {
                        context_window
                            .map(|window| *compact <= window)
                            .unwrap_or(true)
                    })
                })
                .or_else(|| context_window.map(default_auto_compact_token_limit));
            route.capabilities.context_window = context_window;
            route.capabilities.auto_compact_token_limit = auto_compact_token_limit;
            route.capabilities.compaction_support = CompactionSupport::for_api_provider(
                compact_mode,
                looks_like_gpt_model_id(&route.public_model_id, &route.upstream_model_id),
            );
            routes.push(route);
        }

        let catalog = build_version_matched_catalog_for_version(
            &bundled_catalog,
            &routes,
            codex_version,
            auto_review_public_model_id.as_deref(),
        )?;
        // Each Responses API route has a self-referential catalog override.
        // Enabling the native review role here makes the active conversation
        // model select its own reviewer without exposing a second UI choice.
        let profile_name = format!(
            "managed_{}_{}",
            instance_id.replace('-', ""),
            launch_nonce.replace('-', "").get(..12).unwrap_or("launch")
        );
        let transport_mode = ManagedLaunchTransport::Gateway;
        let materialized = materialize_managed_codex_config_with_auto_review(
            &managed_codex_home,
            &profile_name,
            routes
                .first()
                .map(|route| route.public_model_id.as_str())
                .ok_or_else(|| "route_set_has_no_models".to_string())?,
            gateway_base_url.ok_or_else(|| "managed_gateway_not_serving".to_string())?,
            official_account_id.is_some(),
            &catalog,
        )?;
        // Isolated App and CLI share the first-run preference seed and the
        // home config.toml merge. Computer Use MCP stays App-only.
        if let Some(source_home) = crate::support::user_home() {
            seed_default_codex_preferences(&managed_codex_home, &source_home.join(".codex"))?;
        }
        materialize_managed_app_config(&managed_codex_home, &materialized.profile_path)?;
        if LaunchTarget::wire_is_managed_app(&launch_target) {
            #[cfg(all(not(test), target_os = "macos"))]
            materialize_isolated_computer_use_mcp(&managed_codex_home)?;
            #[cfg(all(not(test), target_os = "windows"))]
            {
                let computer_use_log = lifecycle_dir.join("startup.log");
                materialize_windows_computer_use_from_cli(&managed_codex_home, codex_executable)?;
                let _ = crate::launcher_protocol::append_startup_log(
                    &computer_use_log,
                    "windows_computer_use_ready",
                );
            }
            keep_managed_permission_selector_available(&managed_codex_home)?;
            keep_catalog_reasoning_efforts_selectable(&managed_codex_home)?;
        }
        // verify_catalog_models_with_codex performs the parser check and the
        // expected-model check in one Codex invocation; avoid starting the
        // same CLI probe twice on every managed App launch.
        let preflight_log = lifecycle_dir.join("startup.log");
        let verification = (|| -> Result<(), String> {
            verify_catalog_models_with_codex(
                codex_executable,
                &materialized.catalog_path,
                &routes
                    .iter()
                    .map(|route| route.public_model_id.clone())
                    .collect::<Vec<_>>(),
                &preflight_log,
            )?;
            verify_managed_profile_with_codex(
                codex_executable,
                &managed_codex_home,
                &profile_name,
                &routes
                    .iter()
                    .map(|route| route.public_model_id.clone())
                    .collect::<Vec<_>>(),
                &preflight_log,
            )?;
            Ok(())
        })();
        verification?;
        let now = unix_now_seconds();
        let gateway_snapshot = serde_json::json!({
            "protocolVersion": 1,
            "transportMode": "gateway",
            "bindHost": "127.0.0.1",
            "baseUrl": gateway_base_url.map(Url::as_str),
            "compactMode": compact_mode.as_snapshot_str(),
        })
        .to_string();
        let gateway_service = GatewayServiceRevision {
            id: Uuid::new_v4().to_string(),
            protocol_version: 1,
            config_sha256: sha256_label(gateway_snapshot.as_bytes()),
            config_snapshot: gateway_snapshot,
            created_at: now,
        };
        let route_set = RouteSetRevision {
            id: Uuid::new_v4().to_string(),
            launch_preset_id: launch_preset_id.to_string(),
            launch_preset_version: expected_version as u64,
            official_account_id: official_account_id.clone(),
            official_credential_revision_id: official_credential_revision_id.clone(),
            gateway_service,
            requires_openai_auth: official_account_id.is_some(),
            catalog_path: materialized.catalog_path.display().to_string(),
            catalog_sha256: materialized.catalog_sha256.clone(),
            state: RouteSetState::Ready,
            created_at: now,
            providers: provider_snapshots
                .iter()
                .map(|provider| RouteSetProviderRevision {
                    provider_id: provider.provider_id.clone(),
                    provider_revision_id: provider.revision_id.clone(),
                    position: provider.position,
                    timeouts: provider.timeouts.clone(),
                })
                .collect(),
            routes,
        };
        route_set
            .validate_ready()
            .map_err(|error| error.to_string())?;
        self.route_store.insert_ready_route_set(&route_set)?;

        let instance_key = InstanceGatewayKey::generate();
        let instance_key_ref = instance_gateway_key_ref(&instance_id)?;
        let instance_key_hash = instance_gateway_key_hash(&instance_key);
        let gateway_credentials = Some((instance_key_ref, instance_key_hash, instance_key));
        let write_result = self.insert_launch_records(
            &instance_id,
            &launch_nonce,
            &lifecycle_dir,
            &managed_codex_home,
            workspace_path.as_deref(),
            codex_executable,
            codex_version,
            &materialized.profile_sha256,
            &route_set,
            gateway_credentials.as_ref().map(|item| item.0.as_str()),
            gateway_credentials.as_ref().map(|item| item.1.as_str()),
            launch_target,
        );
        if let Err(error) = write_result {
            return Err(error);
        }
        if let Some(marker) = home_layout_marker.as_deref() {
            // The control database now records the short home. A stale marker
            // is recoverable on the next launch, so cleanup cannot invalidate
            // the completed migration.
            let _ = self.complete_managed_app_home_layout(marker);
        }
        if existing_app_runtime.is_some() {
            // The stable instance reuses its lifecycle directory, but a
            // previous receipt contains the old launch nonce. Remove it only
            // after the new database snapshot is committed; otherwise
            // reconcile would treat the restart as a receipt mismatch.
            let _ = std::fs::remove_file(&status_file);
        }

        Ok(PreparedManagedLaunch {
            instance_id,
            launch_nonce,
            sqlite_home,
            launch_codex_home,
            managed_codex_home,
            login_provider_id: Some(login_provider_id),
            electron_user_data_dir,
            status_file,
            workspace_path,
            instance_key_ref: gateway_credentials.as_ref().map(|item| item.0.clone()),
            instance_key_hash: gateway_credentials.as_ref().map(|item| item.1.clone()),
            instance_key: gateway_credentials.map(|item| item.2),
            profile_name,
            filtered_mcp_servers: Vec::new(),
            route_set,
            transport_mode,
        })
    }

    pub(super) fn provider_snapshots(
        &self,
        preset_id: &str,
    ) -> Result<Vec<ProviderSnapshot>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "managed_launch_database_lock_failed".to_string())?;
        let mut statement = connection
            .prepare(
                "SELECT binding.provider_id, provider.name, revision.id, revision.wire_api,
                        revision.base_url, revision.secret_ref,
                        COALESCE(timeout.connect_seconds, 10),
                        COALESCE(timeout.first_byte_seconds, 30),
                        COALESCE(timeout.stream_idle_seconds, 45),
                        COALESCE(timeout.non_stream_total_seconds, 120),
                        binding.position, revision.state
                 FROM launch_preset_api_providers binding
                 JOIN api_key_providers provider ON provider.id=binding.provider_id
                 JOIN api_key_provider_revisions revision
                   ON revision.id=provider.current_revision_id
                 LEFT JOIN gateway_timeout_settings timeout
                   ON timeout.provider_id=binding.provider_id
                 WHERE binding.launch_preset_id=?1 ORDER BY binding.position",
            )
            .map_err(|error| format!("provider_snapshot_read_failed:{error}"))?;
        let rows = statement
            .query_map([preset_id], |row| {
                let wire = row.get::<_, String>(3)?;
                let wire_api = match wire.as_str() {
                    "responses" => WireApi::Responses,
                    "chat_completions" => WireApi::ChatCompletions,
                    _ => {
                        return Err(rusqlite::Error::InvalidColumnType(
                            3,
                            "wire_api".to_string(),
                            rusqlite::types::Type::Text,
                        ))
                    }
                };
                let state = row.get::<_, String>(11)?;
                if state != "active" {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                Ok(ProviderSnapshot {
                    provider_id: row.get(0)?,
                    name: row.get(1)?,
                    revision_id: row.get(2)?,
                    wire_api,
                    base_url: row.get(4)?,
                    secret_ref: row.get(5)?,
                    timeouts: GatewayTimeoutSnapshot {
                        connect_seconds: row.get(6)?,
                        first_byte_seconds: row.get(7)?,
                        stream_idle_seconds: row.get(8)?,
                        non_stream_total_seconds: row.get(9)?,
                    },
                    position: row.get(10)?,
                })
            })
            .map_err(|error| format!("provider_snapshot_read_failed:{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "credential_missing".to_string())?;
        for row in &rows {
            let wire_api: Option<String> = connection
                .query_row(
                    "SELECT wire_api FROM provider_connectivity_reports WHERE provider_id=?1",
                    [row.provider_id.as_str()],
                    |report| report.get(0),
                )
                .optional()
                .map_err(|error| format!("provider_protocol_lookup_failed:{error}"))?
                .flatten();
            if !matches!(wire_api.as_deref(), Some("responses" | "chat_completions")) {
                return Err("protocol_unconfirmed".to_string());
            }
        }
        Ok(rows)
    }
}
