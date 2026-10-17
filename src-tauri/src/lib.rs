//! R Codex Tauri 库入口。
//!
//! 声明领域模块、装配 State、注册 IPC，并处理窗口与托盘生命周期。
//! 命令实现在 `commands/`；不要在这里继续堆业务函数。

mod balance;
mod channel_probe;
mod codex_control;
pub mod codex_installation;
mod connectivity;
mod conversation_archive;
mod database;
mod diagnostics;
pub mod launcher_protocol;
pub use codex_control::{CredentialKind, EntryKind, InstanceState};
mod commands;
pub mod managed_app_launch;
mod managed_codex_config;
pub mod managed_gateway;
mod managed_launch;
mod managed_spawn;
mod model_family_profile;
mod official_accounts;
mod official_auth;
mod providers;
mod route_store;
mod routing;
mod secrets;
mod session_import;
mod settings;
mod special_models;
mod support;
pub use support::{command_without_console, user_home};
#[cfg(target_os = "macos")]
mod macos_quit;
mod usage;

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{Emitter, Manager, RunEvent, WindowEvent};

use balance::BalanceStore;
use channel_probe::ProbeTicketStore;
use codex_control::CodexControlStore;
use commands::*;
use conversation_archive::ConversationArchive;
use database::Database;
use diagnostics::DiagnosticsStore;
use managed_gateway::ManagedGatewayClient;
use managed_launch::ManagedLaunchStore;
use official_accounts::OfficialAccountService;
use providers::ProviderStore;
use settings::SettingsStore;
use usage::UsageStore;

/// Kept as an explicit release gate while official OAuth contracts remain
/// unavailable in the product surface. Existing account data is preserved.
pub(crate) const OFFICIAL_ACCOUNT_AUTH_ENABLED: bool = false;

fn reveal_main_window(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn park_main_window(window: &tauri::Window) {
    let _ = window.hide();
    #[cfg(target_os = "macos")]
    {
        let _ = window
            .app_handle()
            .set_activation_policy(tauri::ActivationPolicy::Accessory);
    }
}

/// Updater and `process.restart` use Tauri's restart exit code. Dev hot reload
/// kills the control process from outside, so this function is not consulted.
/// Menu-bar quit calls `exit(0)` and must stop instances and the gateway.
pub(crate) fn exit_preserves_instances(code: Option<i32>) -> bool {
    code == Some(tauri::RESTART_EXIT_CODE)
}

static EXIT_CLEANUP_DONE: AtomicBool = AtomicBool::new(false);
static EXIT_PRESERVES_RUNTIME: AtomicBool = AtomicBool::new(false);

fn perform_exit_cleanup(app: &tauri::AppHandle, code: Option<i32>) -> Result<(), String> {
    if exit_preserves_instances(code) {
        EXIT_PRESERVES_RUNTIME.store(true, Ordering::SeqCst);
        return Ok(());
    }
    if EXIT_PRESERVES_RUNTIME.load(Ordering::SeqCst) {
        return Ok(());
    }
    if EXIT_CLEANUP_DONE.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let result = tauri::async_runtime::block_on(stop_active_instances_and_gateway(
        app.state::<CodexControlStore>().inner(),
        app.state::<ManagedGatewayClient>().inner(),
        app.state::<ManagedLaunchStore>().inner(),
    ));
    if result.is_err() {
        EXIT_CLEANUP_DONE.store(false, Ordering::SeqCst);
    }
    result
}

fn install_menu_bar_icon(app: &tauri::App) {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let icon = match tauri::image::Image::from_bytes(include_bytes!("../icons/tray-icon.png")) {
        Ok(icon) => icon,
        Err(error) => {
            eprintln!("R Codex menu bar icon skipped: {error}");
            return;
        }
    };
    let Ok(show) = MenuItem::with_id(app, "tray-show", "显示", true, None::<&str>) else {
        eprintln!("R Codex menu bar icon skipped: show item");
        return;
    };
    let Ok(quit) = MenuItem::with_id(app, "tray-quit", "退出", true, None::<&str>) else {
        eprintln!("R Codex menu bar icon skipped: quit item");
        return;
    };
    let Ok(menu) = Menu::with_items(app, &[&show, &quit]) else {
        eprintln!("R Codex menu bar icon skipped: menu");
        return;
    };
    if let Err(error) = TrayIconBuilder::new()
        .icon(icon)
        .icon_as_template(true)
        .tooltip("R Codex")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "tray-show" => reveal_main_window(app),
            "tray-quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                reveal_main_window(tray.app_handle());
            }
        })
        .build(app)
    {
        eprintln!("R Codex menu bar icon skipped: {error}");
    }
}

pub fn run() {
    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            reveal_main_window(app);
        }));
    }

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
        builder = builder.plugin(tauri_plugin_process::init());
        builder = builder.plugin(tauri_plugin_opener::init());
    }

    #[cfg(desktop)]
    {
        builder = builder.on_window_event(|window, event| {
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    // Red close button: hide the control plane. It must not
                    // tear down a sidecar that an active Codex CLI still uses.
                    // On macOS the dock icon goes away; the menu bar icon stays.
                    // Dock Quit and Cmd+Q are handled by macos_quit, not here.
                    api.prevent_close();
                    park_main_window(window);
                }
                _ => {}
            }
        });
    }

    let app = builder
        .setup(|app| {
            install_menu_bar_icon(app);
            #[cfg(target_os = "macos")]
            macos_quit::install(app.handle().clone());
            #[cfg(debug_assertions)]
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title("R Codex Dev");
            }
            let database = Database::open(app)?;
            let api_key_vault =
                std::sync::Arc::new(secrets::DatabaseApiKeyVault::new(database.connection()));
            let diagnostics_store = DiagnosticsStore::from_database(database.clone());
            let app_data_dir = app
                .path()
                .app_data_dir()
                .map_err(|error| format!("无法定位应用数据目录：{error}"))?;
            // The archive is an opt-in, local-only store. It is separate from
            // Codex's own history and from the control database so a large
            // request body cannot block ordinary settings/diagnostics writes.
            app.manage(ConversationArchive::open(&app_data_dir)?);
            let managed_gateway_client =
                ManagedGatewayClient::new(app_data_dir.clone(), database.path().to_path_buf())?;
            // The gateway waits for instance bindings. A lost control token with
            // the data port still occupied is reported later as unavailable;
            // setup must still open the window so the restart button can run.
            if let Err(error) =
                tauri::async_runtime::block_on(managed_gateway_client.ensure_serving())
            {
                eprintln!("managed gateway startup deferred: {error}");
                let note = format!("managed gateway startup deferred: {error}\n");
                let _ = std::fs::write(app_data_dir.join("gateway-startup.log"), note);
            }
            app.manage(managed_gateway_client);
            app.manage(ProviderStore::from_database(
                database.clone(),
                api_key_vault,
            )?);
            app.manage(ManagedLaunchStore::from_database(app, database.clone())?);
            app.manage(CodexControlStore::from_database(app, database.clone())?);
            app.manage(OfficialAccountService::from_database(
                app,
                database.clone(),
            )?);
            app.manage(UsageStore::from_database(database.clone()));
            app.manage(BalanceStore::from_database(database.clone()));
            app.manage(diagnostics_store);
            app.manage(ProbeTicketStore::default());
            app.manage(SettingsStore::from_database(database));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            stop_gateway,
            cleanup_unavailable_gateway,
            restart_unavailable_gateway,
            gateway_status,
            list_api_key_providers,
            get_provider_usage_summary,
            conversation_archive_status,
            conversation_archive_set_policy,
            conversation_archive_list,
            conversation_archive_detail,
            conversation_archive_body,
            conversation_archive_clear,
            refresh_provider_upstream_balance,
            get_gateway_timeout_settings,
            update_gateway_timeout_settings,
            test_api_key_provider,
            probe_channel_draft,
            get_provider_connectivity_report,
            gateway_recent_diagnostics,
            clear_gateway_diagnostics,
            instance_request_diagnostics,
            recent_gateway_requests,
            instance_conversation_summaries,
            clear_instance_diagnostics,
            create_api_key_provider,
            select_default_api_key_provider,
            update_api_key_provider,
            rename_api_key_provider,
            delete_api_key_provider,
            get_codex_control_snapshot,
            create_launch_preset,
            create_managed_launch_preset,
            set_managed_login_provider,
            select_managed_launch_preset,
            list_importable_session_projects,
            import_sessions_from_default,
            get_managed_launch_preset_compositions,
            get_codex_special_model_roles,
            codex_installation_status,
            check_codex_updates,
            update_launch_preset,
            rename_launch_preset,
            delete_launch_preset,
            start_codex_account_authorization,
            poll_codex_account_authorization,
            cancel_codex_account_authorization,
            open_codex_account_authorization_prompt,
            refresh_codex_account_status,
            disconnect_codex_account,
            delete_codex_account,
            launch_codex,
            reconcile_codex_instance,
            stop_codex_instance,
            delete_codex_instance,
            delete_safe_codex_instances,
            open_codex_app,
            open_isolated_codex_home,
            sync_managed_preferences,
        ])
        .build(tauri::generate_context!())
        .expect("error while building R Codex");
    app.run(|app, event| match event {
        RunEvent::Ready => {
            #[cfg(target_os = "macos")]
            macos_quit::install(app.clone());
        }
        RunEvent::ExitRequested { api, code, .. } => {
            if let Err(error) = perform_exit_cleanup(app, code) {
                api.prevent_exit();
                let _ = app.emit(
                    "app-exit-blocked",
                    serde_json::json!({
                        "reason": "gateway-stop-failed",
                        "error": error,
                    }),
                );
            }
        }
        RunEvent::Exit => {
            // Dock Quit previously skipped ExitRequested and only reached
            // LoopDestroyed/Exit. Keep this as a last-chance cleanup.
            let _ = perform_exit_cleanup(app, None);
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::exit_preserves_instances;

    #[test]
    fn only_the_restart_exit_code_preserves_instances() {
        assert!(exit_preserves_instances(Some(tauri::RESTART_EXIT_CODE)));
        assert!(!exit_preserves_instances(Some(0)));
        assert!(!exit_preserves_instances(None));
    }
}
