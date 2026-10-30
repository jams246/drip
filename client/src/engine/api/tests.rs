use super::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

async fn response(body: Vec<u8>) -> Response {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        let _ = socket.read(&mut request).await;
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = socket.write_all(headers.as_bytes()).await;
        let _ = socket.write_all(&body).await;
    });
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("http://{address}/"))
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn response_body_limit_accepts_boundary_and_rejects_excess() {
    let body = read_body(response(vec![b'x'; RESPONSE_LIMIT]).await)
        .await
        .unwrap();
    assert_eq!(body.len(), RESPONSE_LIMIT);
    assert!(matches!(
        read_body(response(vec![b'x'; RESPONSE_LIMIT + 1]).await).await,
        Err(EngineError::ResponseTooLarge)
    ));
}

#[tokio::test]
async fn malformed_json_returns_a_safe_error_without_body_contents() {
    let response = response(b"not-json-private-sentinel".to_vec()).await;
    let error = decode_json_with_status::<serde_json::Value>(response)
        .await
        .unwrap_err();
    assert!(matches!(error, EngineError::ResponseJson(_)));
    assert!(!error.to_string().contains("private-sentinel"));
}

#[test]
fn api_error_messages_are_sanitized_and_bounded() {
    let message = format!("bad\u{202E}{}", "x".repeat(10_000));
    let body = serde_json::to_vec(&drip_core::protocol::ApiError {
        code: drip_core::protocol::ApiErrorCode::InvalidInput,
        message,
        retryable: false,
    })
    .unwrap();
    let EngineError::Api { message, .. } = api_error(StatusCode::BAD_REQUEST, body) else {
        panic!("expected an API error");
    };
    assert!(message.chars().count() <= MESSAGE_LIMIT);
    assert!(!message.contains('\u{202E}'));
    assert!(message.starts_with("badx"));
}
