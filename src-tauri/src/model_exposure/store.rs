use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::{database::Database, support::unix_now_seconds};

use super::{
    is_selectable_model_id, ExposureChange, ModelExposureSummary, ProbeContext,
    IMAGE_PROBE_CONTRACT_VERSION,
};

#[derive(Clone)]
pub(crate) struct ModelExposureStore {
    connection: Arc<Mutex<Connection>>,
}

impl ModelExposureStore {
    pub(crate) fn from_database(database: Database) -> Self {
        Self {
            connection: database.connection(),
        }
    }

    pub(crate) fn list(&self, provider_id: &str) -> Result<Vec<ModelExposureSummary>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "model_exposure_database_lock_failed")?;
        assert_provider_exists(&connection, provider_id)?;
        let mut statement = connection.prepare(
            "SELECT upstream_model_id FROM provider_model_exposures WHERE provider_id=?1
             UNION
             SELECT entry.upstream_model_id FROM provider_model_catalog_entries entry
             JOIN provider_model_catalog_revisions catalog ON catalog.id=entry.catalog_revision_id
             JOIN api_key_providers provider ON provider.id=catalog.provider_id
             WHERE catalog.provider_id=?1 AND catalog.id=(
                 SELECT latest.id FROM provider_model_catalog_revisions latest
                 WHERE latest.provider_id=?1 AND latest.provider_revision_id=provider.current_revision_id
                   AND latest.state='ready'
                 ORDER BY latest.discovered_at DESC, latest.rowid DESC LIMIT 1
             )
             ORDER BY upstream_model_id"
        ).map_err(|error| format!("model_exposure_list_failed:{error}"))?;
        let ids = statement
            .query_map([provider_id], |row| row.get::<_, String>(0))
            .map_err(|error| format!("model_exposure_list_failed:{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("model_exposure_list_failed:{error}"))?;
        ids.into_iter()
            .filter(|id| is_selectable_model_id(id))
            .map(|id| read_summary(&connection, provider_id, &id))
            .collect()
    }

    pub(crate) fn summary(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<ModelExposureSummary, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "model_exposure_database_lock_failed")?;
        assert_provider_exists(&connection, provider_id)?;
        validate_model_id(model_id)?;
        read_summary(&connection, provider_id, model_id)
    }

    pub(crate) fn set_exposure(
        &self,
        provider_id: &str,
        model_id: &str,
        exposed: bool,
    ) -> Result<ExposureChange, String> {
        validate_model_id(model_id)?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "model_exposure_database_lock_failed")?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("model_exposure_transaction_failed:{error}"))?;
        assert_provider_exists(&transaction, provider_id)?;
        let previous = read_summary(&transaction, provider_id, model_id)?;
        if exposed && !previous.exposed {
            // New choices must come from this configuration's discovered catalog.
            // Old saved choices remain visible even if the upstream drops a model.
            let discovered: bool = transaction.query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM provider_model_catalog_entries entry
                   JOIN provider_model_catalog_revisions catalog ON catalog.id=entry.catalog_revision_id
                   JOIN api_key_providers provider ON provider.id=catalog.provider_id
                   WHERE catalog.provider_id=?1 AND entry.upstream_model_id=?2
                     AND catalog.id=(SELECT latest.id FROM provider_model_catalog_revisions latest
                        WHERE latest.provider_id=?1 AND latest.provider_revision_id=provider.current_revision_id
                          AND latest.state='ready'
                        ORDER BY latest.discovered_at DESC, latest.rowid DESC LIMIT 1)
                 )", params![provider_id, model_id], |row| row.get(0)
            ).map_err(|error| format!("model_exposure_catalog_read_failed:{error}"))?;
            if !discovered {
                return Err("model_not_discovered".to_string());
            }
            read_probe_context(&transaction, provider_id, model_id, false)?;
        }
        if !exposed && !previous.referenced_by.is_empty() {
            return Err(format!(
                "model_exposure_in_use:请先从{}移除这个模型，再取消透出。",
                previous.referenced_by.join("、")
            ));
        }
        transaction.execute(
            "INSERT INTO provider_model_exposures(provider_id, upstream_model_id, exposed, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(provider_id, upstream_model_id) DO UPDATE SET exposed=excluded.exposed, updated_at=excluded.updated_at",
            params![provider_id, model_id, exposed, unix_now_seconds()]
        ).map_err(|error| format!("model_exposure_write_failed:{error}"))?;
        let summary = read_summary(&transaction, provider_id, model_id)?;
        let should_probe = exposed && !previous.exposed && summary.image_probe_state == "unknown";
        transaction
            .commit()
            .map_err(|error| format!("model_exposure_commit_failed:{error}"))?;
        Ok(ExposureChange {
            summary,
            should_probe,
        })
    }

    pub(crate) fn probe_context(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<ProbeContext, String> {
        validate_model_id(model_id)?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| "model_exposure_database_lock_failed")?;
        read_probe_context(&connection, provider_id, model_id, true)
    }

    /// A stale result never overwrites the current configuration's evidence.
    /// Only fixed error codes are accepted; upstream response text is forbidden.
    pub(crate) fn save_probe_result(
        &self,
        context: &ProbeContext,
        state: &str,
        error_code: Option<&str>,
    ) -> Result<bool, String> {
        validate_model_id(&context.upstream_model_id)?;
        if !matches!(state, "supported" | "unsupported" | "failed")
            || (state == "supported") != error_code.is_none()
            || error_code.is_some_and(|code| {
                code.is_empty()
                    || code.len() > 128
                    || !code
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
            })
        {
            return Err("invalid_image_probe_result".to_string());
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "model_exposure_database_lock_failed")?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("model_probe_transaction_failed:{error}"))?;
        let current = read_probe_context(
            &transaction,
            &context.provider_id,
            &context.upstream_model_id,
            true,
        );
        if current.as_ref().ok() != Some(context) {
            return Ok(false);
        }
        transaction.execute(
            "INSERT INTO provider_model_image_probes
                (provider_id, provider_revision_id, upstream_model_id, wire_api, contract_version, state, checked_at, error_code)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(provider_revision_id, upstream_model_id, wire_api, contract_version)
             DO UPDATE SET state=excluded.state, checked_at=excluded.checked_at, error_code=excluded.error_code",
            params![context.provider_id, context.provider_revision_id, context.upstream_model_id, context.wire_api, IMAGE_PROBE_CONTRACT_VERSION, state, unix_now_seconds(), error_code]
        ).map_err(|error| format!("model_probe_write_failed:{error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("model_probe_commit_failed:{error}"))?;
        Ok(true)
    }
}

fn validate_model_id(model_id: &str) -> Result<(), String> {
    if model_id.trim() != model_id || !is_selectable_model_id(model_id) {
        return Err("model_not_exposable".to_string());
    }
    Ok(())
}

fn assert_provider_exists(connection: &Connection, provider_id: &str) -> Result<(), String> {
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM api_key_providers WHERE id=?1)",
            [provider_id],
            |row| row.get(0),
        )
        .map_err(|error| format!("model_exposure_provider_read_failed:{error}"))?;
    if !exists {
        return Err("provider_not_found".to_string());
    }
    Ok(())
}

fn read_probe_context(
    connection: &Connection,
    provider_id: &str,
    model_id: &str,
    require_exposed: bool,
) -> Result<ProbeContext, String> {
    if require_exposed {
        assert_model_exposed(connection, provider_id, model_id)?;
    }
    connection
        .query_row(
            "SELECT revision.id, revision.wire_api
         FROM api_key_providers provider
         JOIN api_key_provider_revisions revision ON revision.id=provider.current_revision_id
         JOIN provider_connectivity_reports report ON report.provider_id=provider.id
         JOIN provider_model_catalog_revisions catalog ON catalog.id=report.catalog_revision_id
         WHERE provider.id=?1 AND provider.configured=1 AND revision.state='active'
           AND report.wire_api=revision.wire_api AND report.text_check='ready'
           AND catalog.provider_revision_id=revision.id AND catalog.state='ready'",
            [provider_id],
            |row| {
                Ok(ProbeContext {
                    provider_id: provider_id.to_string(),
                    provider_revision_id: row.get(0)?,
                    upstream_model_id: model_id.to_string(),
                    wire_api: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(|error| format!("model_probe_context_read_failed:{error}"))?
        .ok_or_else(|| "provider_protocol_unconfirmed".to_string())
}

fn read_summary(
    connection: &Connection,
    provider_id: &str,
    model_id: &str,
) -> Result<ModelExposureSummary, String> {
    let exposed = connection.query_row("SELECT exposed FROM provider_model_exposures WHERE provider_id=?1 AND upstream_model_id=?2", params![provider_id, model_id], |row| row.get::<_, bool>(0))
        .optional().map_err(|error| format!("model_exposure_read_failed:{error}"))?.unwrap_or(false);
    let evidence = connection.query_row(
        "SELECT probe.state, probe.checked_at, probe.error_code
         FROM provider_model_image_probes probe
         JOIN api_key_providers provider ON provider.id=probe.provider_id AND provider.current_revision_id=probe.provider_revision_id
         JOIN api_key_provider_revisions revision ON revision.id=probe.provider_revision_id AND revision.wire_api=probe.wire_api
         WHERE probe.provider_id=?1 AND probe.upstream_model_id=?2 AND probe.contract_version=?3
           AND provider.configured=1 AND revision.state='active'",
        params![provider_id, model_id, IMAGE_PROBE_CONTRACT_VERSION],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, Option<String>>(2)?))
    ).optional().map_err(|error| format!("model_probe_read_failed:{error}"))?;
    let mut statement = connection
        .prepare(
            "SELECT DISTINCT preset.name FROM launch_preset_model_selections selection
         JOIN model_route_identities identity ON identity.id=selection.route_identity_id
         JOIN launch_presets preset ON preset.id=selection.launch_preset_id
         WHERE identity.source_kind='api_provider' AND identity.api_key_provider_id=?1
           AND identity.upstream_model_id=?2 AND preset.deleted_at IS NULL
         ORDER BY preset.name",
        )
        .map_err(|error| format!("model_exposure_references_read_failed:{error}"))?;
    let referenced_by = statement
        .query_map(params![provider_id, model_id], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| format!("model_exposure_references_read_failed:{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("model_exposure_references_decode_failed:{error}"))?;
    let (image_probe_state, image_probe_checked_at, image_probe_error_code) = match evidence {
        Some((state, checked_at, code)) => (state, Some(checked_at), code),
        None => ("unknown".to_string(), None, None),
    };
    Ok(ModelExposureSummary {
        provider_id: provider_id.to_string(),
        upstream_model_id: model_id.to_string(),
        exposed,
        image_probe_state,
        image_probe_checked_at,
        image_probe_error_code,
        referenced_by,
    })
}

/// Use on the caller's existing transaction/connection. Do not acquire another
/// store lock while editing a workspace.
pub(crate) fn assert_model_exposed(
    connection: &Connection,
    provider_id: &str,
    model_id: &str,
) -> Result<(), String> {
    validate_model_id(model_id)?;
    let exposed: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM provider_model_exposures WHERE provider_id=?1 AND upstream_model_id=?2 AND exposed=1)",
        params![provider_id, model_id], |row| row.get(0)
    ).map_err(|error| format!("model_exposure_read_failed:{error}"))?;
    if !exposed {
        return Err("model_not_exposed".to_string());
    }
    Ok(())
}

pub(crate) fn image_supported(
    connection: &Connection,
    provider_id: &str,
    revision_id: &str,
    model_id: &str,
    wire_api: &str,
) -> Result<bool, String> {
    validate_model_id(model_id)?;
    connection.query_row(
        "SELECT EXISTS(
          SELECT 1 FROM provider_model_image_probes probe
          JOIN api_key_providers provider ON provider.id=probe.provider_id AND provider.current_revision_id=probe.provider_revision_id
          JOIN api_key_provider_revisions revision ON revision.id=probe.provider_revision_id AND revision.wire_api=probe.wire_api
          JOIN provider_model_exposures exposure ON exposure.provider_id=probe.provider_id AND exposure.upstream_model_id=probe.upstream_model_id
          WHERE probe.provider_id=?1 AND probe.provider_revision_id=?2 AND probe.upstream_model_id=?3
            AND probe.wire_api=?4 AND probe.contract_version=?5 AND probe.state='supported'
            AND exposure.exposed=1 AND provider.configured=1 AND revision.state='active'
        )", params![provider_id, revision_id, model_id, wire_api, IMAGE_PROBE_CONTRACT_VERSION], |row| row.get(0)
    ).map_err(|error| format!("model_probe_capability_read_failed:{error}"))
}
