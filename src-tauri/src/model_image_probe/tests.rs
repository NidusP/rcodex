use super::*;
use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{Request, StatusCode},
    response::Response as AxumResponse,
    Router,
};
use tokio::{net::TcpListener, sync::mpsc, task::JoinHandle};

struct Captured {
    path: String,
    authorization: String,
    body: Value,
}

#[derive(Clone)]
struct Mock {
    status: u16,
    response: String,
    delay: Duration,
    requests: mpsc::UnboundedSender<Captured>,
}

async fn handle(State(mock): State<Mock>, request: Request<Body>) -> AxumResponse {
    let path = request.uri().path().to_string();
    let authorization = request
        .headers()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let bytes = to_bytes(request.into_body(), 64 * 1024).await.unwrap();
    let _ = mock.requests.send(Captured {
        path,
        authorization,
        body: serde_json::from_slice(&bytes).unwrap(),
    });
    tokio::time::sleep(mock.delay).await;
    AxumResponse::builder()
        .status(StatusCode::from_u16(mock.status).unwrap())
        .header("content-type", "application/json")
        .header("location", "/must-not-follow")
        .body(Body::from(mock.response))
        .unwrap()
}

async fn mock(
    status: u16,
    response: String,
    delay: Duration,
) -> (String, mpsc::UnboundedReceiver<Captured>, JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let base = format!(
        "http://{}/confirmed/prefix/v1",
        listener.local_addr().unwrap()
    );
    let (requests, receiver) = mpsc::unbounded_channel();
    let app = Router::new().fallback(handle).with_state(Mock {
        status,
        response,
        delay,
        requests,
    });
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (base, receiver, task)
}

fn responses(text: &str) -> Value {
    json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":text}]}]})
}

fn client(timeout: Duration) -> Client {
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .unwrap()
}

#[tokio::test]
async fn responses_uses_confirmed_path_and_real_image_content() {
    let (base, mut requests, task) =
        mock(200, responses("blue,yellow").to_string(), Duration::ZERO).await;
    let result = execute_with_client(
        &client(PROBE_TIMEOUT),
        &base,
        "fixture-key",
        "vision-model",
        "responses",
    )
    .await;
    assert_eq!(result.state, "supported");
    let request = requests.recv().await.unwrap();
    assert_eq!(request.path, "/confirmed/prefix/v1/responses");
    assert_eq!(request.authorization, "Bearer fixture-key");
    assert_eq!(request.body["model"], "vision-model");
    assert_eq!(request.body["stream"], false);
    assert_eq!(request.body["max_output_tokens"], MAX_OUTPUT_TOKENS);
    let image = request.body["input"][0]["content"][1]["image_url"]
        .as_str()
        .unwrap();
    assert_eq!(
        STANDARD
            .decode(image.strip_prefix("data:image/png;base64,").unwrap())
            .unwrap(),
        FIXTURE
    );
    assert!(!request.body["input"][0]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("blue"));
    assert!(requests.try_recv().is_err());
    task.abort();
}

#[tokio::test]
async fn chat_uses_its_own_image_format() {
    let reply = json!({"choices":[{"finish_reason":"stop","message":{"content":"Blue, yellow."}}]});
    let (base, mut requests, task) = mock(200, reply.to_string(), Duration::ZERO).await;
    let result = execute_with_client(
        &client(PROBE_TIMEOUT),
        &base,
        "fixture-key",
        "chat-vision",
        "chat_completions",
    )
    .await;
    assert_eq!(result.state, "supported");
    let request = requests.recv().await.unwrap();
    assert_eq!(request.path, "/confirmed/prefix/v1/chat/completions");
    assert_eq!(
        request.body["messages"][0]["content"][1]["type"],
        "image_url"
    );
    assert!(
        request.body["messages"][0]["content"][1]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,")
    );
    assert!(request.body.get("input").is_none());
    assert!(requests.try_recv().is_err());
    task.abort();
}

#[test]
fn semantic_answer_is_required_and_reasoning_does_not_count() {
    for text in ["OK", "yellow,blue", "I can see an image", "red,square", ""] {
        let result = classify_response(
            200,
            &serde_json::to_vec(&responses(text)).unwrap(),
            "responses",
        );
        assert_eq!(result.state, "failed", "{text}");
    }
    let reasoning =
        json!({"output":[{"type":"reasoning","content":[{"type":"text","text":"blue,yellow"}]}]});
    assert_eq!(
        classify_response(200, &serde_json::to_vec(&reasoning).unwrap(), "responses").state,
        "failed"
    );
    assert_eq!(
        classify_response(200, b"not-json", "responses")
            .error_code
            .as_deref(),
        Some("image_probe_invalid_response")
    );
}

#[test]
fn explicit_unsupported_is_separate_from_transient_and_shape_failures() {
    let explicit = br#"{"error":{"message":"This model does not support images"}}"#;
    assert_eq!(
        classify_response(400, explicit, "responses").state,
        "unsupported"
    );
    assert_eq!(
        classify_response(422, explicit, "chat_completions").state,
        "unsupported"
    );
    for status in [401, 403, 429, 500, 502, 503] {
        assert_eq!(
            classify_response(status, explicit, "responses").state,
            "failed",
            "{status}"
        );
    }
    for message in [
        "invalid image format",
        "invalid request",
        "upstream overloaded",
        "image is too small",
    ] {
        let body = serde_json::to_vec(&json!({"error":{"message":message}})).unwrap();
        assert_eq!(
            classify_response(400, &body, "responses").state,
            "failed",
            "{message}"
        );
    }
    for message in [
        "Image request rejected: model is not a VLM",
        "Image input rejected because this is a text-only model",
        "This model is text only",
    ] {
        let body = serde_json::to_vec(&json!({"error":{"message":message}})).unwrap();
        assert_eq!(
            classify_response(400, &body, "responses").state,
            "unsupported",
            "{message}"
        );
    }
    let text_only_output =
        serde_json::to_vec(&responses("This is a text-only model output description")).unwrap();
    assert_eq!(
        classify_response(200, &text_only_output, "responses").state,
        "failed"
    );
}

#[test]
fn incomplete_output_cannot_establish_support() {
    let mut body = responses("blue,yellow");
    body["status"] = json!("incomplete");
    assert_eq!(
        classify_response(200, &serde_json::to_vec(&body).unwrap(), "responses")
            .error_code
            .as_deref(),
        Some("image_probe_output_incomplete")
    );
    let chat = json!({"choices":[{"finish_reason":"length","message":{"content":"blue,yellow"}}]});
    assert_eq!(
        classify_response(200, &serde_json::to_vec(&chat).unwrap(), "chat_completions").state,
        "failed"
    );
}

#[tokio::test]
async fn http_failure_does_not_retry_or_follow_redirect() {
    for status in [302, 400, 429, 502] {
        let (base, mut requests, task) = mock(status, "{}".to_string(), Duration::ZERO).await;
        let result = execute_with_client(
            &client(PROBE_TIMEOUT),
            &base,
            "fixture-key",
            "model",
            "responses",
        )
        .await;
        assert_eq!(result.state, "failed");
        assert_eq!(result.http_status, Some(status));
        assert!(requests.recv().await.is_some());
        assert!(requests.try_recv().is_err());
        task.abort();
    }
}

#[tokio::test]
async fn timeout_is_failed_and_does_not_claim_unsupported() {
    let (base, mut requests, task) = mock(
        200,
        responses("blue,yellow").to_string(),
        Duration::from_millis(200),
    )
    .await;
    let result = execute_with_client(
        &client(Duration::from_millis(100)),
        &base,
        "fixture-key",
        "model",
        "responses",
    )
    .await;
    assert_eq!(result.state, "failed");
    assert_eq!(result.error_code.as_deref(), Some("image_probe_timeout"));
    assert!(
        tokio::time::timeout(Duration::from_secs(1), requests.recv())
            .await
            .unwrap()
            .is_some()
    );
    assert!(requests.try_recv().is_err());
    task.abort();
}

#[tokio::test]
async fn oversized_response_has_a_bounded_safe_error() {
    let (base, _, task) = mock(
        200,
        "sensitive-upstream-data".repeat(MAX_RESPONSE_BYTES),
        Duration::ZERO,
    )
    .await;
    let result = execute_with_client(
        &client(PROBE_TIMEOUT),
        &base,
        "fixture-key",
        "model",
        "responses",
    )
    .await;
    assert_eq!(
        result.error_code.as_deref(),
        Some("image_probe_response_too_large")
    );
    assert!(!format!("{result:?}").contains("sensitive-upstream-data"));
    task.abort();
}

#[tokio::test]
async fn chunked_response_is_bounded_without_content_length() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new().fallback(|| async {
        let chunks = futures_util::stream::iter(vec![
            Ok::<_, std::io::Error>(vec![b'a'; MAX_RESPONSE_BYTES / 2]),
            Ok(vec![b'b'; MAX_RESPONSE_BYTES / 2 + 1]),
        ]);
        AxumResponse::new(Body::from_stream(chunks))
    });
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let result = execute_with_client(
        &client(PROBE_TIMEOUT),
        &base,
        "fixture-key",
        "model",
        "responses",
    )
    .await;
    assert_eq!(
        result.error_code.as_deref(),
        Some("image_probe_response_too_large")
    );
    task.abort();
}

#[tokio::test]
async fn unknown_protocol_and_invalid_endpoint_send_no_request() {
    let probe_client = client(PROBE_TIMEOUT);
    let result = execute_with_client(
        &probe_client,
        "http://127.0.0.1:1",
        "fixture-key",
        "model",
        "unknown",
    )
    .await;
    assert_eq!(
        result.error_code.as_deref(),
        Some("image_probe_protocol_unconfirmed")
    );
    for base in [
        "file:///tmp/image",
        "https://user:password@example.com/v1",
        "https://example.com/v1?secret=key",
        "https://example.com/v1#part",
    ] {
        assert_eq!(
            execute_with_client(&probe_client, base, "fixture-key", "model", "responses")
                .await
                .error_code
                .as_deref(),
            Some("image_probe_invalid_base_url")
        );
    }
}
