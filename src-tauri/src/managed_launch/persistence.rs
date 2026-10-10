//! 启动配置依赖行的 SQLite 写入。
//!
//! 不校验模型合同，不创建实例目录。由 binding 在同一事务里调用。

use rusqlite::{params, Connection};

pub(super) fn insert_dependency(
    transaction: &rusqlite::Transaction<'_>,
    configuration_id: &str,
    kind: &str,
    id: &str,
    revision_id: Option<&str>,
) -> Result<(), String> {
    transaction
        .execute(
            "INSERT INTO launch_configuration_dependencies (
                launch_configuration_revision_id, resource_kind,
                resource_id, resource_revision_id
             ) VALUES (?1, ?2, ?3, ?4)",
            params![configuration_id, kind, id, revision_id],
        )
        .map_err(|error| format!("managed_dependency_insert_failed:{error}"))?;
    Ok(())
}

pub(super) fn query_strings(
    connection: &Connection,
    sql: &str,
    parameter: &str,
) -> Result<Vec<String>, String> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| format!("managed_query_failed:{error}"))?;
    if parameter.is_empty() {
        return statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| format!("managed_query_failed:{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("managed_query_decode_failed:{error}"));
    }
    let values = statement
        .query_map([parameter], |row| row.get::<_, String>(0))
        .map_err(|error| format!("managed_query_failed:{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("managed_query_decode_failed:{error}"));
    values
}
