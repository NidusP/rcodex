//! 隔离 Computer Use MCP 与已签名 helper 拷贝。
//!
//! 不读用户默认 ~/.codex/computer-use，不覆盖已经可用的隔离 helper。
//! 仅 API Key 受管理 App 启动准备调用。

#[cfg(target_os = "macos")]
use std::process::Command;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

use crate::codex_installation::{
    bundled_codex_cli_candidates, codex_app_bundle_candidates, codex_app_bundle_from_cli,
    resolve_codex_executable,
};

use super::io::{
    config_path_text, join_config_path, read_regular_toml, reject_link, sync_parent_directory,
    write_private_atomically_overwrite,
};

pub(crate) const DESKTOP_CUA_NODE: &str = "Contents/Resources/cua_node/bin/node";
pub(crate) const DESKTOP_CUA_NODE_REPL: &str = "Contents/Resources/cua_node/bin/node_repl";
pub(crate) const DESKTOP_CUA_REPL: &str =
    "Contents/Resources/cua_node/lib/node_modules/@oai/cua-repl/bin/cua-repl.mjs";
pub(super) const DESKTOP_COMPUTER_USE_APP: &str =
    "Contents/Resources/cua_node/lib/node_modules/@oai/sky/Codex Computer Use.app";
pub(crate) const COMPUTER_USE_CLIENT_RELATIVE: &str =
    "Contents/SharedSupport/SkyComputerUseClient.app/Contents/MacOS/SkyComputerUseClient";
pub(crate) const ISOLATED_COMPUTER_USE_APP: &str = "computer-use/Codex Computer Use.app";

/// Install the only MCP that an API-key managed App may receive.  It is a
/// local, signed Computer Use client rooted in this isolated CODEX_HOME; this
/// deliberately does not import any default-home MCP configuration.
///
/// The signed `Codex Computer Use.app` is copied from the resolved desktop
/// bundle into `computer-use/` when the isolated client is missing. Interpreters
/// (`node`, `node_repl`, `cua_repl`, nested CLI) stay as package-internal
/// references. An existing isolated helper is never overwritten, and
/// `~/.codex/computer-use` is never a copy source.
#[cfg_attr(test, allow(dead_code))]
pub fn materialize_isolated_computer_use_mcp(codex_home: &Path) -> Result<(), String> {
    let codex_home = codex_home
        .canonicalize()
        .map_err(|error| format!("managed_codex_home_invalid:{error}"))?;
    let bundle = resolve_computer_use_desktop_bundle()?;
    materialize_isolated_computer_use_mcp_from_bundle(&codex_home, &bundle)
}

pub(crate) fn materialize_isolated_computer_use_mcp_from_bundle(
    codex_home: &Path,
    bundle: &Path,
) -> Result<(), String> {
    install_isolated_computer_use_helper(codex_home, bundle)?;
    let node = join_config_path(bundle, [DESKTOP_CUA_NODE]);
    let node_repl = join_config_path(bundle, [DESKTOP_CUA_NODE_REPL]);
    let cua_repl = join_config_path(bundle, [DESKTOP_CUA_REPL]);
    let codex_cli = bundled_codex_cli_candidates(bundle)
        .into_iter()
        .find(|path| path.is_file())
        .unwrap_or_else(|| {
            join_config_path(
                bundle,
                ["Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex"],
            )
        });
    let client = isolated_computer_use_client(codex_home);
    materialize_isolated_computer_use_mcp_at(
        codex_home, &client, &node, &node_repl, &cua_repl, &codex_cli,
    )
}

pub(crate) fn isolated_computer_use_app(codex_home: &Path) -> PathBuf {
    join_config_path(codex_home, [ISOLATED_COMPUTER_USE_APP])
}

pub(crate) fn isolated_computer_use_client(codex_home: &Path) -> PathBuf {
    join_config_path(
        &isolated_computer_use_app(codex_home),
        [COMPUTER_USE_CLIENT_RELATIVE],
    )
}

pub(crate) fn desktop_computer_use_app(bundle: &Path) -> PathBuf {
    join_config_path(bundle, [DESKTOP_COMPUTER_USE_APP])
}

#[cfg_attr(test, allow(dead_code))]
fn resolve_computer_use_desktop_bundle() -> Result<PathBuf, String> {
    let mut considered = Vec::new();
    if let Ok(cli) = resolve_codex_executable() {
        if let Ok(bundle) = codex_app_bundle_from_cli(&cli) {
            considered.push(bundle);
        }
    }
    considered.extend(codex_app_bundle_candidates());
    let mut seen = BTreeSet::new();
    for bundle in considered {
        if !seen.insert(bundle.clone()) {
            continue;
        }
        if join_config_path(
            &desktop_computer_use_app(&bundle),
            [COMPUTER_USE_CLIENT_RELATIVE],
        )
        .is_file()
        {
            return Ok(bundle);
        }
    }
    Err("managed_computer_use_client_missing".to_string())
}

/// Copy the signed helper out of the desktop bundle when this isolated home
/// does not already have one. Never reads `~/.codex/computer-use`.
pub(crate) fn install_isolated_computer_use_helper(
    codex_home: &Path,
    bundle: &Path,
) -> Result<(), String> {
    let computer_use_dir = join_config_path(codex_home, ["computer-use"]);
    let destination = isolated_computer_use_app(codex_home);
    let dest_client = isolated_computer_use_client(codex_home);
    reject_path_symlink(&computer_use_dir)?;
    reject_path_symlink(&destination)?;
    reject_path_symlink(&dest_client)?;
    if dest_client.is_file() {
        ensure_computer_use_path_inside_home(codex_home, &dest_client)?;
        return Ok(());
    }
    let source = desktop_computer_use_app(bundle);
    reject_path_symlink(&source)?;
    if source_is_forbidden_default_computer_use(&source) {
        return Err("managed_computer_use_default_home_rejected".to_string());
    }
    if !join_config_path(&source, [COMPUTER_USE_CLIENT_RELATIVE]).is_file() {
        return Err("managed_computer_use_client_missing".to_string());
    }
    if destination.exists() {
        // Incomplete leftover is not a reusable helper. Remove it so the
        // signed bundle can be installed; a working client is never replaced.
        let metadata = fs::symlink_metadata(&destination)
            .map_err(|error| format!("managed_computer_use_install_failed:{error}"))?;
        if metadata.file_type().is_symlink() {
            return Err("managed_file_symlink_rejected".to_string());
        }
        if metadata.is_dir() {
            fs::remove_dir_all(&destination)
                .map_err(|error| format!("managed_computer_use_install_failed:{error}"))?;
        } else {
            fs::remove_file(&destination)
                .map_err(|error| format!("managed_computer_use_install_failed:{error}"))?;
        }
    }
    let parent = destination
        .parent()
        .ok_or_else(|| "managed_computer_use_install_failed".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("managed_computer_use_install_failed:{error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
    }
    let temporary = parent.join(format!(
        ".rcodex-computer-use-install-{}.app",
        Uuid::new_v4()
    ));
    let result = (|| {
        copy_signed_app_bundle(&source, &temporary)?;
        if !join_config_path(&temporary, [COMPUTER_USE_CLIENT_RELATIVE]).is_file() {
            return Err("managed_computer_use_install_failed".to_string());
        }
        fs::rename(&temporary, &destination)
            .map_err(|error| format!("managed_computer_use_install_failed:{error}"))?;
        sync_parent_directory(&destination, "managed_computer_use_install")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

fn copy_signed_app_bundle(source: &Path, destination: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("/usr/bin/ditto")
            .arg(source)
            .arg(destination)
            .output()
            .map_err(|error| format!("managed_computer_use_install_failed:{error}"))?;
        if !output.status.success() {
            return Err("managed_computer_use_install_failed".to_string());
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (source, destination);
        Err("managed_computer_use_client_missing".to_string())
    }
}

fn source_is_forbidden_default_computer_use(path: &Path) -> bool {
    path_has_default_computer_use_components(path)
        || path
            .canonicalize()
            .map(|canonical| path_has_default_computer_use_components(&canonical))
            .unwrap_or(false)
}

fn path_has_default_computer_use_components(path: &Path) -> bool {
    let names: Vec<_> = path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(name) => Some(name.to_os_string()),
            _ => None,
        })
        .collect();
    names
        .windows(2)
        .any(|window| window[0] == ".codex" && window[1] == "computer-use")
}

fn ensure_computer_use_path_inside_home(codex_home: &Path, path: &Path) -> Result<(), String> {
    if source_is_forbidden_default_computer_use(path) {
        return Err("managed_computer_use_default_home_rejected".to_string());
    }
    let home = codex_home
        .canonicalize()
        .map_err(|error| format!("managed_codex_home_invalid:{error}"))?;
    let canonical = path
        .canonicalize()
        .map_err(|_| "managed_computer_use_default_home_rejected".to_string())?;
    if !canonical.starts_with(&home) {
        return Err("managed_computer_use_default_home_rejected".to_string());
    }
    Ok(())
}

fn reject_path_symlink(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err("managed_file_symlink_rejected".to_string())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("managed_file_stat_failed".to_string()),
    }
}

fn node_repl_module_dirs(node: &Path) -> PathBuf {
    if let Some(bin_dir) = node.parent() {
        let beside = join_config_path(bin_dir, ["node_modules"]);
        if beside.is_dir() {
            return beside;
        }
        if let Some(cua_node) = bin_dir.parent() {
            return join_config_path(cua_node, ["lib/node_modules"]);
        }
    }
    join_config_path(node, ["node_modules"])
}

pub(crate) fn materialize_isolated_computer_use_mcp_at(
    codex_home: &Path,
    client: &Path,
    node: &Path,
    node_repl: &Path,
    cua_repl: &Path,
    codex_cli: &Path,
) -> Result<(), String> {
    materialize_isolated_computer_use_mcp_with_service(
        codex_home,
        client,
        node,
        node_repl,
        cua_repl,
        codex_cli,
        &isolated_computer_use_app(codex_home),
    )
}

fn materialize_isolated_computer_use_mcp_with_service(
    codex_home: &Path,
    client: &Path,
    node: &Path,
    node_repl: &Path,
    cua_repl: &Path,
    codex_cli: &Path,
    service_path: &Path,
) -> Result<(), String> {
    for (label, path) in [
        ("managed_computer_use_client_missing", client),
        ("managed_computer_use_node_missing", node),
        ("managed_computer_use_node_repl_missing", node_repl),
        ("managed_computer_use_cua_repl_missing", cua_repl),
        ("managed_computer_use_codex_cli_missing", codex_cli),
    ] {
        if !path.is_file() {
            return Err(label.to_string());
        }
    }
    let config_path = join_config_path(codex_home, ["config.toml"]);
    reject_link(&config_path)?;
    let mut config = read_regular_toml(&config_path)?.unwrap_or_default();
    let servers = config
        .entry("mcp_servers")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let servers = servers
        .as_table_mut()
        .ok_or_else(|| "managed_mcp_servers_invalid".to_string())?;
    let trusted_code = format!(
        "{}:{}",
        config_path_text(codex_home),
        config_path_text(node.parent().unwrap()),
    );
    let trusted_services = r#"{"browser":"@oai/browser-desktop/service","sky":"@oai/sky/service"}"#;
    let mut node_env = toml::Table::new();
    for (key, value) in [
        ("NODE_REPL_NATIVE_PIPE_CONNECT_TIMEOUT_MS", "1000"),
        ("BROWSER_USE_AVAILABLE_BACKENDS", "chrome,iab"),
        ("BROWSER_USE_TINYSKY_ENABLED", "1"),
        ("BROWSER_USE_CODEX_APP_BUILD_FLAVOR", "prod"),
        ("BROWSER_USE_CODEX_APP_VERSION", "26.917.62051"),
        ("CUA_REPL_ENABLED_SURFACES", "browser,computer"),
    ] {
        node_env.insert(key.to_string(), toml::Value::String(value.to_string()));
    }
    node_env.insert(
        "NODE_REPL_NODE_MODULE_DIRS".to_string(),
        toml::Value::String(config_path_text(&node_repl_module_dirs(node))),
    );
    node_env.insert(
        "NODE_REPL_NODE_PATH".to_string(),
        toml::Value::String(config_path_text(node)),
    );
    node_env.insert(
        "CODEX_HOME".to_string(),
        toml::Value::String(config_path_text(codex_home)),
    );
    node_env.insert(
        "NODE_REPL_TRUSTED_CODE_PATHS".to_string(),
        toml::Value::String(trusted_code.clone()),
    );
    node_env.insert(
        "NODE_REPL_TRUSTED_SERVICES".to_string(),
        toml::Value::String(trusted_services.to_string()),
    );
    node_env.insert(
        "SKY_CUA_SERVICE_PATH".to_string(),
        toml::Value::String(config_path_text(service_path)),
    );
    node_env.insert(
        "CUA_REPL_NODE_REPL_PATH".to_string(),
        toml::Value::String(config_path_text(node_repl)),
    );
    node_env.insert(
        "CODEX_CLI_PATH".to_string(),
        toml::Value::String(config_path_text(codex_cli)),
    );
    let mut node_server = toml::Table::new();
    node_server.insert(
        "command".to_string(),
        toml::Value::String(config_path_text(node_repl)),
    );
    node_server.insert("enabled".to_string(), toml::Value::Boolean(true));
    node_server.insert("startup_timeout_sec".to_string(), toml::Value::Integer(120));
    node_server.insert("env".to_string(), toml::Value::Table(node_env.clone()));
    servers.insert("node_repl".to_string(), toml::Value::Table(node_server));
    let mut cua_server = toml::Table::new();
    cua_server.insert(
        "command".to_string(),
        toml::Value::String(config_path_text(node)),
    );
    cua_server.insert(
        "args".to_string(),
        toml::Value::Array(vec![toml::Value::String(config_path_text(cua_repl))]),
    );
    cua_server.insert("enabled".to_string(), toml::Value::Boolean(true));
    cua_server.insert("startup_timeout_sec".to_string(), toml::Value::Integer(120));
    cua_server.insert("env".to_string(), toml::Value::Table(node_env));
    servers.insert("cua_repl".to_string(), toml::Value::Table(cua_server));
    servers.remove("computer-use");
    if let Some(toml::Value::Array(notify)) = config.get_mut("notify") {
        if notify.len() >= 1 {
            notify[0] = toml::Value::String(config_path_text(client));
        }
    }
    let encoded = toml::to_string(&config)
        .map_err(|_| "managed_computer_use_config_encode_failed".to_string())?;
    write_private_atomically_overwrite(
        &config_path,
        encoded.as_bytes(),
        "managed_computer_use_config",
    )
}

#[cfg(any(test, target_os = "windows"))]
const WINDOWS_COMPUTER_USE_RELATIVE: &str = "computer-use/windows";

#[cfg(any(test, target_os = "windows"))]
const WINDOWS_HELPER_DIRECTORIES: [&str; 4] = [
    "resources/cua_node/bin/node_modules/@oai/sky/bin/windows",
    "resources/cua_node/bin/node_modules/@oai/cua/bin/windows",
    "resources/cua_node/lib/node_modules/@oai/sky/bin/windows",
    "resources/cua_node/lib/node_modules/@oai/cua/bin/windows",
];

#[cfg(any(test, target_os = "windows"))]
const WINDOWS_CUA_REPL_RELATIVE: [&str; 2] = [
    "resources/cua_node/bin/node_modules/@oai/cua-repl/bin/cua-repl.mjs",
    "resources/cua_node/lib/node_modules/@oai/cua-repl/bin/cua-repl.mjs",
];

#[cfg(any(test, target_os = "windows"))]
pub(crate) fn windows_app_dir_from_cli(cli: &Path) -> Option<PathBuf> {
    let text = cli.to_string_lossy();
    let folded = text.replace('/', "\\").to_ascii_lowercase();
    let marker = "\\resources\\";
    if let Some(index) = folded.find(marker) {
        let app_dir = text.get(..index)?.trim();
        if !app_dir.is_empty() {
            return Some(PathBuf::from(app_dir.to_string()));
        }
    }
    let normalized = text.replace('/', "\\");
    let parent = normalized.rsplit_once('\\')?.0.trim();
    if parent.is_empty() {
        None
    } else {
        Some(PathBuf::from(parent.to_string()))
    }
}

#[cfg(any(test, target_os = "windows"))]
pub(crate) fn materialize_windows_computer_use_from_cli(
    codex_home: &Path,
    cli: &Path,
) -> Result<(), String> {
    let app_dir = windows_app_dir_from_cli(cli)
        .ok_or_else(|| "managed_computer_use_client_missing".to_string())?;
    let node = join_config_path(&app_dir, ["resources/cua_node/bin/node.exe"]);
    let node_repl = join_config_path(&app_dir, ["resources/cua_node/bin/node_repl.exe"]);
    let cua_repl = WINDOWS_CUA_REPL_RELATIVE
        .into_iter()
        .map(|relative| join_config_path(&app_dir, [relative]))
        .find(|path| path.is_file())
        .ok_or_else(|| "managed_computer_use_cua_repl_missing".to_string())?;
    let codex_cli = join_config_path(&app_dir, ["resources/codex.exe"]);
    let helper = find_windows_computer_use_helper(&app_dir)?;
    let service = install_windows_computer_use_helper(codex_home, &helper)?;
    materialize_isolated_computer_use_mcp_with_service(
        codex_home, &service, &node, &node_repl, &cua_repl, &codex_cli, &service,
    )
}

#[cfg(any(test, target_os = "windows"))]
fn find_windows_computer_use_helper(app_dir: &Path) -> Result<PathBuf, String> {
    for relative in WINDOWS_HELPER_DIRECTORIES {
        let directory = join_config_path(app_dir, [relative]);
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        let mut matches = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.starts_with("codex-compu") && name.ends_with(".exe") {
                matches.push(path);
            }
        }
        matches.sort();
        if let Some(path) = matches.into_iter().next() {
            return Ok(path);
        }
    }
    Err("managed_computer_use_client_missing".to_string())
}

#[cfg(any(test, target_os = "windows"))]
fn install_windows_computer_use_helper(
    codex_home: &Path,
    helper_exe: &Path,
) -> Result<PathBuf, String> {
    let source_dir = helper_exe
        .parent()
        .ok_or_else(|| "managed_computer_use_client_missing".to_string())?;
    reject_path_symlink(source_dir)?;
    reject_path_symlink(helper_exe)?;
    if source_is_forbidden_default_computer_use(source_dir)
        || source_is_forbidden_default_computer_use(helper_exe)
    {
        return Err("managed_computer_use_default_home_rejected".to_string());
    }
    let exe_name = helper_exe
        .file_name()
        .ok_or_else(|| "managed_computer_use_client_missing".to_string())?;
    let exe_name = exe_name.to_string_lossy();
    let destination_dir = join_config_path(codex_home, [WINDOWS_COMPUTER_USE_RELATIVE]);
    let destination_exe = join_config_path(&destination_dir, [exe_name.as_ref()]);
    reject_path_symlink(&destination_dir)?;
    reject_path_symlink(&destination_exe)?;
    if destination_exe.is_file() {
        ensure_computer_use_path_inside_home(codex_home, &destination_exe)?;
        return Ok(destination_exe);
    }
    if destination_dir.exists() {
        fs::remove_dir_all(&destination_dir)
            .map_err(|error| format!("managed_computer_use_install_failed:{error}"))?;
    }
    let parent = destination_dir
        .parent()
        .ok_or_else(|| "managed_computer_use_install_failed".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("managed_computer_use_install_failed:{error}"))?;
    copy_directory_without_symlinks(source_dir, &destination_dir)?;
    if !destination_exe.is_file() {
        return Err("managed_computer_use_install_failed".to_string());
    }
    ensure_computer_use_path_inside_home(codex_home, &destination_exe)?;
    Ok(destination_exe)
}

#[cfg(any(test, target_os = "windows"))]
fn copy_directory_without_symlinks(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination)
        .map_err(|error| format!("managed_computer_use_install_failed:{error}"))?;
    for entry in fs::read_dir(source)
        .map_err(|error| format!("managed_computer_use_install_failed:{error}"))?
    {
        let entry =
            entry.map_err(|error| format!("managed_computer_use_install_failed:{error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("managed_computer_use_install_failed:{error}"))?;
        if file_type.is_symlink() {
            return Err("managed_file_symlink_rejected".to_string());
        }
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_directory_without_symlinks(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target)
                .map_err(|error| format!("managed_computer_use_install_failed:{error}"))?;
        }
    }
    Ok(())
}
