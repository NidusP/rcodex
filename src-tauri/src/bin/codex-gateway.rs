#![cfg_attr(windows, windows_subsystem = "windows")]

use std::{env, path::PathBuf};

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    match required_args(&args) {
        Ok((database_path, ready_path, control_token, data_port)) => {
            if let Err(error) = r_codex_lib::managed_gateway::run_sidecar(
                database_path,
                ready_path,
                control_token,
                data_port,
            )
            .await
            {
                eprintln!("codex-gateway: {error}");
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("codex-gateway: {error}");
            std::process::exit(2);
        }
    }
}

fn required_args(args: &[String]) -> Result<(PathBuf, PathBuf, String, u16), String> {
    let value = |name: &str| {
        args.windows(2)
            .find(|pair| pair[0] == name)
            .map(|pair| pair[1].clone())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("missing {name}"))
    };
    let data_port = args
        .windows(2)
        .find(|pair| pair[0] == "--data-port")
        .map(|pair| pair[1].parse::<u16>())
        .transpose()
        .map_err(|_| "invalid --data-port".to_string())?
        .unwrap_or(r_codex_lib::managed_gateway::GATEWAY_PORT);
    Ok((
        PathBuf::from(value("--database-path")?),
        PathBuf::from(value("--ready-file")?),
        value("--control-token")?,
        data_port,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_required_args_without_echoing_values() {
        let args = vec![
            "codex-gateway".to_string(),
            "--database-path".to_string(),
            "/tmp/test.sqlite3".to_string(),
            "--ready-file".to_string(),
            "/tmp/ready.json".to_string(),
            "--control-token".to_string(),
            "3rctl_test".to_string(),
        ];
        let parsed = required_args(&args).unwrap();
        assert_eq!(parsed.0, PathBuf::from("/tmp/test.sqlite3"));
        assert_eq!(parsed.1, PathBuf::from("/tmp/ready.json"));
        assert_eq!(parsed.2, "3rctl_test");
        assert_eq!(parsed.3, r_codex_lib::managed_gateway::GATEWAY_PORT);
    }
}
