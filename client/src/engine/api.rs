use futures_util::TryStreamExt;
use reqwest::{Method, Response, StatusCode, header::CONTENT_TYPE};
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    engine::error::EngineError,
    transport::{HttpTransport, TransportError},
};

const CONTROL_LIMIT: usize = 256 * 1024;
const RESPONSE_LIMIT: usize = 4 * 1024 * 1024;

pub(super) async fn json<T, B>(
    transport: &HttpTransport,
    method: Method,
    path: &str,
    credential: Option<&str>,
    query: &[(&str, &str)],
    body: Option<&B>,
) -> Result<T, EngineError>
where
    T: DeserializeOwned,
    B: Serialize + ?Sized,
{
    json_with_status(transport, method, path, credential, query, body)
        .await
        .map(|(value, _)| value)
}

pub(super) async fn json_with_status<T, B>(
    transport: &HttpTransport,
    method: Method,
    path: &str,
    credential: Option<&str>,
    query: &[(&str, &str)],
    body: Option<&B>,
) -> Result<(T, StatusCode), EngineError>
where
    T: DeserializeOwned,
    B: Serialize + ?Sized,
{
    let mut request = if query.is_empty() {
        transport.request(method, path)?
    } else {
        transport.request_with_query(method, path, query)?
    };
    if let Some(credential) = credential {
        request = request.bearer_auth(credential);
    }
    if let Some(body) = body {
        let encoded = serde_json::to_vec(body)?;
        if encoded.len() > CONTROL_LIMIT {
            return Err(EngineError::InvalidResponse(
                "client control message exceeds 256 KiB",
            ));
        }
        request = request
            .header(CONTENT_TYPE, "application/json")
            .body(encoded);
    }
    let request = request.build().map_err(TransportError::Build)?;
    let response = transport.send(request).await?;
    decode_json_with_status(response).await
}

pub(super) async fn status(
    transport: &HttpTransport,
    method: Method,
    path: &str,
    credential: &str,
) -> Result<(), EngineError> {
    let request = transport
        .request(method, path)?
        .bearer_auth(credential)
        .build()
        .map_err(TransportError::Build)?;
    ensure_success(transport.send(request).await?).await
}

pub(super) async fn ensure_success(response: Response) -> Result<(), EngineError> {
    let status = response.status();
    let body = read_body(response).await?;
    if status.is_success() {
        return Ok(());
    }
    Err(api_error(status, body))
}

async fn decode_json_with_status<T: DeserializeOwned>(
    response: Response,
) -> Result<(T, StatusCode), EngineError> {
    let status = response.status();
    let body = read_body(response).await?;
    if !status.is_success() {
        return Err(api_error(status, body));
    }
    serde_json::from_slice(&body)
        .map(|value| (value, status))
        .map_err(EngineError::ResponseJson)
}

async fn read_body(response: Response) -> Result<Vec<u8>, EngineError> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream
        .try_next()
        .await
        .map_err(|source| EngineError::Transport(TransportError::Request(source)))?
    {
        if body.len().saturating_add(chunk.len()) > RESPONSE_LIMIT {
            return Err(EngineError::ResponseTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn api_error(status: StatusCode, body: Vec<u8>) -> EngineError {
    let Ok(error) = serde_json::from_slice::<drip_core::protocol::ApiError>(&body) else {
        return EngineError::UnexpectedStatus(status);
    };
    EngineError::Api {
        status,
        code: error.code,
        message: error.message,
        retryable: error.retryable,
    }
}

#[cfg(test)]
mod tests;
