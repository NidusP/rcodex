//! 隔离 Codex 配置投影：目录、profile、Computer Use 和偏好播种。
//!
//! 不启动进程、不读用户默认认证、不把官方 Authorization 发给第三方。
//! 由 managed_launch 启动准备、system command 和 isolated_auth 调用。

mod catalog;
#[cfg(any(test, target_os = "macos", target_os = "windows"))]
mod computer_use;
mod io;
mod isolated_auth;
mod materialize;
mod preferences;
mod probe;

#[cfg(test)]
mod tests_catalog;
#[cfg(test)]
mod tests_computer_use;
#[cfg(test)]
mod tests_preferences;

pub use isolated_auth::project_isolated_auth_json;

pub const MANAGED_MODEL_PROVIDER_ID: &str = "rcodex";
pub const MANAGED_GATEWAY_DISPLAY_NAME: &str = "R Codex Gateway";
pub const INSTANCE_GATEWAY_HEADER: &str = "X-RCodex-Instance-Key";
pub const INSTANCE_GATEWAY_ENV: &str = "RCODEX_INSTANCE_KEY";
pub const LEGACY_INSTANCE_GATEWAY_HEADER: &str = "X-3R-Instance-Key";

pub(super) const MAX_BUNDLED_CATALOG_BYTES: usize = 8 * 1024 * 1024;

#[cfg(test)]
pub use catalog::{build_version_matched_catalog, build_version_matched_catalog_with_auto_review};
pub use catalog::{
    build_version_matched_catalog_for_version, codex_catalog_compatibility, read_bundled_catalog,
};
#[cfg(all(not(test), any(target_os = "macos", target_os = "windows")))]
pub use computer_use::materialize_isolated_computer_use_mcp;
#[cfg(any(test, target_os = "windows"))]
pub(crate) use computer_use::{
    materialize_windows_computer_use_from_cli, windows_app_dir_from_cli,
};
#[cfg(test)]
pub use materialize::materialize_managed_codex_config;
pub use materialize::{
    materialize_managed_app_config, materialize_managed_codex_config_with_auto_review,
};
pub use preferences::{
    ensure_isolated_local_plugins, keep_catalog_reasoning_efforts_selectable,
    keep_managed_permission_selector_available, seed_default_codex_preferences,
    sync_default_codex_preferences,
};
pub use probe::{verify_catalog_models_with_codex, verify_managed_profile_with_codex};

pub(crate) use io::write_private_atomically_overwrite;

#[cfg(test)]
pub use catalog::CompactContract;
#[cfg(test)]
pub(crate) use computer_use::{
    desktop_computer_use_app, install_isolated_computer_use_helper, isolated_computer_use_app,
    isolated_computer_use_client, materialize_isolated_computer_use_mcp_at,
    materialize_isolated_computer_use_mcp_from_bundle, COMPUTER_USE_CLIENT_RELATIVE,
    DESKTOP_CUA_NODE, DESKTOP_CUA_NODE_REPL, DESKTOP_CUA_REPL, ISOLATED_COMPUTER_USE_APP,
};
#[cfg(test)]
pub(crate) use probe::sanitize_probe_summary;
