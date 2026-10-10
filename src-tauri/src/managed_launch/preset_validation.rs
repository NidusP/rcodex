//! 工作区模型勾选和来源校验。
//!
//! 不写库（除 identity upsert）、不准备启动。create_preset 在写入前调用。

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

use super::ManagedModelSelectionInput;
use crate::model_family_profile::{allowed_reasoning_levels_for_model, MAX_CLIENT_CONTEXT_WINDOW};
use crate::routing::{
    is_internal_codex_model_id, validate_public_model_id, RouteKind, RouteSetRevision,
};

pub(crate) const INVALID_MODEL_WINDOW: &str = "invalid_model_window";

pub(super) fn validate_sources(
    official_account_id: Option<&str>,
    provider_ids: Vec<String>,
    allow_empty: bool,
) -> Result<Vec<String>, String> {
    let mut seen = BTreeSet::new();
    let provider_ids = provider_ids
        .into_iter()
        .map(|value| value.trim().to_string())
        .collect::<Vec<_>>();
    if !allow_empty && official_account_id.is_none() && provider_ids.is_empty() {
        return Err("invalid_launch_preset".to_string());
    }
    if provider_ids
        .iter()
        .any(|value| value.is_empty() || !seen.insert(value.clone()))
    {
        return Err("duplicate_provider".to_string());
    }
    Ok(provider_ids)
}

pub(super) fn validate_model(model: &ManagedModelSelectionInput) -> Result<(), String> {
    validate_public_model_id(&model.public_model_id).map_err(|error| error.to_string())?;
    for value in [
        &model.source_id,
        &model.upstream_model_id,
        &model.display_name,
    ] {
        if value.trim() != value || value.is_empty() || value.chars().any(char::is_control) {
            return Err("invalid_model_selection".to_string());
        }
    }
    if is_internal_codex_model_id(&model.upstream_model_id)
        || is_internal_codex_model_id(&model.public_model_id)
    {
        return Err("reserved_internal_model".to_string());
    }
    validate_optional_window(model.context_window)?;
    validate_optional_window(model.auto_compact_token_limit)?;
    if let (Some(window), Some(compact)) = (model.context_window, model.auto_compact_token_limit) {
        if compact > window {
            return Err(INVALID_MODEL_WINDOW.to_string());
        }
    }
    if model
        .reasoning_levels
        .iter()
        .any(|level| level.trim() != level)
        || model.reasoning_levels.iter().any(|level| level.is_empty())
        || model.reasoning_levels.iter().collect::<BTreeSet<_>>().len()
            != model.reasoning_levels.len()
    {
        return Err("invalid_reasoning_levels".to_string());
    }
    let family_levels = allowed_reasoning_levels_for_model(&model.upstream_model_id);
    if !model.reasoning_levels.is_empty()
        && model
            .reasoning_levels
            .iter()
            .any(|level| !family_levels.contains(level))
    {
        return Err("invalid_reasoning_levels".to_string());
    }
    if let Some(default) = model.default_reasoning_level.as_deref() {
        if !model.reasoning_levels.iter().any(|level| level == default) {
            return Err("invalid_default_reasoning_level".to_string());
        }
    }
    Ok(())
}

fn validate_optional_window(value: Option<u64>) -> Result<(), String> {
    match value {
        None => Ok(()),
        Some(value) if value == 0 || value > MAX_CLIENT_CONTEXT_WINDOW => {
            Err(INVALID_MODEL_WINDOW.to_string())
        }
        Some(_) => Ok(()),
    }
}

pub(super) fn upsert_identity(
    transaction: &rusqlite::Transaction<'_>,
    model: &ManagedModelSelectionInput,
    now: i64,
) -> Result<String, String> {
    let (official_account_id, api_key_provider_id) = match model.source_kind {
        RouteKind::Official => (Some(model.source_id.as_str()), None),
        RouteKind::ApiProvider => (None, Some(model.source_id.as_str())),
    };
    let existing = transaction
        .query_row(
            "SELECT id, public_model_id FROM model_route_identities
             WHERE source_kind=?1
               AND COALESCE(official_account_id, api_key_provider_id)=?2
               AND upstream_model_id=?3",
            params![
                model.source_kind.as_db(),
                model.source_id,
                model.upstream_model_id
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|error| format!("model_identity_lookup_failed:{error}"))?;
    if let Some((id, public_model_id)) = existing {
        if public_model_id != model.public_model_id {
            return Err("model_public_id_immutable".to_string());
        }
        return Ok(id);
    }
    let id = Uuid::new_v4().to_string();
    transaction
        .execute(
            "INSERT INTO model_route_identities (
                id, public_model_id, source_kind, official_account_id,
                api_key_provider_id, upstream_model_id, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                model.public_model_id,
                model.source_kind.as_db(),
                official_account_id,
                api_key_provider_id,
                model.upstream_model_id,
                now,
            ],
        )
        .map_err(|error| format!("model_identity_insert_failed:{error}"))?;
    Ok(id)
}

/// Reconcile public IDs against the global identity table before validation
/// and insertion. Public IDs are immutable and globally unique because they
/// are referenced by historical RouteSets, while the UI only has the current
/// workspace's model list available. Reuse an existing source/model identity;
/// when a newly selected source collides with an older identity, append a
/// stable source slug instead of surfacing a raw SQLite UNIQUE error.
pub(super) fn normalize_public_model_ids(
    transaction: &rusqlite::Transaction<'_>,
    models: &mut [ManagedModelSelectionInput],
) -> Result<BTreeMap<String, String>, String> {
    let mut remaps = BTreeMap::new();
    let mut assigned = BTreeSet::new();
    for model in models.iter_mut() {
        let source_kind = model.source_kind.as_db();
        let existing_identity = transaction
            .query_row(
                "SELECT id, public_model_id FROM model_route_identities
                 WHERE source_kind=?1
                   AND COALESCE(official_account_id, api_key_provider_id)=?2
                   AND upstream_model_id=?3",
                params![source_kind, model.source_id, model.upstream_model_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(|error| format!("model_identity_lookup_failed:{error}"))?;
        if let Some((_, existing_public_id)) = existing_identity {
            if existing_public_id != model.public_model_id {
                remaps.insert(model.public_model_id.clone(), existing_public_id.clone());
                model.public_model_id = existing_public_id;
            }
            assigned.insert(model.public_model_id.clone());
            continue;
        }

        let mut candidate = model.public_model_id.clone();
        let mut suffix = 1_u32;
        loop {
            let occupied = assigned.contains(&candidate)
                || transaction
                    .query_row(
                        "SELECT EXISTS(
                           SELECT 1 FROM model_route_identities
                           WHERE public_model_id=?1
                         )",
                        [candidate.as_str()],
                        |row| row.get::<_, bool>(0),
                    )
                    .map_err(|error| format!("model_identity_public_lookup_failed:{error}"))?;
            if !occupied {
                break;
            }
            let source_slug = public_id_source_slug(&model.source_id);
            let suffix_label = if suffix == 1 {
                format!("-{source_slug}")
            } else {
                format!("-{source_slug}-{suffix}")
            };
            let base_limit = 128_usize.saturating_sub(suffix_label.len());
            let base = model
                .public_model_id
                .chars()
                .take(base_limit.max(1))
                .collect::<String>();
            candidate = format!("{base}{suffix_label}");
            suffix = suffix.saturating_add(1);
        }
        if candidate != model.public_model_id {
            remaps.insert(model.public_model_id.clone(), candidate.clone());
            model.public_model_id = candidate;
        }
        assigned.insert(model.public_model_id.clone());
    }
    Ok(remaps)
}

fn public_id_source_slug(source_id: &str) -> String {
    let slug = source_id
        .chars()
        .filter_map(|character| {
            if character.is_ascii_alphanumeric() {
                Some(character.to_ascii_lowercase())
            } else if matches!(character, '-' | '_' | '.') {
                Some('-')
            } else {
                None
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if slug.is_empty() {
        "provider".to_string()
    } else {
        slug.chars().take(48).collect()
    }
}

pub(super) fn ensure_resources_available(
    transaction: &rusqlite::Transaction<'_>,
    route_set: &RouteSetRevision,
) -> Result<(), String> {
    let mut resources = vec![("launch_preset", route_set.launch_preset_id.as_str())];
    for provider in &route_set.providers {
        resources.push(("api_key_provider", provider.provider_id.as_str()));
        resources.push((
            "api_key_provider_revision",
            provider.provider_revision_id.as_str(),
        ));
    }
    for (kind, id) in resources {
        let leased: bool = transaction
            .query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM resource_operation_leases
                   WHERE resource_kind=?1 AND resource_id=?2
                )",
                params![kind, id],
                |row| row.get(0),
            )
            .map_err(|error| format!("resource_lease_check_failed:{error}"))?;
        if leased {
            return Err("resource_operation_in_progress".to_string());
        }
    }
    Ok(())
}
