//! Gateway 到独立磁盘写入线程的有界采集通道。

use super::{identity, ConversationArchive};
use axum::http::HeaderMap;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc,
    },
    time::Duration,
};
use uuid::Uuid;

const QUEUE_BYTES: usize = 64 * 1024 * 1024;
const QUEUE_EVENTS: usize = 1024;
const MAX_ACTIVE: usize = 256;
const EVENT_BYTES: usize = 64 * 1024;
const MAX_SECRET_BYTES: usize = 8192;
const MAX_SECRETS: usize = 32;
const MAX_METADATA_BYTES: usize = 64 * 1024;

#[derive(Clone)]
pub struct ArchiveCapture {
    sender: SyncSender<Event>,
    queued_bytes: Arc<AtomicUsize>,
    policy_enabled: Arc<AtomicBool>,
    policy_generation: Arc<AtomicU64>,
    active_count: Arc<AtomicUsize>,
    dropped_events: Arc<AtomicU64>,
}

impl ArchiveCapture {
    pub fn is_enabled(&self) -> bool {
        self.policy_enabled.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub fn dropped_events(&self) -> u64 {
        self.dropped_events.load(Ordering::Relaxed)
    }

    pub fn start(store: ConversationArchive) -> Self {
        let (sender, receiver) = mpsc::sync_channel(QUEUE_EVENTS);
        let capture = Self {
            sender,
            queued_bytes: Arc::new(AtomicUsize::new(0)),
            policy_enabled: Arc::new(AtomicBool::new(
                store.status().map(|s| s.enabled).unwrap_or(false),
            )),
            policy_generation: Arc::new(AtomicU64::new(0)),
            active_count: Arc::new(AtomicUsize::new(0)),
            dropped_events: Arc::new(AtomicU64::new(0)),
        };
        let queued_bytes = capture.queued_bytes.clone();
        std::thread::Builder::new()
            .name("rcodex-archive".into())
            .spawn(move || worker(store, receiver, queued_bytes))
            .expect("archive worker creation failed");
        capture
    }

    pub fn begin(
        &self,
        scope: String,
        headers: &HeaderMap,
        body: &[u8],
        home: Option<PathBuf>,
        secrets: Vec<String>,
    ) -> Option<RequestCapture> {
        if !self.is_enabled()
            || self
                .active_count
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                    (count < MAX_ACTIVE).then_some(count + 1)
                })
                .is_err()
        {
            self.dropped_events.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        let mut identity_headers = HeaderMap::new();
        let mut known_secrets = Vec::new();
        let mut registration_failed = false;
        for (name, value) in headers {
            if is_secret_header(name.as_str()) {
                if let Ok(value) = value.to_str() {
                    registration_failed |= !insert_secret(&mut known_secrets, value);
                } else {
                    registration_failed = true;
                }
            } else if matches!(
                name.as_str(),
                "session-id" | "thread-id" | "x-codex-turn-metadata"
            ) {
                identity_headers.append(name.clone(), value.clone());
            }
        }
        for secret in secrets {
            registration_failed |= !insert_secret(&mut known_secrets, &secret);
        }
        if registration_failed {
            self.active_count.fetch_sub(1, Ordering::AcqRel);
            self.dropped_events.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        let flags = Arc::new(CaptureFlags::default());
        let inner = Arc::new(CaptureInner {
            request_id: Uuid::new_v4().to_string(),
            capture: self.clone(),
            flags: flags.clone(),
            known_secrets: AtomicUsize::new(known_secrets.len()),
            start_generation: self.policy_generation.load(Ordering::Acquire),
        });
        let capture = RequestCapture { inner };
        // Ingress is captured before route mutation. The Gateway's later
        // stage("ingress") call is ignored to avoid doubling this body.
        let body = sanitize_json_fields(body);
        let len = body.len();
        if !capture.enqueue(
            Event::Begin {
                id: capture.id().into(),
                scope,
                headers: identity_headers,
                body,
                home,
                secrets: known_secrets,
                flags,
            },
            len,
        ) {
            return None;
        }
        Some(capture)
    }

    pub fn set_enabled(&self, enabled: bool) {
        if !enabled {
            self.policy_generation.fetch_add(1, Ordering::AcqRel);
        }
        self.policy_enabled.store(enabled, Ordering::Release);
    }
}

#[derive(Clone)]
pub struct RequestCapture {
    inner: Arc<CaptureInner>,
}

#[derive(Default)]
struct CaptureFlags {
    sent: AtomicU64,
    finished: AtomicBool,
    partial: AtomicBool,
    disabled: AtomicBool,
}

struct CaptureInner {
    request_id: String,
    capture: ArchiveCapture,
    flags: Arc<CaptureFlags>,
    known_secrets: AtomicUsize,
    start_generation: u64,
}

impl RequestCapture {
    pub fn id(&self) -> &str {
        &self.inner.request_id
    }

    pub fn stage(&self, stage: &'static str, body: &[u8]) {
        if stage == "ingress" || !super::valid_stage(stage) || self.should_stop() {
            return;
        }
        // A synchronous response can be very large. Enqueue bounded chunks
        // rather than cloning it once more as a single event.
        for chunk in body.chunks(EVENT_BYTES) {
            if self.should_stop() {
                break;
            }
            let body = sanitize_json_fields(chunk);
            let len = body.len();
            if !self.enqueue(
                Event::Stage {
                    id: self.id().into(),
                    stage,
                    observed: chunk.len() as u64,
                    body,
                },
                len,
            ) {
                break;
            }
        }
    }

    pub fn metadata(&self, mut value: Value) {
        if self.should_stop() {
            return;
        }
        redact_auth_fields(&mut value);
        let Ok(body) = serde_json::to_vec(&value) else {
            self.mark_partial();
            return;
        };
        if body.len() > MAX_METADATA_BYTES {
            self.mark_partial();
            return;
        }
        let len = body.len();
        self.enqueue(
            Event::Metadata {
                id: self.id().into(),
                body,
            },
            len,
        );
    }

    pub fn add_secret(&self, value: &str) {
        if value.is_empty() || self.should_stop() {
            return;
        }
        if value.len() > MAX_SECRET_BYTES
            || self
                .inner
                .known_secrets
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                    (count < MAX_SECRETS).then_some(count + 1)
                })
                .is_err()
        {
            self.mark_partial();
            self.inner.flags.disabled.store(true, Ordering::Release);
            return;
        }
        let body = value.as_bytes().to_vec();
        let len = body.len();
        if !self.enqueue(
            Event::Secret {
                id: self.id().into(),
                body,
            },
            len,
        ) {
            // A missing credential rule must fail closed for future capture.
            self.inner.flags.disabled.store(true, Ordering::Release);
        }
    }

    fn should_stop(&self) -> bool {
        if !self.inner.capture.is_enabled()
            || self.inner.start_generation
                != self.inner.capture.policy_generation.load(Ordering::Acquire)
        {
            self.mark_partial();
            return true;
        }
        self.inner.flags.disabled.load(Ordering::Acquire)
    }
    fn mark_partial(&self) {
        self.inner.flags.partial.store(true, Ordering::Release);
    }

    fn enqueue(&self, event: Event, bytes: usize) -> bool {
        let queue = &self.inner.capture.queued_bytes;
        if queue
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                count.checked_add(bytes).filter(|next| *next <= QUEUE_BYTES)
            })
            .is_err()
        {
            self.mark_partial();
            self.inner
                .capture
                .dropped_events
                .fetch_add(1, Ordering::Relaxed);
            return false;
        }
        self.inner.flags.sent.fetch_add(1, Ordering::AcqRel);
        match self.inner.capture.sender.try_send(event) {
            Ok(()) => true,
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                self.inner.flags.sent.fetch_sub(1, Ordering::AcqRel);
                queue.fetch_sub(bytes, Ordering::AcqRel);
                self.mark_partial();
                self.inner
                    .capture
                    .dropped_events
                    .fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }
}

impl Drop for CaptureInner {
    fn drop(&mut self) {
        // Worker holds only CaptureFlags. No Arc cycle can suppress this Drop;
        // the marker works even when the bounded event queue is full.
        if !self.capture.is_enabled()
            || self.start_generation != self.capture.policy_generation.load(Ordering::Acquire)
        {
            self.flags.partial.store(true, Ordering::Release);
        }
        self.flags.finished.store(true, Ordering::Release);
        self.capture.active_count.fetch_sub(1, Ordering::AcqRel);
    }
}

enum Event {
    Begin {
        id: String,
        scope: String,
        headers: HeaderMap,
        body: Vec<u8>,
        home: Option<PathBuf>,
        secrets: Vec<Vec<u8>>,
        flags: Arc<CaptureFlags>,
    },
    Stage {
        id: String,
        stage: &'static str,
        observed: u64,
        body: Vec<u8>,
    },
    Metadata {
        id: String,
        body: Vec<u8>,
    },
    Secret {
        id: String,
        body: Vec<u8>,
    },
}

struct Active {
    flags: Arc<CaptureFlags>,
    processed: u64,
    stages: HashMap<&'static str, StageRedactor>,
    secrets: Vec<Vec<u8>>,
    metadata: serde_json::Map<String, Value>,
    failed: bool,
}

fn worker(store: ConversationArchive, receiver: Receiver<Event>, queued_bytes: Arc<AtomicUsize>) {
    let _ = store.recover_interrupted();
    let mut active: HashMap<String, Active> = HashMap::new();
    loop {
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(Event::Begin {
                id,
                scope,
                headers,
                body,
                home,
                secrets,
                flags,
            }) => {
                queued_bytes.fetch_sub(body.len(), Ordering::AcqRel);
                let claims = identity::extract_candidates(&headers, &body);
                let identity = home
                    .as_ref()
                    .map(|home| identity::verify_identity(home, None, &claims));
                let key = identity
                    .as_ref()
                    .filter(|i| i.status == "verified")
                    .and_then(|i| i.native_thread_id.as_deref());
                let correlation = identity
                    .as_ref()
                    .map(|i| i.status.as_str())
                    .unwrap_or("unmatched");
                if store
                    .begin_request(&id, &scope, key, correlation)
                    .unwrap_or(false)
                {
                    let mut request = Active {
                        flags,
                        processed: 1,
                        stages: HashMap::new(),
                        secrets,
                        metadata: serde_json::Map::new(),
                        failed: false,
                    };
                    if let Some(identity) = identity {
                        if let Ok(bytes) = serde_json::to_vec(&identity) {
                            write_chunk(
                                &store,
                                &id,
                                "identity",
                                &bytes,
                                bytes.len() as u64,
                                &mut request,
                            );
                        }
                    }
                    write_chunk(
                        &store,
                        &id,
                        "ingress",
                        &body,
                        body.len() as u64,
                        &mut request,
                    );
                    active.insert(id, request);
                }
            }
            Ok(Event::Stage {
                id,
                stage,
                observed,
                body,
            }) => {
                queued_bytes.fetch_sub(body.len(), Ordering::AcqRel);
                if let Some(request) = active.get_mut(&id) {
                    request.processed += 1;
                    write_chunk(&store, &id, stage, &body, observed, request);
                }
            }
            Ok(Event::Metadata { id, body }) => {
                queued_bytes.fetch_sub(body.len(), Ordering::AcqRel);
                if let Some(request) = active.get_mut(&id) {
                    request.processed += 1;
                    if let Ok(Value::Object(map)) = serde_json::from_slice::<Value>(&body) {
                        request.metadata.extend(map);
                    } else {
                        request.failed = true;
                    }
                }
            }
            Ok(Event::Secret { id, body }) => {
                queued_bytes.fetch_sub(body.len(), Ordering::AcqRel);
                if let Some(request) = active.get_mut(&id) {
                    request.processed += 1;
                    insert_secret_bytes(&mut request.secrets, body);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        // Last handle can be dropped while its last chunk still waits in the
        // queue. Only publish after every successfully enqueued event is read.
        let ready: Vec<String> = active
            .iter()
            .filter(|(_, request)| {
                request.flags.finished.load(Ordering::Acquire)
                    && request.processed >= request.flags.sent.load(Ordering::Acquire)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in ready {
            if let Some(request) = active.remove(&id) {
                finish(&store, &id, request);
            }
        }
    }
}

fn finish(store: &ConversationArchive, id: &str, request: Active) {
    let mut partial = request.failed || request.flags.partial.load(Ordering::Acquire);
    for (stage, mut redactor) in request.stages {
        let remaining = redactor.feed(&[], &request.secrets, true);
        if store.append_stage_chunk(id, stage, &remaining, 0).is_err() {
            partial = true;
        }
        if store.finalize_stage(id, stage, partial).is_err() {
            partial = true;
        }
    }
    if store
        .finish_request(id, partial, &Value::Object(request.metadata))
        .is_err()
    {
        let _ = store.mark_partial(id);
    }
}

fn write_chunk(
    store: &ConversationArchive,
    id: &str,
    stage: &'static str,
    body: &[u8],
    observed: u64,
    request: &mut Active,
) {
    if request.failed {
        return;
    }
    let safe = request
        .stages
        .entry(stage)
        .or_default()
        .feed(body, &request.secrets, false);
    if store
        .append_stage_chunk(id, stage, &safe, observed)
        .is_err()
    {
        request.failed = true;
        request.flags.partial.store(true, Ordering::Release);
        request.flags.disabled.store(true, Ordering::Release);
    }
}

#[derive(Default)]
struct StageRedactor {
    pending: Vec<u8>,
}

impl StageRedactor {
    fn feed(&mut self, chunk: &[u8], secrets: &[Vec<u8>], final_chunk: bool) -> Vec<u8> {
        let mut input = std::mem::take(&mut self.pending);
        input.extend_from_slice(chunk);
        let mut output = Vec::with_capacity(input.len());
        let mut index = 0;
        while index < input.len() {
            // A shorter key may be the prefix of a longer one. Wait for the
            // next chunk before choosing, otherwise the longer key's suffix
            // could be written in clear text.
            if !final_chunk
                && secrets.iter().any(|secret| {
                    secret.len() > input.len() - index && secret.starts_with(&input[index..])
                })
            {
                break;
            }
            if let Some(secret) = secrets
                .iter()
                .find(|secret| input[index..].starts_with(secret.as_slice()))
            {
                output.extend_from_slice(b"[redacted]");
                index += secret.len();
                continue;
            }
            if !final_chunk
                && secrets
                    .iter()
                    .any(|secret| secret.starts_with(&input[index..]))
            {
                break;
            }
            output.push(input[index]);
            index += 1;
        }
        self.pending.extend_from_slice(&input[index..]);
        output
    }
}

fn insert_secret(secrets: &mut Vec<Vec<u8>>, value: &str) -> bool {
    // Empty authentication values carry no bytes that need masking.
    if value.is_empty() {
        return true;
    }
    let mut accepted = insert_secret_bytes(secrets, value.as_bytes().to_vec());
    if let Some(bearer) = value.strip_prefix("Bearer ") {
        accepted &= insert_secret_bytes(secrets, bearer.as_bytes().to_vec());
    }
    accepted
}
fn insert_secret_bytes(secrets: &mut Vec<Vec<u8>>, value: Vec<u8>) -> bool {
    if value.is_empty() {
        return true;
    }
    if value.len() > MAX_SECRET_BYTES {
        return false;
    }
    if secrets.contains(&value) {
        return true;
    }
    if secrets.len() >= MAX_SECRETS {
        return false;
    }
    secrets.push(value);
    secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
    true
}

fn sanitize_json_fields(body: &[u8]) -> Vec<u8> {
    if let Ok(mut value) = serde_json::from_slice::<Value>(body) {
        if redact_auth_fields(&mut value) {
            return serde_json::to_vec(&value)
                .unwrap_or_else(|_| b"[redaction unavailable]".to_vec());
        }
    }
    body.to_vec()
}

fn redact_auth_fields(value: &mut Value) -> bool {
    match value {
        Value::Object(fields) => {
            let mut changed = false;
            for (key, child) in fields.iter_mut() {
                if matches!(
                    key.to_ascii_lowercase().as_str(),
                    "authorization"
                        | "api_key"
                        | "apikey"
                        | "access_token"
                        | "refresh_token"
                        | "client_secret"
                        | "password"
                        | "cookie"
                        | "instance_key"
                        | "x-api-key"
                ) {
                    *child = Value::String("[redacted]".into());
                    changed = true;
                } else {
                    changed |= redact_auth_fields(child);
                }
            }
            changed
        }
        Value::Array(items) => items
            .iter_mut()
            .fold(false, |changed, item| redact_auth_fields(item) | changed),
        _ => false,
    }
}

fn is_secret_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("authorization")
        || name.contains("api-key")
        || name.contains("api_key")
        || name.contains("cookie")
        || name.contains("token")
        || name.contains("instance")
        || name.contains("rcodex")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redacts_credentials_across_every_split() {
        let key = b"secret-1234".to_vec();
        for split in 1..key.len() {
            let mut state = StageRedactor::default();
            let mut output = state.feed(&key[..split], &[key.clone()], false);
            output.extend(state.feed(&key[split..], &[key.clone()], false));
            output.extend(state.feed(&[], &[key.clone()], true));
            assert_eq!(output, b"[redacted]");
        }
    }
    #[test]
    fn ordinary_prefix_is_released_at_end() {
        let mut state = StageRedactor::default();
        let mut output = state.feed(b"secr", &[b"secret".to_vec()], false);
        output.extend(state.feed(&[], &[b"secret".to_vec()], true));
        assert_eq!(output, b"secr");
    }
    #[test]
    fn structured_credentials_drop_values_without_erasing_unknown_fields() {
        let safe = sanitize_json_fields(br#"{"input":"hello","api_key":"never-record","reasoning":{"encrypted_content":"keep"}}"#);
        let value: Value = serde_json::from_slice(&safe).unwrap();
        assert_eq!(value["api_key"], "[redacted]");
        assert_eq!(value["reasoning"]["encrypted_content"], "keep");
    }

    #[test]
    fn oversized_secret_fails_capture_closed_without_blocking_forwarding() {
        let mut secrets = Vec::new();
        assert!(!insert_secret(
            &mut secrets,
            &"x".repeat(MAX_SECRET_BYTES + 1)
        ));
        assert!(secrets.is_empty());
    }
}
