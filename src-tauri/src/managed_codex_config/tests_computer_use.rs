//! 隔离 Computer Use helper 拷贝和 MCP 物化。

use std::{fs, path::Path};
use uuid::Uuid;

use super::*;

#[test]
fn isolated_computer_use_mcp_isolated_paths_and_services_are_materialized() {
    let root = std::env::temp_dir().join(format!("3r-cua-mcp-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let client = root.join("computer-use/SkyComputerUseClient");
    let node = root.join("bin/node");
    let node_repl = root.join("bin/node_repl");
    let cua_repl = root.join("bin/cua-repl.mjs");
    let codex_cli = root.join("bin/codex");
    for path in [&client, &node, &node_repl, &cua_repl, &codex_cli] {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"fixture").unwrap();
    }
    fs::write(
        root.join("config.toml"),
        "notify = [\"/Users/populus/.codex/computer-use/default\", \"turn-ended\"]\n[mcp_servers.cua_repl]\nenabled = false\n[mcp_servers.computer-use]\nenabled = true\n",
    )
    .unwrap();

    materialize_isolated_computer_use_mcp_at(
        &root, &client, &node, &node_repl, &cua_repl, &codex_cli,
    )
    .unwrap();
    let config: toml::Table = fs::read_to_string(root.join("config.toml"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(config["mcp_servers"].get("computer-use").is_none());
    assert_eq!(
        config["mcp_servers"]["node_repl"]["enabled"].as_bool(),
        Some(true)
    );
    assert_eq!(
        config["mcp_servers"]["cua_repl"]["enabled"].as_bool(),
        Some(true)
    );
    assert_eq!(
        config["mcp_servers"]["cua_repl"]["command"].as_str(),
        Some(node.to_str().unwrap())
    );
    assert_eq!(
        config["mcp_servers"]["node_repl"]["env"]["CODEX_HOME"].as_str(),
        Some(root.to_str().unwrap())
    );
    assert_eq!(
        config["mcp_servers"]["node_repl"]["env"]["CODEX_CLI_PATH"].as_str(),
        Some(codex_cli.to_str().unwrap())
    );
    assert_eq!(
        config["mcp_servers"]["node_repl"]["env"]["NODE_REPL_NODE_PATH"].as_str(),
        Some(node.to_str().unwrap())
    );
    assert_eq!(
        config["mcp_servers"]["node_repl"]["env"]["SKY_CUA_SERVICE_PATH"].as_str(),
        Some(root.join(ISOLATED_COMPUTER_USE_APP).to_str().unwrap())
    );
    assert_eq!(config["notify"][0].as_str(), Some(client.to_str().unwrap()));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn isolated_computer_use_mcp_fails_when_a_runtime_helper_is_missing() {
    let root = std::env::temp_dir().join(format!("3r-cua-mcp-missing-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let missing = root.join("missing");
    let error = materialize_isolated_computer_use_mcp_at(
        &root, &missing, &missing, &missing, &missing, &missing,
    )
    .unwrap_err();
    assert_eq!(error, "managed_computer_use_client_missing");
    fs::remove_dir_all(root).unwrap();
}

fn write_fake_file(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn write_fake_computer_use_app(app: &Path, marker: &[u8]) {
    write_fake_file(&app.join(COMPUTER_USE_CLIENT_RELATIVE), marker);
}

fn write_fake_desktop_bundle(bundle: &Path, helper_marker: Option<&[u8]>) {
    for relative in [DESKTOP_CUA_NODE, DESKTOP_CUA_NODE_REPL, DESKTOP_CUA_REPL] {
        write_fake_file(&bundle.join(relative), b"fixture");
    }
    write_fake_file(
        &bundle.join("Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex"),
        b"fixture",
    );
    if let Some(marker) = helper_marker {
        write_fake_computer_use_app(&desktop_computer_use_app(bundle), marker);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn isolated_computer_use_helper_is_copied_from_the_desktop_bundle() {
    let root = std::env::temp_dir().join(format!("3r-cua-install-{}", Uuid::new_v4()));
    let bundle = root.join("ChatGPT.app");
    let home = root.join("isolated-home");
    fs::create_dir_all(&home).unwrap();
    write_fake_desktop_bundle(&bundle, Some(b"bundle-helper"));
    fs::write(
        home.join("config.toml"),
        "notify = [\"/Users/populus/.codex/computer-use/default\", \"turn-ended\"]\n",
    )
    .unwrap();

    materialize_isolated_computer_use_mcp_from_bundle(&home, &bundle).unwrap();

    let dest_client = isolated_computer_use_client(&home);
    assert_eq!(fs::read(&dest_client).unwrap(), b"bundle-helper");
    let config: toml::Table = fs::read_to_string(home.join("config.toml"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        config["mcp_servers"]["node_repl"]["env"]["SKY_CUA_SERVICE_PATH"].as_str(),
        Some(isolated_computer_use_app(&home).to_str().unwrap())
    );
    assert_eq!(
        config["notify"][0].as_str(),
        Some(dest_client.to_str().unwrap())
    );
    assert!(!config["notify"][0]
        .as_str()
        .unwrap()
        .contains("/.codex/computer-use/"));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn isolated_computer_use_helper_is_not_overwritten_once_present() {
    let root = std::env::temp_dir().join(format!("3r-cua-keep-{}", Uuid::new_v4()));
    let bundle = root.join("ChatGPT.app");
    let home = root.join("isolated-home");
    fs::create_dir_all(&home).unwrap();
    write_fake_desktop_bundle(&bundle, Some(b"bundle-helper"));
    write_fake_computer_use_app(&isolated_computer_use_app(&home), b"authorized-isolated");

    materialize_isolated_computer_use_mcp_from_bundle(&home, &bundle).unwrap();

    assert_eq!(
        fs::read(isolated_computer_use_client(&home)).unwrap(),
        b"authorized-isolated"
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn isolated_computer_use_helper_missing_from_bundle_does_not_use_default_home() {
    let root = std::env::temp_dir().join(format!("3r-cua-no-default-{}", Uuid::new_v4()));
    let bundle = root.join("ChatGPT.app");
    let home = root.join("isolated-home");
    let default_helper = root.join(".codex/computer-use/Codex Computer Use.app");
    fs::create_dir_all(&home).unwrap();
    write_fake_desktop_bundle(&bundle, None);
    write_fake_computer_use_app(&default_helper, b"default-home-helper");

    let error = materialize_isolated_computer_use_mcp_from_bundle(&home, &bundle).unwrap_err();
    assert_eq!(error, "managed_computer_use_client_missing");
    assert!(!isolated_computer_use_app(&home).exists());
    assert_eq!(
        fs::read(default_helper.join(COMPUTER_USE_CLIENT_RELATIVE)).unwrap(),
        b"default-home-helper"
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn isolated_computer_use_helper_rejects_default_home_as_copy_source() {
    let root = std::env::temp_dir().join(format!("3r-cua-reject-default-{}", Uuid::new_v4()));
    let home = root.join("isolated-home");
    let default_bundle = root.join(".codex/computer-use/disguised.app");
    fs::create_dir_all(&home).unwrap();
    write_fake_desktop_bundle(&default_bundle, Some(b"default-home-helper"));

    let error = install_isolated_computer_use_helper(&home, &default_bundle).unwrap_err();
    assert_eq!(error, "managed_computer_use_default_home_rejected");
    assert!(!isolated_computer_use_app(&home).exists());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn isolated_computer_use_helper_rejects_a_symlink_into_default_home() {
    let root = std::env::temp_dir().join(format!("3r-cua-symlink-{}", Uuid::new_v4()));
    let bundle = root.join("ChatGPT.app");
    let home = root.join("isolated-home");
    let default_helper = root.join(".codex/computer-use");
    fs::create_dir_all(&home).unwrap();
    write_fake_desktop_bundle(&bundle, Some(b"bundle-helper"));
    write_fake_computer_use_app(
        &default_helper.join("Codex Computer Use.app"),
        b"default-home-helper",
    );
    std::os::unix::fs::symlink(&default_helper, home.join("computer-use")).unwrap();

    let error = materialize_isolated_computer_use_mcp_from_bundle(&home, &bundle).unwrap_err();
    assert_eq!(error, "managed_file_symlink_rejected");
    assert_eq!(
        fs::read(
            default_helper
                .join("Codex Computer Use.app")
                .join(COMPUTER_USE_CLIENT_RELATIVE)
        )
        .unwrap(),
        b"default-home-helper"
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn isolated_computer_use_helper_replaces_an_incomplete_leftover() {
    let root = std::env::temp_dir().join(format!("3r-cua-leftover-{}", Uuid::new_v4()));
    let bundle = root.join("ChatGPT.app");
    let home = root.join("isolated-home");
    fs::create_dir_all(&home).unwrap();
    write_fake_desktop_bundle(&bundle, Some(b"bundle-helper"));
    fs::create_dir_all(isolated_computer_use_app(&home).join("Contents")).unwrap();

    materialize_isolated_computer_use_mcp_from_bundle(&home, &bundle).unwrap();

    assert_eq!(
        fs::read(isolated_computer_use_client(&home)).unwrap(),
        b"bundle-helper"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn windows_store_computer_use_helper_is_copied_and_interpreters_stay_in_the_package() {
    let root = std::env::temp_dir().join(format!("3r-cua-windows-{}", Uuid::new_v4()));
    let app = root.join("app");
    let home = root.join("isolated-home");
    fs::create_dir_all(&home).unwrap();
    let helper =
        app.join("resources/cua_node/bin/node_modules/@oai/sky/bin/windows/codex-computer-use.exe");
    write_fake_file(&helper, b"sky-helper");
    write_fake_file(
        &helper.parent().unwrap().join("swift/arm64/runtime.dll"),
        b"runtime",
    );
    for relative in [
        "resources/cua_node/bin/node.exe",
        "resources/cua_node/bin/node_repl.exe",
        "resources/cua_node/bin/node_modules/@oai/cua-repl/bin/cua-repl.mjs",
        "resources/codex.exe",
    ] {
        write_fake_file(&app.join(relative), b"fixture");
    }
    fs::write(
        home.join("config.toml"),
        "notify = [\"old\", \"turn-ended\"]\n",
    )
    .unwrap();

    materialize_windows_computer_use_from_cli(&home, &app.join("resources/codex.exe")).unwrap();

    let copied = home.join("computer-use/windows/codex-computer-use.exe");
    assert_eq!(fs::read(&copied).unwrap(), b"sky-helper");
    assert_eq!(
        fs::read(home.join("computer-use/windows/swift/arm64/runtime.dll")).unwrap(),
        b"runtime"
    );
    let config: toml::Table = fs::read_to_string(home.join("config.toml"))
        .unwrap()
        .parse()
        .unwrap();
    let node = app.join("resources/cua_node/bin/node.exe");
    assert_eq!(
        config["mcp_servers"]["cua_repl"]["command"].as_str(),
        Some(node.to_str().unwrap())
    );
    assert_eq!(
        config["mcp_servers"]["node_repl"]["env"]["SKY_CUA_SERVICE_PATH"].as_str(),
        Some(copied.to_str().unwrap())
    );
    assert_eq!(config["notify"][0].as_str(), Some(copied.to_str().unwrap()));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn windows_cli_path_resolves_the_install_app_directory_case_insensitively() {
    let cli = Path::new(
        r"C:\Program Files\WindowsApps\OpenAI.Codex_26.1013.3656.0_arm64__2p2nqsd0c76g0\App\Resources\codex.exe",
    );
    assert_eq!(
        windows_app_dir_from_cli(cli),
        Some(
            Path::new(
                r"C:\Program Files\WindowsApps\OpenAI.Codex_26.1013.3656.0_arm64__2p2nqsd0c76g0\App"
            )
            .to_path_buf()
        )
    );
}

#[test]
fn windows_flat_cli_falls_back_to_parent_directory() {
    let cli = Path::new(r"D:\Apps\Codex\codex.exe");
    assert_eq!(
        windows_app_dir_from_cli(cli),
        Some(Path::new(r"D:\Apps\Codex").to_path_buf())
    );
}
