//! éç¦» home ç API Key ç»å½æä»¶ã
//!
//! åªå R Codex èªæ auth.jsonï¼æç»ç¨æ·é»è®¤ CODEX_HOMEã
//! ç±å¯å¨åå¤è°ç¨ã

use serde_json::json;
use std::path::Path;

/// Write an API-key login into an R Codex-managed Codex home.
///
/// This is the only login file R Codex is allowed to create. It must never point
/// at the user's default CODEX_HOME; Finder/Dock Codex keeps its own auth.json.
pub fn project_isolated_auth_json(
    isolated_home: &Path,
    default_home: &Path,
    api_key: &str,
) -> Result<(), String> {
    if api_key.trim().is_empty()
        || api_key.trim() != api_key
        || api_key.chars().any(char::is_control)
    {
        return Err("isolated_auth_key_invalid".to_string());
    }
    let isolated = isolated_home
        .canonicalize()
        .map_err(|_| "isolated_auth_home_invalid".to_string())?;
    if default_home.exists() {
        if let Ok(default) = default_home.canonicalize() {
            if isolated.starts_with(&default) || default.starts_with(&isolated) {
                return Err("isolated_auth_refuses_default_home".to_string());
            }
        }
    }
    if !isolated.is_dir() {
        return Err("isolated_auth_home_invalid".to_string());
    }
    let body = serde_json::to_vec(&json!({ "OPENAI_API_KEY": api_key }))
        .map_err(|_| "isolated_auth_encode_failed".to_string())?;
    super::write_private_atomically_overwrite(&isolated.join("auth.json"), &body, "isolated_auth")
}

#[cfg(test)]
mod tests {
    use super::project_isolated_auth_json;
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn writes_api_key_login_and_refuses_default_home() {
        let root = std::env::temp_dir().join(format!("3r-isolated-auth-{}", Uuid::new_v4()));
        let isolated = root.join("isolated");
        let default = root.join("default");
        fs::create_dir_all(&isolated).unwrap();
        fs::create_dir_all(&default).unwrap();
        project_isolated_auth_json(&isolated, &default, "sk-isolated").unwrap();
        let written = fs::read_to_string(isolated.join("auth.json")).unwrap();
        assert!(written.contains("sk-isolated"));
        assert_eq!(
            project_isolated_auth_json(&default, &default, "sk-isolated").unwrap_err(),
            "isolated_auth_refuses_default_home"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
