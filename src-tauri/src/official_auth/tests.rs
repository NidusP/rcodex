//! Codex app-server 认证适配测试。
//!
//! FakeTransport 留在本文件。不改断言。

use std::{
    collections::{BTreeMap, VecDeque},
    env, fs, io,
    path::Path,
    process::Command,
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};

use serde_json::{json, Value};

use super::*;

#[derive(Default)]
struct FakeState {
    sent: Vec<Value>,
    shutdown: bool,
}

struct FakeTransport {
    state: Arc<Mutex<FakeState>>,
    incoming: VecDeque<Result<Value, OfficialAuthError>>,
}

impl FakeTransport {
    fn new(incoming: Vec<Value>) -> (Self, Arc<Mutex<FakeState>>) {
        let state = Arc::new(Mutex::new(FakeState::default()));
        (
            Self {
                state: Arc::clone(&state),
                incoming: incoming.into_iter().map(Ok).collect(),
            },
            state,
        )
    }
}

impl JsonLineTransport for FakeTransport {
    fn send(&mut self, value: &Value) -> Result<(), OfficialAuthError> {
        self.state.lock().unwrap().sent.push(value.clone());
        Ok(())
    }

    fn receive(&mut self, _timeout: Duration) -> Result<Value, OfficialAuthError> {
        self.incoming
            .pop_front()
            .unwrap_or(Err(OfficialAuthError::Timeout {
                operation: "fake_receive",
            }))
    }

    fn shutdown(&mut self) {
        self.state.lock().unwrap().shutdown = true;
    }
}

fn test_home() -> ProbeDirectory {
    ProbeDirectory::create().unwrap()
}

fn initialized_session(
    mut messages: Vec<Value>,
) -> (CodexAppServerSession, Arc<Mutex<FakeState>>, ProbeDirectory) {
    let home = test_home();
    let canonical_home = fs::canonicalize(home.codex_home()).unwrap();
    messages.insert(
        0,
        json!({
            "id": 1,
            "result": {
                "codexHome": canonical_home,
                "userAgent": "fake",
                "platformFamily": "unix",
                "platformOs": "test",
                "additiveField": true
            }
        }),
    );
    let (transport, state) = FakeTransport::new(messages);
    let session = CodexAppServerSession::initialize(
        Box::new(transport),
        canonical_home,
        ClientInfo::new("test", "Test", "1.0.0"),
        Duration::from_millis(10),
        true,
    )
    .unwrap();
    (session, state, home)
}

#[test]
fn account_read_is_typed_and_accepts_additive_fields() {
    let (mut session, state, _home) = initialized_session(vec![json!({
        "id": 2,
        "result": {
            "account": {
                "type": "chatgpt",
                "email": "user@example.com",
                "planType": "plus",
                "newField": "ignored"
            },
            "requiresOpenaiAuth": true,
            "anotherNewField": true
        }
    })]);

    let account = session.read_account().unwrap();
    assert_eq!(
        account,
        AccountSnapshot {
            account: Some(AccountKind::ChatGpt {
                email: Some("user@example.com".to_owned()),
                plan_type: Some("plus".to_owned()),
            }),
            requires_openai_auth: true,
        }
    );
    let sent = &state.lock().unwrap().sent;
    assert_eq!(sent[0]["method"], "initialize");
    assert_eq!(sent[1]["method"], "initialized");
    assert_eq!(sent[2]["method"], "account/read");
    assert_eq!(sent[2]["params"]["refreshToken"], false);
}

#[test]
fn account_read_can_force_token_refresh() {
    let (mut session, state, _home) = initialized_session(vec![json!({
        "id": 2,
        "result": {
            "account": {
                "type": "chatgpt",
                "email": "user@example.test",
                "planType": "plus"
            },
            "requiresOpenaiAuth": true
        }
    })]);

    let account = session.read_account_with_refresh(true).unwrap();
    assert!(matches!(account.account, Some(AccountKind::ChatGpt { .. })));
    let sent = &state.lock().unwrap().sent;
    assert_eq!(sent[2]["method"], "account/read");
    assert_eq!(sent[2]["params"]["refreshToken"], true);
}

#[test]
fn notification_before_login_response_is_queued_and_correlated() {
    let (mut session, _state, _home) = initialized_session(vec![
        json!({
            "method": "account/login/completed",
            "params": { "loginId": "login-1", "success": true, "error": null }
        }),
        json!({
            "id": 2,
            "result": {
                "type": "chatgpt",
                "loginId": "login-1",
                "authUrl": "https://chatgpt.com/auth"
            }
        }),
    ]);

    let started = session
        .start_browser_login(BrowserLoginOptions::default())
        .unwrap();
    assert_eq!(started.login_id(), "login-1");
    let completed = session
        .wait_for_login_completion("login-1", Duration::from_millis(10))
        .unwrap();
    assert!(completed.success);
}

#[test]
fn cancel_tolerates_completion_notification_race() {
    let (mut session, _state, _home) = initialized_session(vec![
        json!({
            "id": 2,
            "result": {
                "type": "chatgptDeviceCode",
                "loginId": "login-2",
                "verificationUrl": "https://auth.openai.com/codex/device",
                "userCode": "ABCD-1234"
            }
        }),
        json!({
            "method": "account/login/completed",
            "params": { "loginId": "login-2", "success": false, "error": "cancelled" }
        }),
        json!({ "id": 3, "result": { "status": "canceled" } }),
    ]);

    session.start_device_code_login().unwrap();
    assert_eq!(
        session.cancel_login("login-2").unwrap(),
        CancelLoginStatus::Canceled
    );
    assert!(matches!(
        session
            .next_notification(Duration::from_millis(10))
            .unwrap(),
        AuthNotification::LoginCompleted(LoginCompleted { success: false, .. })
    ));
}

#[test]
fn poll_timeout_does_not_poison_session_and_cancel_still_works() {
    let home = test_home();
    let canonical_home = fs::canonicalize(home.codex_home()).unwrap();
    let state = Arc::new(Mutex::new(FakeState::default()));
    let transport = FakeTransport {
        state,
        incoming: VecDeque::from([
            Ok(json!({
                "id": 1,
                "result": {
                    "codexHome": canonical_home,
                    "userAgent": "fake",
                    "platformFamily": "unix",
                    "platformOs": "test"
                }
            })),
            Ok(json!({
                "id": 2,
                "result": {
                    "type": "chatgptDeviceCode",
                    "loginId": "login-poll",
                    "verificationUrl": "https://auth.openai.com/codex/device",
                    "userCode": "ABCD-1234"
                }
            })),
            Err(OfficialAuthError::Timeout {
                operation: "fake_poll",
            }),
            Ok(json!({ "id": 3, "result": { "status": "canceled" } })),
        ]),
    };
    let mut session = CodexAppServerSession::initialize(
        Box::new(transport),
        canonical_home,
        ClientInfo::new("test", "Test", "1"),
        Duration::from_millis(10),
        false,
    )
    .unwrap();
    session.start_device_code_login().unwrap();

    assert_eq!(
        session
            .poll_login_completion("login-poll", Duration::from_millis(1))
            .unwrap(),
        None
    );
    assert_eq!(
        session.cancel_login("login-poll").unwrap(),
        CancelLoginStatus::Canceled
    );
}

#[test]
fn timeout_poisons_session_and_prevents_late_response_reuse() {
    let (mut session, _state, _home) = initialized_session(Vec::new());
    assert!(matches!(
        session.read_account(),
        Err(OfficialAuthError::Timeout { .. })
    ));
    assert!(matches!(
        session.read_account(),
        Err(OfficialAuthError::SessionPoisoned)
    ));
}

#[test]
fn rpc_error_without_numeric_code_is_still_an_error() {
    let (mut session, _state, _home) = initialized_session(vec![json!({
        "id": 2,
        "error": { "message": "upstream detail is intentionally not retained" }
    })]);
    assert!(matches!(
        session.read_account(),
        Err(OfficialAuthError::Rpc { code: None })
    ));
}

#[test]
fn external_token_refresh_request_is_refused_without_breaking_correlation() {
    let (mut session, state, _home) = initialized_session(vec![
        json!({
            "id": "server-request-1",
            "method": "account/chatgptAuthTokens/refresh",
            "params": { "reason": "unauthorized", "previousAccountId": "not-retained" }
        }),
        json!({
            "id": 2,
            "result": { "account": null, "requiresOpenaiAuth": true }
        }),
    ]);
    assert_eq!(
        session.read_account().unwrap(),
        AccountSnapshot {
            account: None,
            requires_openai_auth: true,
        }
    );
    let sent = &state.lock().unwrap().sent;
    assert_eq!(sent[3]["id"], "server-request-1");
    assert_eq!(sent[3]["error"]["code"], -32601);
    assert!(sent[3].to_string().find("previousAccountId").is_none());
}

#[test]
fn login_urls_must_be_https() {
    let (mut session, _state, _home) = initialized_session(vec![json!({
        "id": 2,
        "result": {
            "type": "chatgpt",
            "loginId": "login-unsafe",
            "authUrl": "file:///tmp/not-allowed"
        }
    })]);
    assert!(matches!(
        session.start_browser_login(BrowserLoginOptions::default()),
        Err(OfficialAuthError::UnexpectedLoginResponse)
    ));
}

#[test]
fn logout_and_optional_rate_limits_have_typed_results() {
    let (mut session, _state, _home) = initialized_session(vec![
        json!({ "id": 2, "result": {} }),
        json!({
            "id": 3,
            "result": { "account": null, "requiresOpenaiAuth": true }
        }),
        json!({
            "id": 4,
            "result": {
                "rateLimits": {
                    "limitId": "codex",
                    "primary": {
                        "usedPercent": 25,
                        "windowDurationMins": 15,
                        "resetsAt": 1234
                    }
                },
                "rateLimitsByLimitId": {
                    "codex": {
                        "limitId": "codex",
                        "primary": { "usedPercent": 25 }
                    }
                },
                "additiveField": true
            }
        }),
    ]);
    session.logout().unwrap();
    let rate_limits = session.read_rate_limits().unwrap().unwrap();
    assert_eq!(rate_limits.rate_limits.primary.unwrap().used_percent, 25);
    assert_eq!(
        rate_limits.by_limit_id["codex"].limit_id.as_deref(),
        Some("codex")
    );
}

#[test]
fn malformed_transport_message_poisons_session() {
    let home = test_home();
    let canonical_home = fs::canonicalize(home.codex_home()).unwrap();
    let state = Arc::new(Mutex::new(FakeState::default()));
    let transport = FakeTransport {
        state,
        incoming: VecDeque::from([
            Ok(json!({ "id": 1, "result": { "codexHome": canonical_home } })),
            Err(OfficialAuthError::ProtocolMalformed),
        ]),
    };
    let mut session = CodexAppServerSession::initialize(
        Box::new(transport),
        canonical_home,
        ClientInfo::new("test", "Test", "1"),
        Duration::from_millis(10),
        false,
    )
    .unwrap();
    assert!(matches!(
        session.read_account(),
        Err(OfficialAuthError::ProtocolMalformed)
    ));
    assert!(matches!(
        session.read_account(),
        Err(OfficialAuthError::SessionPoisoned)
    ));
}

#[test]
fn schema_accepts_additive_fields_and_rejects_missing_required_method() {
    let mut documents = schema_fixture();
    documents.login_response["oneOf"][0]["properties"]["futureField"] = json!({ "type": "string" });
    let capabilities = validate_auth_schema(&documents).unwrap();
    assert!(capabilities.supports_rate_limits);

    documents.client_request["oneOf"]
        .as_array_mut()
        .unwrap()
        .retain(|variant| {
            variant
                .pointer("/properties/method/enum/0")
                .and_then(Value::as_str)
                != Some("account/logout")
        });
    assert!(matches!(
        validate_auth_schema(&documents),
        Err(OfficialAuthError::ProtocolIncompatible { .. })
    ));
}

#[test]
fn malformed_json_line_is_rejected_without_echoing_input() {
    let (sender, receiver) = mpsc::channel();
    read_json_lines(io::Cursor::new(b"not-json\n"), sender);
    assert!(matches!(
        receiver.recv().unwrap(),
        TransportEvent::Malformed
    ));
}

#[test]
fn isolated_command_does_not_inherit_auth_overrides() {
    let home = test_home();
    let command = isolated_command(Path::new("/bin/echo"), home.codex_home());
    let environment: BTreeMap<String, Option<String>> = command
        .get_envs()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
        })
        .collect();
    for forbidden in ["CODEX_ACCESS_TOKEN", "CODEX_API_KEY", "OPENAI_API_KEY"] {
        assert!(!environment.contains_key(forbidden));
    }
    assert_eq!(
        environment["CODEX_HOME"].as_deref(),
        Some(home.codex_home().to_string_lossy().as_ref())
    );
    assert_eq!(environment["CODEX_HOME"], environment["CODEX_SQLITE_HOME"]);
}

#[test]
fn installed_app_server_contract_when_explicitly_requested() {
    let Some(executable) = env::var_os("CODEX_REAL_APP_SERVER") else {
        return;
    };
    let adapter = CodexAppServerAdapter::probe(executable).unwrap();
    let home = test_home();
    let mut session = adapter
        .start_session(
            home.codex_home(),
            ClientInfo::new("rcodex_contract_test", "R Codex contract test", "0.0.0"),
        )
        .unwrap();
    let account = session.read_account().unwrap();
    assert_eq!(account.account, None);
    assert!(account.requires_openai_auth);
    assert!(adapter.probe_result().supports_rate_limits);
}

#[cfg(unix)]
#[test]
fn process_transport_drains_stderr_without_mixing_it_into_jsonl() {
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg(
        "i=0; while [ $i -lt 8000 ]; do printf 'stderr-only-noise\\n' >&2; i=$((i+1)); done; IFS= read -r line; printf '%s\\n' '{\"id\":1,\"result\":{}}'",
    );
    let mut transport = ProcessTransport::spawn(command).unwrap();
    transport
        .send(&json!({ "id": 1, "method": "probe" }))
        .unwrap();
    let message = transport.receive(Duration::from_secs(3)).unwrap();
    assert_eq!(message["id"], 1);
    transport.shutdown();
}

fn method_schema(methods: &[&str]) -> Value {
    json!({
        "oneOf": methods.iter().map(|method| json!({
            "properties": { "method": { "enum": [method] } },
            "required": ["method"],
            "type": "object"
        })).collect::<Vec<_>>()
    })
}

fn object_schema(required: &[&str], optional: &[&str]) -> Value {
    let mut properties = serde_json::Map::new();
    for field in required.iter().chain(optional.iter()) {
        properties.insert((*field).to_owned(), json!({}));
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required
    })
}

fn variant(discriminator: &str, required: &[&str]) -> Value {
    let mut schema = object_schema(required, &[]);
    schema["properties"]["type"] = json!({ "enum": [discriminator] });
    schema
}

fn schema_fixture() -> SchemaDocuments {
    let mut account_response = object_schema(&["requiresOpenaiAuth"], &["account"]);
    account_response["definitions"] = json!({
        "Account": {
            "oneOf": [variant("chatgpt", &["type", "email", "planType"])]
        }
    });
    SchemaDocuments {
        client_request: method_schema(&[
            "initialize",
            "account/read",
            "account/login/start",
            "account/login/cancel",
            "account/logout",
            "account/rateLimits/read",
        ]),
        client_notification: method_schema(&["initialized"]),
        server_notification: method_schema(&["account/login/completed", "account/updated"]),
        initialize_params: object_schema(&["clientInfo"], &[]),
        initialize_response: object_schema(
            &["codexHome", "userAgent", "platformFamily", "platformOs"],
            &[],
        ),
        login_params: json!({
            "oneOf": [
                variant("chatgpt", &["type"]),
                variant("chatgptDeviceCode", &["type"]),
                variant("chatgptAuthTokens", &["type", "accessToken"])
            ]
        }),
        login_response: json!({
            "oneOf": [
                variant("chatgpt", &["type", "loginId", "authUrl"]),
                variant(
                    "chatgptDeviceCode",
                    &["type", "loginId", "verificationUrl", "userCode"]
                )
            ]
        }),
        account_response,
        cancel_params: object_schema(&["loginId"], &[]),
        cancel_response: object_schema(&["status"], &[]),
        logout_response: object_schema(&[], &[]),
        login_completed: object_schema(&["success"], &["loginId", "error"]),
        account_updated: object_schema(&[], &["authMode", "planType"]),
    }
}
