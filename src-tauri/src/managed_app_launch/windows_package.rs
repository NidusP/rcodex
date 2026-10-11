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

pub fn build_package_activation_script(
    package: &RegisteredLaunch,
    helper: &Path,
    payload_b64: &str,
) -> Result<String, String> {
    if payload_b64.len() > 30_000 {
        return Err("隔离启动参数超过商店激活限制。".to_string());
    }
    let helper_name = windows_file_name(helper);
    if helper_name != "codex-launcher.exe" && helper_name != "codex-launcher" {
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
pub fn query_registered_launch(_path: &Path) -> Result<Option<RegisteredLaunch>, String> {
    // Registration query is desktop-only. Until a Windows machine records a
    // real Get-AppxPackage result, fail closed for store paths by returning None.
    Ok(None)
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
