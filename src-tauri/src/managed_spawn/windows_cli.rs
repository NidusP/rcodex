//! Windows 隔离 CLI 的终端命令行。纯组装，不 spawn。
//!
//! 控制面在 Windows 上按这份 spec spawn；macOS 单测只检查 argv。

use std::path::Path;

use crate::codex_installation::windows::normalized_windows_path_text;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowsCliTerminalKind {
    WindowsTerminal,
    CmdStart,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowsCliTerminalSpec {
    pub kind: WindowsCliTerminalKind,
    pub program: String,
    pub args: Vec<String>,
}

/// 有 Windows Terminal 时用 wt -d <dir> -- launcher argv；否则 cmd /c start。
pub fn windows_cli_terminal_spec(
    helper: &Path,
    launcher_args: &[String],
    working_directory: Option<&str>,
    wt_path: Option<&Path>,
) -> WindowsCliTerminalSpec {
    let helper = normalized_windows_path_text(helper);
    let workdir = working_directory
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| normalized_windows_path_text(Path::new(value)));
    if let Some(wt) = wt_path {
        let mut args = Vec::new();
        if let Some(dir) = workdir.as_deref() {
            args.push("-d".to_string());
            args.push(dir.to_string());
        }
        args.push("--".to_string());
        args.push(helper);
        args.extend(launcher_args.iter().cloned());
        WindowsCliTerminalSpec {
            kind: WindowsCliTerminalKind::WindowsTerminal,
            program: normalized_windows_path_text(wt),
            args,
        }
    } else {
        let mut args = vec![
            "/c".to_string(),
            "start".to_string(),
            "R Codex CLI".to_string(),
        ];
        if let Some(dir) = workdir.as_deref() {
            args.push("/D".to_string());
            args.push(dir.to_string());
        }
        args.push(helper);
        args.extend(launcher_args.iter().cloned());
        WindowsCliTerminalSpec {
            kind: WindowsCliTerminalKind::CmdStart,
            program: "cmd.exe".to_string(),
            args,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn launcher_args() -> Vec<String> {
        vec![
            "--target".into(),
            "cli".into(),
            "--credential-kind".into(),
            "managed_route_set".into(),
        ]
    }

    #[test]
    fn windows_terminal_wraps_launcher_after_double_dash() {
        let spec = windows_cli_terminal_spec(
            Path::new(r"C:\Program Files\R Codex\codex-launcher.exe"),
            &launcher_args(),
            Some(r"C:\Users\a\project"),
            Some(Path::new(
                r"C:\Users\a\AppData\Local\Microsoft\WindowsApps\wt.exe",
            )),
        );
        assert_eq!(spec.kind, WindowsCliTerminalKind::WindowsTerminal);
        assert_eq!(
            spec.program,
            r"C:\Users\a\AppData\Local\Microsoft\WindowsApps\wt.exe"
        );
        assert_eq!(
            spec.args,
            vec![
                "-d".to_string(),
                r"C:\Users\a\project".to_string(),
                "--".to_string(),
                r"C:\Program Files\R Codex\codex-launcher.exe".to_string(),
                "--target".to_string(),
                "cli".to_string(),
                "--credential-kind".to_string(),
                "managed_route_set".to_string(),
            ]
        );
    }

    #[test]
    fn cmd_start_uses_title_and_optional_directory() {
        let spec = windows_cli_terminal_spec(
            Path::new(r"C:\Program Files\R Codex\codex-launcher.exe"),
            &launcher_args(),
            Some(r"C:\Users\a\project"),
            None,
        );
        assert_eq!(spec.kind, WindowsCliTerminalKind::CmdStart);
        assert_eq!(spec.program, "cmd.exe");
        assert_eq!(
            spec.args,
            vec![
                "/c".to_string(),
                "start".to_string(),
                "R Codex CLI".to_string(),
                "/D".to_string(),
                r"C:\Users\a\project".to_string(),
                r"C:\Program Files\R Codex\codex-launcher.exe".to_string(),
                "--target".to_string(),
                "cli".to_string(),
                "--credential-kind".to_string(),
                "managed_route_set".to_string(),
            ]
        );
    }

    #[test]
    fn missing_working_directory_omits_directory_flags() {
        let wt = windows_cli_terminal_spec(
            Path::new(r"C:\codex-launcher.exe"),
            &["--target".into(), "cli".into()],
            None,
            Some(Path::new(r"C:\wt.exe")),
        );
        assert_eq!(wt.args[0], "--");
        assert!(!wt.args.iter().any(|arg| arg == "-d"));
        let cmd = windows_cli_terminal_spec(
            Path::new(r"C:\codex-launcher.exe"),
            &["--target".into(), "cli".into()],
            Some("   "),
            None,
        );
        assert!(!cmd.args.iter().any(|arg| arg == "/D"));
        assert_eq!(cmd.args[2], "R Codex CLI");
    }
}
