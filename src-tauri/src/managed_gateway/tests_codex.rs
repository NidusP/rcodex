//! 安装的 Codex 经 Gateway 调用第三方渠道。

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex as StdMutex,
};

use axum::{http::HeaderValue, routing::post};
use tower::ServiceExt;

use super::test_support::*;
use super::*;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json, Router,
};
use serde_json::{json, Value};
use uuid::Uuid;

use super::errors::*;
use super::forward::*;
use super::helpers::*;
use super::http::*;
use super::lifecycle::*;
use super::protocol::*;
use super::runtime::*;
use super::runtime_types;
use super::runtime_types::*;
use super::sse::SseDecoder;
use super::stream::*;
use crate::managed_codex_config::{INSTANCE_GATEWAY_HEADER, LEGACY_INSTANCE_GATEWAY_HEADER};
use crate::routing::{
    CompactMode, CompactionSupport, GatewayTimeoutSnapshot, ModelRoute, RouteSource, WireApi,
};
use crate::{
    database::Database,
    managed_codex_config::{
        build_version_matched_catalog, materialize_managed_codex_config, read_bundled_catalog,
        INSTANCE_GATEWAY_ENV,
    },
    providers::{CreateProviderInput, ProviderStore, TransportMode},
    routing::{
        CapabilitySet, GatewayServiceRevision, RouteSetProviderRevision, RouteSetRevision,
        RouteSetState,
    },
    secrets::{instance_gateway_key_hash, ApiKey, ApiKeyVault, InstanceGatewayKey},
};
use std::collections::BTreeMap;
use std::sync::{atomic::AtomicUsize, Arc};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use url::Url;

#[tokio::test]
async fn installed_codex_can_call_a_grok_route_through_base_url_and_api_key_when_requested() {
    let Ok(codex_executable) = std::env::var("CODEX_REAL_GROK_CONTRACT_BIN") else {
        return;
    };
    let codex_executable = PathBuf::from(codex_executable);
    let observed = Arc::new(StdMutex::new(None));
    let listener = TcpListener::bind((LOOPBACK_HOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/v1/responses", post(mock_grok_responses_stream))
        .with_state(observed.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let vault = Arc::new(MemoryVault::default());
    let store = ProviderStore::from_database(Database::in_memory(), vault).unwrap();
    let provider = store
        .create(CreateProviderInput {
            name: "Grok Relay".to_string(),
            base_url: format!("http://127.0.0.1:{port}/v1"),
            api_key: "grok-provider-key-sentinel".to_string(),
            transport_mode: TransportMode::GatewayPassthrough,
            gateway_timeout_settings: None,
        })
        .unwrap();
    let upstream = store.upstream_for_provider(&provider.id).unwrap();
    let mut routes = route_set(provider.id, upstream.provider_revision_id);
    routes.routes[0].public_model_id = "grok-managed".to_string();
    routes.routes[0].display_name = "Grok Managed".to_string();
    routes.routes[0].upstream_model_id = "grok-upstream".to_string();

    let instance_key = InstanceGatewayKey::for_test("grok-instance-key-sentinel");
    let runtime = ManagedGatewayRuntime::new();
    let status = runtime.ensure_serving().await.unwrap();
    runtime
        .register_binding(
            GatewayBinding {
                binding_id: "grok-contract-instance".to_string(),
                route_set: routes.clone(),
                admission_key_hash: instance_gateway_key_hash(&instance_key),
            },
            &store,
        )
        .await
        .unwrap();

    let root = std::env::temp_dir().join(format!("3r-grok-contract-{}", Uuid::new_v4()));
    let current_home = root.join("current-home");
    let managed_home = root.join("managed-home");
    std::fs::create_dir_all(&current_home).unwrap();
    std::fs::create_dir_all(&managed_home).unwrap();
    std::fs::write(
        current_home.join("config.toml"),
        "model = \"gpt-existing\"\nmodel_provider = \"OpenAI\"\n[features]\ngoals = true\n",
    )
    .unwrap();
    std::fs::write(
        managed_home.join("config.toml"),
        "[features]\ngoals = true\n",
    )
    .unwrap();
    let bundled = read_bundled_catalog(&codex_executable).unwrap();
    let catalog = build_version_matched_catalog(&bundled, &routes.routes).unwrap();
    materialize_managed_codex_config(
        &managed_home,
        "grok_contract",
        &Url::parse(status.base_url.as_deref().unwrap()).unwrap(),
        false,
        &catalog,
    )
    .unwrap();

    let executable = codex_executable.clone();
    let home = managed_home.clone();
    let key = instance_key.expose().to_string();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(executable)
            .args([
                "--profile",
                "grok_contract",
                "-c",
                "model_provider=\"rcodex\"",
                "exec",
                "--ephemeral",
                "--skip-git-repo-check",
                "--sandbox",
                "read-only",
                "--model",
                "grok-managed",
                "--json",
                "Reply exactly with GROK_CONTRACT_OK and do not use tools.",
            ])
            .current_dir(&home)
            .env("CODEX_HOME", &home)
            .env("CODEX_SQLITE_HOME", &home)
            .env(INSTANCE_GATEWAY_ENV, key)
            .env_remove("OPENAI_API_KEY")
            .env_remove("CODEX_API_KEY")
            .env_remove("CODEX_ACCESS_TOKEN")
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    if !output.status.success() {
        panic!(
            "real Codex Grok contract failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(String::from_utf8_lossy(&output.stdout).contains("GROK_CONTRACT_OK"));
    let (headers, request) = observed.lock().unwrap().take().unwrap();
    assert_eq!(request["model"], "grok-upstream");
    assert_eq!(
        headers.get(AUTHORIZATION).unwrap(),
        "Bearer grok-provider-key-sentinel"
    );
    assert!(headers.get(INSTANCE_GATEWAY_HEADER).is_none());
    assert_eq!(
        std::fs::read_to_string(current_home.join("config.toml")).unwrap(),
        "model = \"gpt-existing\"\nmodel_provider = \"OpenAI\"\n[features]\ngoals = true\n"
    );
    runtime.unregister_binding("grok-contract-instance").await;
    runtime.stop_if_unused().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn installed_codex_can_call_a_chat_only_grok_relay_when_requested() {
    let Ok(codex_executable) = std::env::var("CODEX_REAL_GROK_CONTRACT_BIN") else {
        return;
    };
    let codex_executable = PathBuf::from(codex_executable);
    let observed = Arc::new(StdMutex::new(None));
    let listener = TcpListener::bind((LOOPBACK_HOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/v1/chat/completions", post(mock_grok_chat_stream))
        .with_state(observed.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let vault = Arc::new(MemoryVault::default());
    let store = ProviderStore::from_database(Database::in_memory(), vault).unwrap();
    let provider = store
        .create(CreateProviderInput {
            name: "Chat-only Grok Relay".to_string(),
            base_url: format!("http://127.0.0.1:{port}/v1"),
            api_key: "grok-chat-key-sentinel".to_string(),
            transport_mode: TransportMode::GatewayChatCompletions,
            gateway_timeout_settings: None,
        })
        .unwrap();
    let upstream = store.upstream_for_provider(&provider.id).unwrap();
    let mut routes = route_set(provider.id, upstream.provider_revision_id);
    routes.routes[0].public_model_id = "grok-chat-managed".to_string();
    routes.routes[0].display_name = "Grok Chat Managed".to_string();
    routes.routes[0].upstream_model_id = "grok-upstream".to_string();
    routes.routes[0].wire_api = WireApi::ChatCompletions;
    routes.routes[0].adapter_kind = "chat_completions_to_responses".to_string();
    routes.routes[0].capabilities.supports_tools = false;

    let instance_key = InstanceGatewayKey::for_test("grok-chat-instance-key-sentinel");
    let runtime = ManagedGatewayRuntime::new();
    let status = runtime.ensure_serving().await.unwrap();
    runtime
        .register_binding(
            GatewayBinding {
                binding_id: "grok-chat-contract-instance".to_string(),
                route_set: routes.clone(),
                admission_key_hash: instance_gateway_key_hash(&instance_key),
            },
            &store,
        )
        .await
        .unwrap();

    let root = std::env::temp_dir().join(format!("3r-grok-chat-{}", Uuid::new_v4()));
    let current_home = root.join("current-home");
    let managed_home = root.join("managed-home");
    std::fs::create_dir_all(&current_home).unwrap();
    std::fs::create_dir_all(&managed_home).unwrap();
    std::fs::write(
        current_home.join("config.toml"),
        "model = \"gpt-existing\"\nmodel_provider = \"OpenAI\"\n[features]\ngoals = true\n",
    )
    .unwrap();
    std::fs::write(
        managed_home.join("config.toml"),
        "[features]\ngoals = true\n",
    )
    .unwrap();
    let bundled = read_bundled_catalog(&codex_executable).unwrap();
    let catalog = build_version_matched_catalog(&bundled, &routes.routes).unwrap();
    materialize_managed_codex_config(
        &managed_home,
        "grok_chat_contract",
        &Url::parse(status.base_url.as_deref().unwrap()).unwrap(),
        false,
        &catalog,
    )
    .unwrap();

    let executable = codex_executable.clone();
    let home = managed_home.clone();
    let key = instance_key.expose().to_string();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(executable)
            .args([
                "--profile",
                "grok_chat_contract",
                "exec",
                "--ephemeral",
                "--skip-git-repo-check",
                "--sandbox",
                "read-only",
                "--model",
                "grok-chat-managed",
                "--json",
                "Reply exactly with GROK_CONTRACT_OK and do not use tools.",
            ])
            .current_dir(&home)
            .env("CODEX_HOME", &home)
            .env("CODEX_SQLITE_HOME", &home)
            .env(INSTANCE_GATEWAY_ENV, key)
            .env_remove("OPENAI_API_KEY")
            .env_remove("CODEX_API_KEY")
            .env_remove("CODEX_ACCESS_TOKEN")
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    if !output.status.success() {
        panic!(
            "real Codex Chat-only Grok contract failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    if !String::from_utf8_lossy(&output.stdout).contains("GROK_CONTRACT_OK") {
        panic!(
            "real Codex Chat-only Grok output was incomplete: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let (headers, request) = observed.lock().unwrap().take().unwrap();
    assert_eq!(request["model"], "grok-upstream");
    assert!(request["messages"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(request.get("tools").is_none());
    assert!(request.get("reasoning").is_none());
    assert_eq!(
        headers.get(AUTHORIZATION).unwrap(),
        "Bearer grok-chat-key-sentinel"
    );
    assert!(headers.get(INSTANCE_GATEWAY_HEADER).is_none());
    assert_eq!(
        std::fs::read_to_string(current_home.join("config.toml")).unwrap(),
        "model = \"gpt-existing\"\nmodel_provider = \"OpenAI\"\n[features]\ngoals = true\n"
    );
    runtime
        .unregister_binding("grok-chat-contract-instance")
        .await;
    runtime.stop_if_unused().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn installed_codex_isolated_rcodex_does_not_emit_compaction_trigger() {
    let Ok(codex_executable) = std::env::var("CODEX_REAL_COMPACT_PROBE_BIN") else {
        return;
    };
    let codex_executable = PathBuf::from(codex_executable);
    let observed = Arc::new(StdMutex::new(Vec::new()));
    let listener = TcpListener::bind((LOOPBACK_HOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/v1/responses", post(mock_compact_probe_stream))
        .with_state(CompactProbeUpstream {
            observed: observed.clone(),
            next_id: Arc::new(StdMutex::new(0)),
        });
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let vault = Arc::new(MemoryVault::default());
    let store = ProviderStore::from_database(Database::in_memory(), vault).unwrap();
    let provider = store
        .create(CreateProviderInput {
            name: "Compact Probe Relay".to_string(),
            base_url: format!("http://127.0.0.1:{port}/v1"),
            api_key: "compact-probe-key-sentinel".to_string(),
            transport_mode: TransportMode::GatewayPassthrough,
            gateway_timeout_settings: None,
        })
        .unwrap();
    let upstream = store.upstream_for_provider(&provider.id).unwrap();
    let mut routes = route_set(provider.id, upstream.provider_revision_id);
    routes.routes[0].public_model_id = "compact-probe-managed".to_string();
    routes.routes[0].display_name = "Compact Probe Managed".to_string();
    routes.routes[0].upstream_model_id = "grok-4.6".to_string();
    routes.routes[0].capabilities.context_window = Some(400);

    let instance_key = InstanceGatewayKey::for_test("compact-probe-instance-key");
    let runtime = ManagedGatewayRuntime::new();
    let status = runtime.ensure_serving().await.unwrap();
    runtime
        .register_binding(
            GatewayBinding {
                binding_id: "compact-probe-instance".to_string(),
                route_set: routes.clone(),
                admission_key_hash: instance_gateway_key_hash(&instance_key),
            },
            &store,
        )
        .await
        .unwrap();

    let captured = Arc::new(StdMutex::new(Vec::new()));
    let capture_listener = TcpListener::bind((LOOPBACK_HOST, 0)).await.unwrap();
    let capture_port = capture_listener.local_addr().unwrap().port();
    let gateway_base = status.base_url.as_deref().unwrap().trim_end_matches('/');
    let capture_app = Router::new()
        .route("/v1/responses", post(capture_isolated_compact_request))
        .route(
            "/v1/responses/compact",
            post(capture_isolated_compact_v1_endpoint),
        )
        .route(
            "/responses/compact",
            post(capture_isolated_compact_v1_endpoint),
        )
        .with_state(IsolatedCompactCapture {
            observed: captured.clone(),
            gateway_responses_url: format!("{gateway_base}/responses"),
            client: reqwest::Client::new(),
        });
    tokio::spawn(async move {
        axum::serve(capture_listener, capture_app).await.unwrap();
    });

    let root = std::env::temp_dir().join(format!("3r-compact-probe-{}", Uuid::new_v4()));
    let current_home = root.join("current-home");
    let managed_home = root.join("managed-home");
    std::fs::create_dir_all(&current_home).unwrap();
    std::fs::create_dir_all(&managed_home).unwrap();
    std::fs::write(
        current_home.join("config.toml"),
        "model = \"gpt-existing\"\nmodel_provider = \"OpenAI\"\n[features]\ngoals = true\n",
    )
    .unwrap();
    std::fs::write(
        managed_home.join("config.toml"),
        "[features]\ngoals = true\n",
    )
    .unwrap();
    let bundled = read_bundled_catalog(&codex_executable).unwrap();
    let catalog = build_version_matched_catalog(&bundled, &routes.routes).unwrap();
    let capture_base = Url::parse(&format!("http://127.0.0.1:{capture_port}/v1")).unwrap();
    materialize_managed_codex_config(
        &managed_home,
        "compact_probe",
        &capture_base,
        false,
        &catalog,
    )
    .unwrap();

    let executable = codex_executable.clone();
    let home = managed_home.clone();
    let key = instance_key.expose().to_string();
    let first = tokio::task::spawn_blocking({
        let executable = executable.clone();
        let home = home.clone();
        let key = key.clone();
        move || {
            run_isolated_codex(
                &executable,
                &home,
                &key,
                &[
                    "--profile",
                    "compact_probe",
                    "-c",
                    "model_provider=\"rcodex\"",
                    "-c",
                    "model_auto_compact_token_limit=100",
                    "-c",
                    "model_context_window=400",
                    "exec",
                    "--skip-git-repo-check",
                    "--sandbox",
                    "read-only",
                    "--model",
                    "compact-probe-managed",
                    "--json",
                    "Reply exactly with COMPACT_PROBE_OK and do not use tools.",
                ],
            )
        }
    })
    .await
    .unwrap();
    if !first.status.success() {
        panic!(
            "isolated compact probe first turn failed: stdout={} stderr={}",
            String::from_utf8_lossy(&first.stdout),
            String::from_utf8_lossy(&first.stderr)
        );
    }
    let first_stdout = String::from_utf8_lossy(&first.stdout);
    let thread_id = jsonl_thread_id(&first_stdout)
        .unwrap_or_else(|| panic!("isolated compact probe missing thread_id: {first_stdout}"));

    let second = tokio::task::spawn_blocking({
        let executable = executable.clone();
        let home = home.clone();
        let key = key.clone();
        move || {
            run_isolated_codex(
                &executable,
                &home,
                &key,
                &[
                    "--profile",
                    "compact_probe",
                    "-c",
                    "model_provider=\"rcodex\"",
                    "-c",
                    "model_auto_compact_token_limit=100",
                    "-c",
                    "model_context_window=400",
                    "exec",
                    "resume",
                    &thread_id,
                    "--skip-git-repo-check",
                    "--json",
                    "--model",
                    "compact-probe-managed",
                    "Reply exactly with COMPACT_PROBE_OK and do not use tools.",
                ],
            )
        }
    })
    .await
    .unwrap();
    if !second.status.success() {
        panic!(
            "isolated compact probe resume failed: stdout={} stderr={}",
            String::from_utf8_lossy(&second.stdout),
            String::from_utf8_lossy(&second.stderr)
        );
    }
    let second_stdout = String::from_utf8_lossy(&second.stdout);
    assert!(
        second_stdout.contains("COMPACT_PROBE_OK"),
        "isolated compact probe resume output was incomplete: stdout={second_stdout} stderr={}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        second_stdout.to_lowercase().contains("compaction")
            || second_stdout.contains("Long threads and multiple compactions"),
        "isolated compact probe did not observe a local compaction event: {second_stdout}"
    );

    let captured_requests = captured.lock().unwrap().clone();
    assert!(
        captured_requests.len() >= 3,
        "isolated compact probe expected seed plus local-summary plus follow-up, got {}",
        captured_requests.len()
    );
    let captured_shapes = captured_requests
        .iter()
        .map(|(path, request)| {
            format!(
                "{path} trigger={} tools={}",
                request_contains_compaction_trigger(request),
                tools_len(request)
            )
        })
        .collect::<Vec<_>>();
    assert!(
        captured_requests
            .iter()
            .all(|(path, request)| path == "/v1/responses"
                && !request_contains_compaction_trigger(request)),
        "isolated rcodex emitted a remote compact contract: {captured_shapes:?}"
    );
    assert!(
        captured_requests
            .iter()
            .any(|(path, request)| path == "/v1/responses" && tools_len(request) == 0),
        "isolated compact probe expected a tool-less local summarizer turn: {captured_shapes:?}"
    );
    let upstream_requests = observed.lock().unwrap().clone();
    assert_eq!(upstream_requests.len(), captured_requests.len());
    assert!(upstream_requests.iter().all(|(_, request)| {
        request["model"] == "grok-4.6" && !request_contains_compaction_trigger(request)
    }));
    assert_eq!(
        std::fs::read_to_string(current_home.join("config.toml")).unwrap(),
        "model = \"gpt-existing\"\nmodel_provider = \"OpenAI\"\n[features]\ngoals = true\n"
    );

    runtime.unregister_binding("compact-probe-instance").await;
    runtime.stop_if_unused().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
