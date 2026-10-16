//! 商店包激活脚本与 helper 合同。
//!
//! helper 必须是当前 codex-launcher，不能是 ChatGPT.exe 或控制面 UI。
//! PowerShell 只在 Windows 上执行；脚本文本可在 Mac 上测。

use std::path::Path;

use super::windows::RegisteredLaunch;
use crate::codex_installation::windows::normalized_windows_path_text;

pub const HELPER_ARG: &str = "--windows-package-helper";

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PackageHelperRequest {
    pub executable: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub result_path: String,
    #[serde(default)]
    pub instance_id: String,
    #[serde(default)]
    pub launch_nonce: String,
    #[serde(default)]
    pub status_file: String,
    #[serde(default)]
    pub codex_home: String,
}

pub fn escape_powershell_single_quoted(value: &str) -> String {
    value.replace('\'', "''")
}

fn windows_file_name(path: &Path) -> String {
    normalized_windows_path_text(path)
        .rsplit('\\')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn is_launcher_helper_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == "codex-launcher"
        || name == "codex-launcher.exe"
        || (name.starts_with("codex-launcher-") && name.ends_with(".exe"))
}

pub fn build_package_activation_script(
    package: &RegisteredLaunch,
    helper: &Path,
    payload_b64: &str,
) -> Result<String, String> {
    if payload_b64.len() > 30_000 {
        return Err("隔离启动参数超过商店激活限制。".to_string());
    }
    if !is_launcher_helper_name(&windows_file_name(helper)) {
        return Err("商店包激活 helper 必须是 codex-launcher。".to_string());
    }
    Ok(format!(
        "$ErrorActionPreference = 'Stop'\ntry {{\n  Invoke-CommandInDesktopPackage -PackageFamilyName '{family}' -AppId '{app_id}' -PreventBreakaway -Command '{helper}' -Args '{helper_arg} {payload}'\n}} catch {{\n  $exception = $_.Exception\n  while ($exception) {{\n    if ($exception.HResult -in @(-2147023728, -2147024894, -2147024893)) {{\n      Write-Output 'CODEX_PACKAGE_REGISTRATION_CHANGED'\n      exit 1\n    }}\n    $exception = $exception.InnerException\n  }}\n  [Console]::Error.WriteLine('Codex package activation failed')\n  exit 1\n}}",
        family = escape_powershell_single_quoted(&package.family_name),
        app_id = escape_powershell_single_quoted(&package.app_id),
        helper = escape_powershell_single_quoted(&helper.to_string_lossy()),
        helper_arg = HELPER_ARG,
        payload = payload_b64,
    ))
}

#[cfg(target_os = "windows")]
pub fn query_registered_launch(path: &Path) -> Result<Option<RegisteredLaunch>, String> {
    use crate::codex_installation::windows::is_windowsapps_launch_path;
    use crate::codex_installation::windows_store::package_family_from_path;
    if !is_windowsapps_launch_path(path) {
        return Ok(None);
    }
    let Some(install) = crate::codex_installation::windows::query_store_install_location() else {
        return Ok(None);
    };
    let probe = std::path::PathBuf::from(format!(r"{install}\app\Codex.exe"));
    let Some(family_name) =
        package_family_from_path(&probe).or_else(|| package_family_from_path(path))
    else {
        return Ok(None);
    };
    let executable = if path.is_file() {
        path.display().to_string()
    } else {
        probe.display().to_string()
    };
    Ok(Some(RegisteredLaunch {
        family_name,
        app_id: "App".to_string(),
        executable,
    }))
}

pub fn encode_helper_request(request: &PackageHelperRequest) -> Result<String, String> {
    let bytes =
        serde_json::to_vec(request).map_err(|error| format!("无法序列化商店激活参数：{error}"))?;
    use base64::Engine;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

#[cfg(target_os = "windows")]
pub fn run_helper(payload_b64: &str) -> Result<(), String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload_b64)
        .map_err(|_| "商店激活参数无法解码。".to_string())?;
    let request: PackageHelperRequest =
        serde_json::from_slice(&bytes).map_err(|_| "商店激活参数无法解析。".to_string())?;
    let mut command = std::process::Command::new(&request.executable);
    command.args(&request.args);
    for (key, value) in &request.env {
        command.env(key, value);
    }
    command.stdin(std::process::Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|error| format!("无法启动商店版 Codex：{error}"))?;
    if request.status_file.is_empty() {
        let status = child
            .wait()
            .map_err(|error| format!("无法等待商店版 Codex：{error}"))?;
        return if status.success() {
            Ok(())
        } else {
            Err(format!("商店版 Codex 以状态 {status} 退出。"))
        };
    }
    let identity = crate::launcher_protocol::process_start_identity(child.id())
        .unwrap_or_else(|_| "unknown".to_string());
    let running = crate::launcher_protocol::LauncherReceipt::running(
        request.instance_id,
        request.launch_nonce,
        child.id(),
        identity,
        request.executable,
        request.codex_home,
    );
    let status_path = std::path::PathBuf::from(&request.status_file);
    crate::launcher_protocol::write_receipt_atomically(&status_path, &running)?;
    let status = child
        .wait()
        .map_err(|error| format!("无法等待商店版 Codex：{error}"))?;
    let exited = crate::launcher_protocol::LauncherReceipt::exited_from(&running, status.code());
    crate::launcher_protocol::write_receipt_atomically(&status_path, &exited)?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("商店版 Codex 以状态 {status} 退出。"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn store_gui() -> &'static str {
        r"C:\Program Files\WindowsApps\OpenAI.ChatGPT-Desktop_1.2026.1.0_x64__8wekyb3d8bbwe\app\ChatGPT.exe"
    }

    fn launcher() -> &'static str {
        r"C:\Program Files\R Codex\codex-launcher.exe"
    }

    #[test]
    fn triple_named_launcher_can_activate_store_package() {
        let package = RegisteredLaunch {
            family_name: "OpenAI.Codex_2p2nqsd0c76g0".into(),
            app_id: "App".into(),
            executable: store_gui().into(),
        };
        let helper =
            Path::new(r"C:\Program Files\R Codex\codex-launcher-x86_64-pc-windows-msvc.exe");
        let script = build_package_activation_script(&package, helper, "cA==").unwrap();
        assert!(script.contains("OpenAI.Codex_2p2nqsd0c76g0"));
        assert!(script.contains("codex-launcher-x86_64-pc-windows-msvc.exe"));
    }

    #[test]
    fn activation_script_invokes_launcher_not_gui() {
        let package = RegisteredLaunch {
            family_name: "OpenAI.ChatGPT-Desktop_8wekyb3d8bbwe".into(),
            app_id: "App".into(),
            executable: store_gui().into(),
        };
        let helper = Path::new(launcher());
        let script = build_package_activation_script(&package, helper, "cGF5bG9hZA==").unwrap();
        assert!(script.contains("Invoke-CommandInDesktopPackage"));
        assert!(script.contains("PreventBreakaway"));
        assert!(script.contains("codex-launcher.exe"));
        let command_line = script
            .lines()
            .find(|line| line.contains("Invoke-CommandInDesktopPackage"))
            .unwrap();
        assert!(command_line.contains("codex-launcher.exe"));
        assert!(!command_line.contains("ChatGPT.exe"));
    }

    #[test]
    fn helper_must_be_launcher() {
        let package = RegisteredLaunch {
            family_name: "OpenAI.ChatGPT-Desktop_8wekyb3d8bbwe".into(),
            app_id: "App".into(),
            executable: store_gui().into(),
        };
        let error =
            build_package_activation_script(&package, Path::new(store_gui()), "cA==").unwrap_err();
        assert!(error.contains("codex-launcher"));
    }

    #[test]
    fn powershell_single_quotes_are_doubled() {
        assert_eq!(escape_powershell_single_quoted("a'b"), "a''b");
    }
}
