use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use r_codex_lib::codex_installation::{resolve_codex_app_executable, resolve_codex_executable};
use r_codex_lib::launcher_protocol::{
    append_startup_log, process_start_identity, write_receipt_atomically, LauncherReceipt,
};
use r_codex_lib::managed_app_launch::{
    apply_loopback_no_proxy, configure_managed_app_command, configure_unmanaged_app_command,
};
use zeroize::Zeroize;

struct ReceiptTarget {
    instance_id: String,
    launch_nonce: String,
    status_file: PathBuf,
    codex_home: String,
    codex_executable: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("R Codex launcher: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut target = "cli".to_string();
    let mut credential_kind = "api_key".to_string();
    let mut secret_ref = None;
    let mut instance_key_ref = None;
    let mut profile = None;
    let mut model = None;
    let mut working_directory = None;
    let mut codex_home = None;
    let mut electron_user_data_dir = None;
    let mut sqlite_home = None;
    let mut instance_id = None;
    let mut launch_nonce = None;
    let mut status_file = None;
    let mut log_file = None;
    let mut secret_file = None;
    let mut arguments = env::args().skip(1).peekable();
    #[cfg(target_os = "windows")]
    if arguments.peek().map(String::as_str)
        == Some(r_codex_lib::managed_app_launch::windows_package::HELPER_ARG)
    {
        arguments.next();
        let payload = arguments
            .next()
            .ok_or_else(|| "商店激活参数缺失。".to_string())?;
        return r_codex_lib::managed_app_launch::windows_package::run_helper(&payload);
    }
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--target" => {
                target = arguments
                    .next()
                    .ok_or_else(|| "缺少 --target 值。".to_string())?
            }
            "--credential-kind" => {
                credential_kind = arguments
                    .next()
                    .ok_or_else(|| "缺少 --credential-kind 值。".to_string())?
            }
            "--secret-ref" => secret_ref = arguments.next(),
            "--instance-key-ref" => instance_key_ref = arguments.next(),
            "--profile" => profile = arguments.next(),
            "--model" => model = arguments.next(),
            "--working-directory" => working_directory = arguments.next(),
            "--codex-home" => codex_home = arguments.next(),
            "--electron-user-data-dir" => electron_user_data_dir = arguments.next(),
            "--sqlite-home" => sqlite_home = arguments.next(),
            "--instance-id" => instance_id = arguments.next(),
            "--launch-nonce" => launch_nonce = arguments.next(),
            "--status-file" => status_file = arguments.next(),
            "--log-file" => log_file = arguments.next(),
            "--secret-file" => secret_file = arguments.next(),
            _ => return Err("Codex 启动器收到了未知参数。".to_string()),
        }
    }
    let log_file = log_file.map(PathBuf::from);
    log_event(
        log_file.as_deref(),
        &format!("launcher_started target={target} credential_kind={credential_kind}"),
    );
    let codex = resolve_codex_executable().map_err(|error| {
        log_error(log_file.as_deref(), "resolve_codex_failed", &error);
        error
    })?;
    log_event(
        log_file.as_deref(),
        &format!("codex_resolved executable={}", codex.display()),
    );
    let working_directory =
        resolve_working_directory(working_directory.as_deref()).map_err(|error| {
            log_error(
                log_file.as_deref(),
                "resolve_working_directory_failed",
                &error,
            );
            error
        })?;
    log_event(log_file.as_deref(), "working_directory_resolved");
    match target.as_str() {
        "cli" => {
            if codex_home.is_none() {
                return Err("R Codex CLI 启动缺少独立 --codex-home。".to_string());
            }
            let codex = codex.canonicalize().unwrap_or(codex);
            let mut command = Command::new(&codex);
            command
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .env_remove("OPENAI_API_KEY")
                .env_remove("CODEX_API_KEY")
                .env_remove("CODEX_ACCESS_TOKEN")
                .env_remove("RCODEX_INSTANCE_KEY")
                .env_remove("THREER_INSTANCE_KEY");
            let mut sensitive_value = if instance_key_ref.is_some() {
                if !r_codex_lib::CredentialKind::wire_is_managed_route_set(&credential_kind)
                    || secret_ref.is_some()
                {
                    return Err("受管理 RouteSet 启动参数冲突。".to_string());
                }
                if codex_home.is_none() {
                    return Err("受管理 RouteSet 启动缺少 --codex-home。".to_string());
                }
                let key = read_instance_key_handoff(secret_file.as_deref(), log_file.as_deref())?;
                log_event(log_file.as_deref(), "instance_key_loaded");
                let profile = profile
                    .as_deref()
                    .ok_or_else(|| "受管理 RouteSet 启动缺少 --profile。".to_string())?;
                let model = model
                    .as_deref()
                    .ok_or_else(|| "受管理 RouteSet 启动缺少 --model。".to_string())?;
                validate_model_id(model)?;
                validate_profile_name(profile)?;
                configure_managed_cli_command(&mut command, profile, model, &key);
                log_event(
                    log_file.as_deref(),
                    &format!(
                        "managed_route_contract provider=rcodex model={model} profile={profile}"
                    ),
                );
                Some(key)
            } else {
                match credential_kind.as_str() {
                    "api_key" => {
                        return Err(
                            "受管理启动必须经过 Gateway，不再支持直连 API Key。".to_string()
                        );
                    }
                    "official" => None,
                    _ => return Err(format!("未知凭据类型：{credential_kind}")),
                }
            };
            if let Some(home) = codex_home.as_deref() {
                command
                    .env("CODEX_HOME", home)
                    .env("CODEX_SQLITE_HOME", home);
            }
            if let Some(directory) = working_directory.as_deref() {
                command.arg("--cd").arg(directory);
            }
            if instance_key_ref.is_none() {
                if let Some(profile) = profile.as_deref() {
                    let model = model
                        .as_deref()
                        .ok_or_else(|| "受管理启动缺少 --model。".to_string())?;
                    validate_profile_name(profile)?;
                    validate_model_id(model)?;
                    command
                        .arg("--profile")
                        .arg(profile)
                        .arg("--model")
                        .arg(model);
                } else if model.is_some() {
                    return Err("启动器收到未绑定 Profile 的 --model。".to_string());
                }
            }
            if let Some(value) = sensitive_value.as_mut() {
                value.zeroize();
            }
            let receipt_target = match (instance_id, launch_nonce, status_file, codex_home) {
                (Some(instance_id), Some(launch_nonce), Some(status_file), Some(codex_home)) => {
                    Some(ReceiptTarget {
                        instance_id,
                        launch_nonce,
                        status_file: PathBuf::from(status_file),
                        codex_home,
                        codex_executable: codex.display().to_string(),
                    })
                }
                (None, None, None, _) => None,
                _ => return Err("受管理 CLI 启动参数不完整。".to_string()),
            };
            let status = supervise_child(&mut command, receipt_target, log_file.as_deref())?;
            if status.success() {
                log_event(log_file.as_deref(), "launcher_finished success=true");
                Ok(())
            } else {
                log_event(log_file.as_deref(), "launcher_finished success=false");
                Err(format!("Codex 以状态 {status} 退出。"))
            }
        }
        // The IPC/instance contract uses snake_case (`managed_app`); accept
        // the historical dashed spelling as a compatibility alias for old
        // generated launch scripts.
        target
            if r_codex_lib::launcher_protocol::LaunchTarget::parse_alias(target)
                .is_some_and(|item| item.is_managed_app()) =>
        {
            if !r_codex_lib::CredentialKind::wire_is_managed_route_set(&credential_kind)
                || secret_ref.is_some()
                || instance_key_ref.is_none()
                || profile.is_none()
                || model.is_none()
                || codex_home.is_none()
                || electron_user_data_dir.is_none()
                || sqlite_home.is_none()
            {
                return Err("受管理 Codex App 启动参数不完整。".to_string());
            }
            let instance_id = instance_id
                .ok_or_else(|| "受管理 Codex App 启动缺少 --instance-id。".to_string())?;
            let launch_nonce = launch_nonce
                .ok_or_else(|| "受管理 Codex App 启动缺少 --launch-nonce。".to_string())?;
            let status_file = status_file
                .ok_or_else(|| "受管理 Codex App 启动缺少 --status-file。".to_string())?;
            let codex_home = PathBuf::from(
                codex_home.ok_or_else(|| "受管理 Codex App 启动缺少 --codex-home。".to_string())?,
            );
            let electron_user_data_dir =
                PathBuf::from(electron_user_data_dir.ok_or_else(|| {
                    "受管理 Codex App 启动缺少 --electron-user-data-dir。".to_string()
                })?);
            let sqlite_home = PathBuf::from(
                sqlite_home
                    .ok_or_else(|| "受管理 Codex App 启动缺少 --sqlite-home。".to_string())?,
            );
            if !codex_home.is_dir() || !electron_user_data_dir.is_dir() || !sqlite_home.is_dir() {
                return Err("受管理 Codex App 隔离目录不存在。".to_string());
            }
            let profile = profile
                .as_deref()
                .ok_or_else(|| "受管理 Codex App 启动缺少 --profile。".to_string())?;
            let model = model
                .as_deref()
                .ok_or_else(|| "受管理 Codex App 启动缺少 --model。".to_string())?;
            validate_profile_name(profile)?;
            validate_model_id(model)?;
            let mut instance_key =
                read_instance_key_handoff(secret_file.as_deref(), log_file.as_deref())?;
            let app_executable = resolve_codex_app_executable(&codex)?;
            #[cfg(target_os = "windows")]
            {
                use r_codex_lib::managed_app_launch::windows::{
                    managed_launch_route, ManagedLaunchRoute,
                };
                use r_codex_lib::managed_app_launch::windows_package::query_registered_launch;
                let registered = query_registered_launch(&app_executable)?;
                match managed_launch_route(&app_executable, registered.as_ref())? {
                    ManagedLaunchRoute::DirectExe => {}
                    ManagedLaunchRoute::PackageIdentity(package) => {
                        let result = activate_store_package(
                            &package,
                            &app_executable,
                            &codex_home,
                            &electron_user_data_dir,
                            &sqlite_home,
                            &instance_key,
                            working_directory.as_deref(),
                            &instance_id,
                            &launch_nonce,
                            &status_file,
                            log_file.as_deref(),
                        );
                        instance_key.zeroize();
                        return result;
                    }
                }
            }
            let mut command = Command::new(&app_executable);
            command
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .env_remove("OPENAI_API_KEY")
                .env_remove("CODEX_API_KEY")
                .env_remove("CODEX_ACCESS_TOKEN");
            configure_managed_app_command(
                &mut command,
                &codex_home,
                &electron_user_data_dir,
                &sqlite_home,
                &instance_key,
            );
            if let Some(directory) = working_directory.as_ref() {
                command.current_dir(&directory).arg(directory);
            }
            instance_key.zeroize();
            let receipt_target = ReceiptTarget {
                instance_id,
                launch_nonce,
                status_file: PathBuf::from(status_file),
                codex_home: codex_home.display().to_string(),
                codex_executable: app_executable.display().to_string(),
            };
            let status = supervise_child(&mut command, Some(receipt_target), log_file.as_deref())?;
            if status.success() {
                log_event(log_file.as_deref(), "managed_app_finished success=true");
                Ok(())
            } else {
                log_event(log_file.as_deref(), "managed_app_finished success=false");
                Err(format!("Codex App 以状态 {status} 退出。"))
            }
        }
        "app" => {
            if secret_ref.is_some()
                || instance_key_ref.is_some()
                || profile.is_some()
                || model.is_some()
                || codex_home.is_some()
                || electron_user_data_dir.is_some()
                || instance_id.is_some()
                || launch_nonce.is_some()
                || status_file.is_some()
                || secret_file.is_some()
            {
                return Err("Codex App 默认启动不接受受管理配置。".to_string());
            }
            let mut command = Command::new(codex);
            let directory = match working_directory.as_deref() {
                Some(directory) => directory.to_path_buf(),
                None => default_working_directory()?,
            };
            configure_unmanaged_app_command(&mut command, &directory);
            let status = command
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status()
                .map_err(|error| format!("无法启动 Codex App：{error}"))?;
            if status.success() {
                Ok(())
            } else {
                Err(format!("Codex App 以状态 {status} 退出。"))
            }
        }
        _ => return Err(format!("未知 Codex 启动目标：{target}")),
    }
}

fn configure_managed_cli_command(
    command: &mut Command,
    profile: &str,
    model: &str,
    instance_key: &str,
) {
    command
        .arg("--profile")
        .arg(profile)
        // The profile is the persisted contract; these explicit overrides are
        // the final launch boundary so a stale/default config cannot silently
        // route the request to the official provider or another model.
        .arg("-c")
        .arg("model_provider=\"rcodex\"")
        .arg("--model")
        .arg(model)
        .arg("--ask-for-approval")
        .arg("on-request")
        .env("RCODEX_INSTANCE_KEY", instance_key)
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_API_KEY")
        .env_remove("CODEX_ACCESS_TOKEN");
    apply_loopback_no_proxy(command);
}

fn validate_model_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
    {
        return Err("受管理模型 ID 无效。".to_string());
    }
    Ok(())
}

fn validate_profile_name(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 96
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err("受管理 Profile 名称无效。".to_string());
    }
    Ok(())
}

fn supervise_child(
    command: &mut Command,
    receipt_target: Option<ReceiptTarget>,
    log_file: Option<&Path>,
) -> Result<std::process::ExitStatus, String> {
    log_event(log_file, "codex_spawn_requested");
    let mut child = command.spawn().map_err(|error| {
        let message = format!("无法启动 Codex：{error}");
        log_error(log_file, "codex_spawn_failed", &message);
        message
    })?;
    log_event(log_file, &format!("codex_spawned pid={}", child.id()));
    let running_receipt = if let Some(target) = receipt_target {
        let identity = match process_start_identity(child.id()) {
            Ok(identity) => identity,
            Err(error) => {
                log_error(log_file, "process_identity_failed", &error);
                terminate_and_wait(&mut child);
                return Err(error);
            }
        };
        let receipt = LauncherReceipt::running(
            target.instance_id,
            target.launch_nonce,
            child.id(),
            identity,
            target.codex_executable,
            target.codex_home,
        );
        if let Err(error) = write_receipt_atomically(&target.status_file, &receipt) {
            log_error(log_file, "running_receipt_write_failed", &error);
            terminate_and_wait(&mut child);
            return Err(error);
        }
        log_event(log_file, "running_receipt_written");
        Some((target.status_file, receipt))
    } else {
        None
    };

    let status = child.wait().map_err(|error| {
        let message = format!("无法等待 Codex 退出：{error}");
        log_error(log_file, "codex_wait_failed", &message);
        message
    })?;
    log_event(
        log_file,
        &format!(
            "codex_exited success={} code={:?}",
            status.success(),
            status.code()
        ),
    );
    if let Some((status_file, running)) = running_receipt {
        let exited = LauncherReceipt::exited_from(&running, status.code());
        if let Err(error) = write_receipt_atomically(&status_file, &exited) {
            log_error(log_file, "exited_receipt_write_failed", &error);
            return Err(error);
        }
        log_event(log_file, "exited_receipt_written");
    }
    Ok(status)
}

fn log_event(path: Option<&Path>, event: &str) {
    if let Some(path) = path {
        let _ = append_startup_log(path, event);
    }
}

fn log_error(path: Option<&Path>, code: &str, error: &str) {
    let safe_error = error.replace(['\r', '\n'], " ");
    log_event(path, &format!("{code} error={safe_error}"));
}

fn read_instance_key_handoff(
    secret_file: Option<&str>,
    log_file: Option<&Path>,
) -> Result<String, String> {
    let Some(path) = secret_file else {
        let message = "受管理启动缺少一次性实例密钥管道。".to_string();
        log_error(log_file, "instance_key_handoff_missing", &message);
        return Err(message);
    };
    log_event(log_file, "instance_key_handoff_read_requested");
    read_secret_file(path).map_err(|error| {
        log_error(log_file, "instance_key_handoff_read_failed", &error);
        error
    })
}

fn read_secret_file(path: &str) -> Result<String, String> {
    let path = Path::new(path);
    // The launcher owns unlinking after a successful read. Removing the FIFO
    // on an open/read error would hide the original failure and race the
    // parent-side handoff retry/timeout cleanup.
    let bytes = fs::read(path).map_err(|error| format!("无法读取一次性启动凭据：{error}"))?;
    let _ = fs::remove_file(path);
    String::from_utf8(bytes).map_err(|_| "一次性启动凭据不是有效文本。".to_string())
}

fn terminate_and_wait(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn resolve_working_directory(value: Option<&str>) -> Result<Option<PathBuf>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let expanded = if value == "~" {
        r_codex_lib::user_home().ok_or_else(|| "无法定位用户主目录。".to_string())?
    } else if let Some(relative) = value.strip_prefix("~/") {
        r_codex_lib::user_home()
            .ok_or_else(|| "无法定位用户主目录。".to_string())?
            .join(relative)
    } else {
        PathBuf::from(value)
    };
    let directory = expanded
        .canonicalize()
        .map_err(|error| format!("Codex 工作目录不可用：{}（{error}）", expanded.display()))?;
    if !directory.is_dir() {
        return Err(format!("Codex 工作目录不是文件夹：{}", directory.display()));
    }
    Ok(Some(directory))
}

fn default_working_directory() -> Result<PathBuf, String> {
    let directory = r_codex_lib::user_home()
        .ok_or_else(|| "无法定位用户主目录作为 Codex 工作目录。".to_string())?
        .canonicalize()
        .map_err(|error| format!("无法定位用户主目录作为 Codex 工作目录：{error}"))?;
    if !directory.is_dir() {
        return Err(format!("用户主目录不是文件夹：{}", directory.display()));
    }
    Ok(directory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use r_codex_lib::launcher_protocol::{read_receipt, LauncherState};

    #[test]
    fn empty_working_directory_keeps_cli_default_and_resolves_app_home() {
        assert!(resolve_working_directory(None).unwrap().is_none());
        assert!(resolve_working_directory(Some("   ")).unwrap().is_none());
        assert!(default_working_directory().unwrap().is_dir());
    }

    #[test]
    fn app_target_removes_managed_and_inherited_auth_environment() {
        let directory = default_working_directory().unwrap();
        let mut command = Command::new("/bin/true");
        configure_unmanaged_app_command(&mut command, &directory);

        let removals = command
            .get_envs()
            .filter_map(|(name, value)| value.is_none().then(|| name.to_string_lossy().to_string()))
            .collect::<std::collections::HashSet<_>>();
        for name in [
            "CODEX_HOME",
            "CODEX_SQLITE_HOME",
            "CODEX_ACCESS_TOKEN",
            "CODEX_API_KEY",
            "OPENAI_API_KEY",
        ] {
            assert!(removals.contains(name));
        }
    }

    #[test]
    fn managed_cli_uses_only_instance_header_environment_and_named_profile() {
        let mut command = Command::new("/bin/true");
        command
            .env("OPENAI_API_KEY", "provider-secret-sentinel")
            .env("CODEX_API_KEY", "codex-key-sentinel")
            .env("CODEX_ACCESS_TOKEN", "official-token-sentinel");
        configure_managed_cli_command(
            &mut command,
            "launch-revision-a",
            "grok-4.6",
            "instance-sentinel",
        );

        let arguments = command
            .get_args()
            .map(|value| value.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            arguments,
            vec![
                "--profile",
                "launch-revision-a",
                "-c",
                "model_provider=\"rcodex\"",
                "--model",
                "grok-4.6",
                "--ask-for-approval",
                "on-request"
            ]
        );
        let environment = command
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().to_string(),
                    value.map(|value| value.to_string_lossy().to_string()),
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(
            environment.get("RCODEX_INSTANCE_KEY"),
            Some(&Some("instance-sentinel".to_string()))
        );
        for name in ["OPENAI_API_KEY", "CODEX_API_KEY", "CODEX_ACCESS_TOKEN"] {
            assert_eq!(environment.get(name), Some(&None));
        }
        let upper = environment
            .get("NO_PROXY")
            .and_then(|value| value.as_deref())
            .unwrap();
        let lower = environment
            .get("no_proxy")
            .and_then(|value| value.as_deref())
            .unwrap();
        assert_eq!(upper, lower);
        for entry in ["127.0.0.1", "localhost", "::1"] {
            assert!(upper.split(',').any(|current| current == entry));
        }
    }

    #[test]
    fn resolves_desktop_app_next_to_bundled_codex_cli() {
        let codex = PathBuf::from(
            "/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex",
        );
        if codex.is_file() {
            let app = resolve_codex_app_executable(&codex).unwrap();
            assert_eq!(
                app,
                PathBuf::from("/Applications/ChatGPT.app/Contents/MacOS/ChatGPT")
            );
        }
    }

    #[test]
    fn managed_profile_name_cannot_escape_codex_home() {
        assert!(validate_profile_name("launch-a_1").is_ok());
        for value in ["../escape", "with/slash", "", "has space"] {
            assert!(validate_profile_name(value).is_err());
        }
    }

    #[test]
    fn managed_model_id_rejects_shell_or_path_tokens() {
        assert!(validate_model_id("grok-4.6").is_ok());
        assert!(validate_model_id("grok/other").is_err());
        assert!(validate_model_id("grok\" --oss").is_err());
    }

    #[test]
    fn working_directory_must_be_an_existing_directory() {
        let directory = env::temp_dir().join(format!("rcodex-launcher-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let resolved = resolve_working_directory(directory.to_str())
            .unwrap()
            .unwrap();
        assert!(resolved.is_dir());

        let file = directory.join("not-a-directory");
        std::fs::write(&file, "test").unwrap();
        let error = resolve_working_directory(file.to_str()).unwrap_err();
        assert!(error.contains("不是文件夹"));

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn supervisor_records_real_pid_and_nonzero_exit() {
        let directory = env::temp_dir().join(format!("3r-supervisor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let status_file = directory.join("status.json");
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg("exit 7");
        let status = supervise_child(
            &mut command,
            Some(ReceiptTarget {
                instance_id: "instance-a".to_string(),
                launch_nonce: "nonce-a".to_string(),
                status_file: status_file.clone(),
                codex_home: directory.display().to_string(),
                codex_executable: "/bin/sh".to_string(),
            }),
            None,
        )
        .unwrap();
        assert_eq!(status.code(), Some(7));
        let receipt = read_receipt(&status_file).unwrap();
        assert_eq!(receipt.state, LauncherState::Exited);
        assert_eq!(receipt.exit_code, Some(7));
        assert!(receipt.codex_pid > 0);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn supervisor_terminates_child_when_running_receipt_cannot_be_written() {
        let directory = env::temp_dir().join(format!("3r-supervisor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let invalid_parent = directory.join("not-a-directory");
        std::fs::write(&invalid_parent, "x").unwrap();
        let pid_file = directory.join("pid");
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(format!("echo $$ > '{}'; sleep 30", pid_file.display()));
        let error = supervise_child(
            &mut command,
            Some(ReceiptTarget {
                instance_id: "instance-a".to_string(),
                launch_nonce: "nonce-a".to_string(),
                status_file: invalid_parent.join("status.json"),
                codex_home: directory.display().to_string(),
                codex_executable: "/bin/sh".to_string(),
            }),
            None,
        )
        .unwrap_err();
        assert!(error.contains("启动回执"));
        if let Ok(pid) = std::fs::read_to_string(&pid_file) {
            let pid = pid.trim().parse::<u32>().unwrap();
            assert!(process_start_identity(pid).is_err());
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn instance_key_handoff_requires_secret_pipe() {
        let error = read_instance_key_handoff(None, None).unwrap_err();
        assert!(error.contains("管道"));
    }

    #[test]
    fn one_time_secret_file_is_removed_after_read() {
        let path = std::env::temp_dir().join(format!("3r-launch-secret-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"secret-sentinel").unwrap();
        let value = read_secret_file(path.to_str().unwrap()).unwrap();
        assert_eq!(value, "secret-sentinel");
        assert!(!path.exists());
    }
}

#[cfg(target_os = "windows")]
fn activate_store_package(
    package: &r_codex_lib::managed_app_launch::windows::RegisteredLaunch,
    app_executable: &Path,
    codex_home: &Path,
    electron_user_data_dir: &Path,
    sqlite_home: &Path,
    instance_key: &str,
    working_directory: Option<&str>,
    instance_id: &str,
    launch_nonce: &str,
    status_file: &str,
    log_file: Option<&Path>,
) -> Result<(), String> {
    use r_codex_lib::managed_app_launch::merged_loopback_no_proxy;
    use r_codex_lib::managed_app_launch::windows_package::{
        build_package_activation_script, encode_helper_request, PackageHelperRequest,
    };
    let no_proxy = merged_loopback_no_proxy(
        env::var("NO_PROXY").ok().as_deref(),
        env::var("no_proxy").ok().as_deref(),
    );
    let mut args = vec![format!(
        "--user-data-dir={}",
        electron_user_data_dir.display()
    )];
    if let Some(directory) = working_directory {
        args.push(directory.to_string());
    }
    let request = PackageHelperRequest {
        executable: app_executable.display().to_string(),
        args,
        env: vec![
            ("CODEX_HOME".to_string(), codex_home.display().to_string()),
            (
                "CODEX_SQLITE_HOME".to_string(),
                sqlite_home.display().to_string(),
            ),
            (
                "CODEX_ELECTRON_USER_DATA_PATH".to_string(),
                electron_user_data_dir.display().to_string(),
            ),
            ("RCODEX_INSTANCE_KEY".to_string(), instance_key.to_string()),
            ("NO_PROXY".to_string(), no_proxy.clone()),
            ("no_proxy".to_string(), no_proxy),
        ],
        result_path: String::new(),
        instance_id: instance_id.to_string(),
        launch_nonce: launch_nonce.to_string(),
        status_file: status_file.to_string(),
        codex_home: codex_home.display().to_string(),
    };
    let payload = encode_helper_request(&request)?;
    let helper = env::current_exe().map_err(|error| format!("无法定位启动器：{error}"))?;
    let script = build_package_activation_script(package, &helper, &payload)?;
    let script_path = Path::new(status_file).with_file_name("store-activation.ps1");
    fs::write(&script_path, script).map_err(|error| format!("无法写入商店激活脚本：{error}"))?;
    log_event(
        log_file,
        &format!("store_activation_started family={}", package.family_name),
    );
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script_path)
        .output()
        .map_err(|error| format!("无法运行商店激活：{error}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    log_event(
        log_file,
        &format!(
            "store_activation_finished success={} stdout={} stderr={}",
            output.status.success(),
            stdout.chars().take(180).collect::<String>(),
            stderr.chars().take(180).collect::<String>()
        ),
    );
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "商店包激活失败：{}",
            stderr.chars().take(300).collect::<String>()
        ))
    }
}
