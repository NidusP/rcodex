//! OfficialAccountService 行为测试。
//!
//! FakeDriver 留在本文件。不改断言。

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use rusqlite::params;
use url::Url;
use uuid::Uuid;

use crate::database::Database;
use crate::official_auth::{
    AccountKind, AccountRateLimits, AccountSnapshot, CancelLoginStatus, CodexAppServerProbe,
    LoginCompleted, LoginStart, OfficialAuthError,
};

use super::*;

#[derive(Clone)]
struct FakeDriver {
    state: Arc<Mutex<FakeState>>,
}

struct FakeState {
    completion: Option<LoginCompleted>,
    account: AccountSnapshot,
    cancel_status: CancelLoginStatus,
    login_starts: usize,
    logouts: usize,
}

impl FakeDriver {
    fn authorized(email: &str) -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeState {
                completion: Some(LoginCompleted {
                    login_id: Some("login-1".to_owned()),
                    success: true,
                    transient_error: None,
                }),
                account: AccountSnapshot {
                    account: Some(AccountKind::ChatGpt {
                        email: Some(email.to_owned()),
                        plan_type: Some("plus".to_owned()),
                    }),
                    requires_openai_auth: true,
                },
                cancel_status: CancelLoginStatus::Canceled,
                login_starts: 0,
                logouts: 0,
            })),
        }
    }

    fn login_starts(&self) -> usize {
        self.state.lock().unwrap().login_starts
    }
}

impl AuthDriver for FakeDriver {
    fn probe(&self, executable: &Path) -> Result<Box<dyn ProbedAuth>, OfficialAuthError> {
        Ok(Box::new(FakeProbed {
            state: self.state.clone(),
            probe: CodexAppServerProbe {
                executable: executable.to_owned(),
                version: "codex-cli-test".to_owned(),
                schema_fingerprint: "fnv1a64:test".to_owned(),
                supports_rate_limits: false,
            },
        }))
    }
}

struct FakeProbed {
    state: Arc<Mutex<FakeState>>,
    probe: CodexAppServerProbe,
}

impl ProbedAuth for FakeProbed {
    fn metadata(&self) -> &CodexAppServerProbe {
        &self.probe
    }

    fn start_session(&self, _codex_home: &Path) -> Result<Box<dyn AuthSession>, OfficialAuthError> {
        Ok(Box::new(FakeSession {
            state: self.state.clone(),
        }))
    }
}

struct FakeSession {
    state: Arc<Mutex<FakeState>>,
}

impl AuthSession for FakeSession {
    fn start_browser_login(&mut self) -> Result<LoginStart, OfficialAuthError> {
        self.state.lock().unwrap().login_starts += 1;
        Ok(LoginStart::Browser {
            login_id: "login-1".to_owned(),
            auth_url: Url::parse("https://example.test/authorize").unwrap(),
        })
    }

    fn start_device_code_login(&mut self) -> Result<LoginStart, OfficialAuthError> {
        self.state.lock().unwrap().login_starts += 1;
        Ok(LoginStart::DeviceCode {
            login_id: "login-1".to_owned(),
            verification_url: Url::parse("https://example.test/device").unwrap(),
            user_code: "ABCD-EFGH".to_owned(),
        })
    }

    fn poll_login_completion(
        &mut self,
        _login_id: &str,
        _timeout: Duration,
    ) -> Result<Option<LoginCompleted>, OfficialAuthError> {
        Ok(self.state.lock().unwrap().completion.take())
    }

    fn cancel_login(&mut self, _login_id: &str) -> Result<CancelLoginStatus, OfficialAuthError> {
        Ok(self.state.lock().unwrap().cancel_status)
    }

    fn read_account(&mut self) -> Result<AccountSnapshot, OfficialAuthError> {
        Ok(self.state.lock().unwrap().account.clone())
    }

    fn read_rate_limits(&mut self) -> Result<Option<AccountRateLimits>, OfficialAuthError> {
        Ok(None)
    }

    fn logout(&mut self) -> Result<(), OfficialAuthError> {
        self.state.lock().unwrap().logouts += 1;
        Ok(())
    }
}

struct TestService {
    service: OfficialAccountService,
    database: Database,
    root: PathBuf,
}

impl TestService {
    fn new(driver: FakeDriver) -> Self {
        let database = Database::in_memory();
        let root =
            std::env::temp_dir().join(format!("rcodex-official-accounts-test-{}", Uuid::new_v4()));
        let service = OfficialAccountService::new(database.clone(), root.clone(), Arc::new(driver));
        Self {
            service,
            database,
            root,
        }
    }
}

impl Drop for TestService {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn input(account_id: Option<String>) -> StartAuthorizationInput {
    StartAuthorizationInput {
        mode: AuthorizationMode::Browser,
        account_id,
        workspace_label: Some("Personal".to_owned()),
        codex_executable: "/fake/codex".to_owned(),
    }
}

#[test]
fn successful_authorization_commits_only_masked_projection() {
    let driver = FakeDriver::authorized("alice@example.com");
    let test = TestService::new(driver);
    let started = test.service.start_authorization(input(None)).unwrap();
    assert!(matches!(
        started.prompt,
        AuthorizationPrompt::Browser { .. }
    ));
    let status = test
        .service
        .poll_authorization(&started.attempt_id)
        .unwrap();
    assert_eq!(status.state, AuthorizationState::Authorized);

    let connection = test.database.connection();
    let connection = connection.lock().unwrap();
    let row: (Option<String>, Option<String>, Option<String>, String, i64) = connection
        .query_row(
            "SELECT a.masked_identity, a.plan_label,
                    a.current_credential_revision_id, r.auth_state,
                    (SELECT COUNT(*) FROM resource_operation_leases)
             FROM codex_accounts a JOIN official_credential_revisions r
               ON r.id=a.current_credential_revision_id WHERE a.id=?1",
            [&started.account_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(row.0.as_deref(), Some("a***e@example.com"));
    assert_eq!(row.1.as_deref(), Some("plus"));
    assert_eq!(
        row.2.as_deref(),
        Some(started.credential_revision_id.as_str())
    );
    assert_eq!(row.3, "authorized");
    assert_eq!(row.4, 0);
    let raw_email_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM codex_accounts WHERE masked_identity='alice@example.com'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(raw_email_count, 0);
}

#[test]
fn cancellation_releases_lease_and_marks_attempt_terminal() {
    let driver = FakeDriver::authorized("alice@example.com");
    driver.state.lock().unwrap().completion = None;
    let test = TestService::new(driver);
    let started = test.service.start_authorization(input(None)).unwrap();
    let waiting = test
        .service
        .poll_authorization(&started.attempt_id)
        .unwrap();
    assert_eq!(waiting.state, AuthorizationState::AwaitingUser);
    let status = test
        .service
        .cancel_authorization(&started.attempt_id)
        .unwrap();
    assert_eq!(status.state, AuthorizationState::Cancelled);
    assert_eq!(
        status.error_code.as_deref(),
        Some("authorization_cancelled")
    );
    let connection = test.database.connection();
    let leases: i64 = connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM resource_operation_leases",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(leases, 0);
}

#[test]
fn device_code_prompt_is_transient_and_unavailable_after_cancel() {
    let driver = FakeDriver::authorized("alice@example.com");
    driver.state.lock().unwrap().completion = None;
    let test = TestService::new(driver);
    let mut request = input(None);
    request.mode = AuthorizationMode::DeviceCode;
    let started = test.service.start_authorization(request).unwrap();
    assert!(matches!(
        started.prompt,
        AuthorizationPrompt::DeviceCode { ref user_code, .. } if user_code == "ABCD-EFGH"
    ));
    assert_eq!(
        test.service
            .authorization_prompt_url(&started.attempt_id)
            .unwrap(),
        "https://example.test/device"
    );
    test.service
        .cancel_authorization(&started.attempt_id)
        .unwrap();
    assert_eq!(
        test.service
            .authorization_prompt_url(&started.attempt_id)
            .unwrap_err()
            .code,
        "authorization_attempt_not_active"
    );
}

#[test]
fn startup_recovery_marks_abandoned_authorization_interrupted() {
    let driver = FakeDriver::authorized("alice@example.com");
    driver.state.lock().unwrap().completion = None;
    let test = TestService::new(driver);
    let started = test.service.start_authorization(input(None)).unwrap();
    test.service.pending.lock().unwrap().clear();

    test.service.recover_interrupted_authorizations().unwrap();

    let status = test
        .service
        .authorization_status(&started.attempt_id)
        .unwrap();
    assert_eq!(status.state, AuthorizationState::Interrupted);
    let connection = test.database.connection();
    let connection = connection.lock().unwrap();
    let revision: (String, Option<String>) = connection
        .query_row(
            "SELECT auth_state, auth_error_code
             FROM official_credential_revisions WHERE id=?1",
            [&started.credential_revision_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(revision.0, "failed");
    assert_eq!(revision.1.as_deref(), Some("authorization_interrupted"));
    let lease_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM resource_operation_leases",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(lease_count, 0);
}

#[test]
fn failed_reauthorization_does_not_replace_old_current_revision() {
    let driver = FakeDriver::authorized("alice@example.com");
    driver.state.lock().unwrap().completion = Some(LoginCompleted {
        login_id: Some("login-1".to_owned()),
        success: false,
        transient_error: Some("must not persist".to_owned()),
    });
    let test = TestService::new(driver);
    let account_id = "account-existing";
    seed_authorized_account(&test.database, account_id, "revision-old");

    let started = test
        .service
        .start_authorization(input(Some(account_id.to_owned())))
        .unwrap();
    let status = test
        .service
        .poll_authorization(&started.attempt_id)
        .unwrap();
    assert_eq!(status.state, AuthorizationState::Failed);

    let connection = test.database.connection();
    let connection = connection.lock().unwrap();
    let current: Option<String> = connection
        .query_row(
            "SELECT current_credential_revision_id FROM codex_accounts WHERE id=?1",
            [account_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(current.as_deref(), Some("revision-old"));
    let old_state: String = connection
        .query_row(
            "SELECT auth_state FROM official_credential_revisions WHERE id='revision-old'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(old_state, "authorized");
    let persisted_transient: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM codex_authorization_attempts
             WHERE error_code='must not persist'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(persisted_transient, 0);
}

#[test]
fn unsupported_region_login_failure_has_stable_error_code() {
    assert_eq!(
        login_completion_error_code(&LoginCompleted {
            login_id: Some("login-1".to_owned()),
            success: false,
            transient_error: Some("unsupported_country_region_territory".to_owned()),
        }),
        "official_auth_region_unsupported"
    );
    assert_eq!(
        login_completion_error_code(&LoginCompleted {
            login_id: Some("login-1".to_owned()),
            success: false,
            transient_error: Some("generic upstream failure".to_owned()),
        }),
        "official_login_failed"
    );
}

#[test]
fn runtime_lock_rejects_reauthorization_before_login_starts() {
    let driver = FakeDriver::authorized("alice@example.com");
    let test = TestService::new(driver.clone());
    let account_id = "account-locked";
    let revision_id = "revision-locked";
    seed_authorized_account(&test.database, account_id, revision_id);
    seed_runtime_lock(&test.database, account_id, revision_id);

    let error = test
        .service
        .start_authorization(input(Some(account_id.to_owned())))
        .unwrap_err();
    assert_eq!(error.code, "configuration_in_use");
    assert_eq!(driver.login_starts(), 0);
}

#[test]
fn runtime_lock_rejects_all_account_mutations() {
    let driver = FakeDriver::authorized("alice@example.com");
    let test = TestService::new(driver);
    let account_id = "account-locked-actions";
    let revision_id = "revision-locked-actions";
    seed_authorized_account(&test.database, account_id, revision_id);
    seed_runtime_lock(&test.database, account_id, revision_id);
    let action = || AccountActionInput {
        account_id: account_id.to_owned(),
        codex_executable: "/fake/codex".to_owned(),
    };

    assert_eq!(
        test.service
            .refresh_account_status(action())
            .unwrap_err()
            .code,
        "configuration_in_use"
    );
    assert_eq!(
        test.service.disconnect_account(action()).unwrap_err().code,
        "configuration_in_use"
    );
    assert_eq!(
        test.service.delete_account(action()).unwrap_err().code,
        "configuration_in_use"
    );
}

#[test]
fn operation_lease_rejects_concurrent_authorization_for_same_account() {
    let driver = FakeDriver::authorized("alice@example.com");
    driver.state.lock().unwrap().completion = None;
    let test = TestService::new(driver.clone());
    let first = test.service.start_authorization(input(None)).unwrap();
    let second = test
        .service
        .start_authorization(input(Some(first.account_id.clone())))
        .unwrap_err();

    assert_eq!(second.code, "resource_operation_in_progress");
    assert_eq!(driver.login_starts(), 1);
    test.service
        .cancel_authorization(&first.attempt_id)
        .unwrap();
}

#[test]
fn delete_tombstones_account_and_removes_only_its_managed_home() {
    let driver = FakeDriver::authorized("alice@example.com");
    let test = TestService::new(driver);
    let account_id = "account-delete";
    let revision_id = "revision-delete";
    seed_authorized_account(&test.database, account_id, revision_id);
    let managed_home = test.root.join(account_id).join(revision_id);
    fs::create_dir_all(&managed_home).unwrap();
    fs::write(managed_home.join("auth.json"), "sensitive-test-fixture").unwrap();
    test.database
        .connection()
        .lock()
        .unwrap()
        .execute(
            "UPDATE official_credential_revisions
             SET managed_codex_home=?2 WHERE id=?1",
            params![revision_id, managed_home.display().to_string()],
        )
        .unwrap();

    let status = test
        .service
        .delete_account(AccountActionInput {
            account_id: account_id.to_string(),
            codex_executable: String::new(),
        })
        .unwrap();

    assert_eq!(status.auth_state, "deleted");
    assert!(!managed_home.exists());
    let revision: (String, Option<i64>) = test
        .database
        .connection()
        .lock()
        .unwrap()
        .query_row(
            "SELECT auth_state, retired_at
             FROM official_credential_revisions WHERE id=?1",
            [revision_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(revision.0, "retired");
    assert!(revision.1.is_some());
}

fn seed_authorized_account(database: &Database, account_id: &str, revision_id: &str) {
    let connection = database.connection();
    let connection = connection.lock().unwrap();
    connection
        .execute(
            "INSERT INTO codex_accounts (
                id, masked_identity, workspace_label, plan_label,
                current_credential_revision_id, created_at, updated_at
             ) VALUES (?1, 'o***d@example.test', NULL, 'plus', NULL, 1, 1)",
            [account_id],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO official_credential_revisions (
                id, account_id, managed_codex_home, auth_state, auth_checked_at,
                auth_error_code, codex_version, schema_fingerprint, created_at, retired_at
             ) VALUES (?1, ?2, ?3, 'authorized', 1, NULL, 'test', 'test', 1, NULL)",
            params![
                revision_id,
                account_id,
                format!("/tmp/{account_id}/{revision_id}"),
            ],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE codex_accounts SET current_credential_revision_id=?2 WHERE id=?1",
            params![account_id, revision_id],
        )
        .unwrap();
}

fn seed_runtime_lock(database: &Database, account_id: &str, revision_id: &str) {
    let connection = database.connection();
    let connection = connection.lock().unwrap();
    connection
        .execute(
            "INSERT INTO launch_presets (
                id, name, credential_kind, api_key_provider_id, codex_account_id,
                gateway_policy, profile_options, default_workspace_path, version,
                is_preferred, created_at, updated_at
             ) VALUES ('preset-1', 'Official', 'official', NULL, ?1, NULL,
                       '{}', NULL, 1, 0, 1, 1)",
            [account_id],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO launch_configuration_revisions (
                id, launch_preset_id, launch_preset_version, credential_kind,
                profile_snapshot_hash, config_snapshot, codex_executable,
                codex_version, created_at
             ) VALUES ('launch-1', 'preset-1', 1, 'official', 'hash', '{}',
                       '/fake/codex', 'test', 1)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO launch_configuration_dependencies (
                launch_configuration_revision_id, resource_kind,
                resource_id, resource_revision_id
             ) VALUES ('launch-1', 'official_account', ?1, ?2)",
            params![account_id, revision_id],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO codex_instances (
                id, launch_configuration_revision_id, credential_kind,
                api_key_provider_revision_id, official_credential_revision_id,
                workspace_path, managed_codex_home, lifecycle_dir, launch_nonce,
                state, supervisor_pid, pid, process_started_at, launched_at,
                exited_at, exit_code
             ) VALUES ('instance-1', 'launch-1', 'official', NULL, ?1, NULL,
                       '/tmp/managed', '/tmp/lifecycle', 'nonce-1', 'running',
                       1, 2, 'identity', 1, NULL, NULL)",
            [revision_id],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO runtime_configuration_locks (
                instance_id, launch_configuration_revision_id, acquired_at, released_at
             ) VALUES ('instance-1', 'launch-1', 1, NULL)",
            [],
        )
        .unwrap();
}
