use reqwest::{Client, Method, Request, RequestBuilder, Response, StatusCode, redirect::Policy};
use thiserror::Error;
use url::Url;

/// Canonical HTTPS origin for the paired Drip server.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerUrl(Url);

impl ServerUrl {
    /// Parses an HTTPS origin and returns its normalized representation.
    pub fn parse(value: &str) -> Result<Self, ServerUrlError> {
        let url = Url::parse(value).map_err(ServerUrlError::Parse)?;
        if url.scheme() != "https" {
            return Err(ServerUrlError::HttpsRequired);
        }
        if url.host_str().is_none() {
            return Err(ServerUrlError::HostRequired);
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(ServerUrlError::UserInfoForbidden);
        }
        if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
            return Err(ServerUrlError::OriginRequired);
        }
        Ok(Self(url))
    }

    /// Returns the canonical HTTPS origin, including its root slash.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Resolves a relative API path without allowing authority or path escape.
    pub fn endpoint(&self, path: &str) -> Result<Url, ServerUrlError> {
        if path.is_empty()
            || path.starts_with('/')
            || path.contains(['\\', '?', '#', ':'])
            || path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(ServerUrlError::InvalidEndpoint);
        }
        self.0.join(path).map_err(ServerUrlError::Parse)
    }
}

/// Reqwest client pinned to validated HTTPS with redirects disabled.
#[derive(Clone)]
pub struct HttpTransport {
    server: ServerUrl,
    client: Client,
}

impl HttpTransport {
    /// Builds a client using the platform's default certificate validation.
    pub fn new(server_url: &str) -> Result<Self, TransportError> {
        let server = ServerUrl::parse(server_url)?;
        let client = Client::builder()
            .tls_backend_native()
            .https_only(true)
            .redirect(Policy::none())
            .connect_timeout(std::time::Duration::from_secs(10))
            .read_timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(TransportError::Client)?;
        Ok(Self { server, client })
    }

    /// Returns the validated server origin.
    pub fn server(&self) -> &ServerUrl {
        &self.server
    }

    /// Builds a request against a validated relative API endpoint.
    pub fn request(&self, method: Method, path: &str) -> Result<RequestBuilder, TransportError> {
        let endpoint = self.server.endpoint(path)?;
        Ok(self.client.request(method, endpoint))
    }

    /// Builds a request using safely encoded query pairs.
    pub fn request_with_query(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<RequestBuilder, TransportError> {
        let mut endpoint = self.server.endpoint(path)?;
        endpoint
            .query_pairs_mut()
            .extend_pairs(query.iter().copied());
        Ok(self.client.request(method, endpoint))
    }

    /// Sends a request only to the paired origin and rejects redirect responses.
    pub async fn send(&self, request: Request) -> Result<Response, TransportError> {
        if request.url().origin() != self.server.0.origin() {
            return Err(TransportError::WrongOrigin);
        }
        let response = self
            .client
            .execute(request)
            .await
            .map_err(TransportError::Request)?;
        if response.status().is_redirection() {
            return Err(TransportError::Redirect(response.status()));
        }
        Ok(response)
    }
}

/// Invalid or non-canonical server URL.
#[derive(Debug, Error)]
pub enum ServerUrlError {
    #[error("invalid server URL: {0}")]
    Parse(#[source] url::ParseError),
    #[error("server URL must use HTTPS")]
    HttpsRequired,
    #[error("server URL must include a host")]
    HostRequired,
    #[error("server URL must not contain username or password")]
    UserInfoForbidden,
    #[error("server URL must be an origin without path, query, or fragment")]
    OriginRequired,
    #[error("invalid API endpoint path")]
    InvalidEndpoint,
}

/// Transport construction failure.
#[derive(Debug, Error)]
pub enum TransportError {
    #[error(transparent)]
    ServerUrl(#[from] ServerUrlError),
    #[error("could not build HTTPS client: {0}")]
    Client(#[source] reqwest::Error),
    #[error("could not build HTTPS request: {0}")]
    Build(#[source] reqwest::Error),
    #[error("request failed: {0}")]
    Request(#[source] reqwest::Error),
    #[error("server returned redirect status {0}")]
    Redirect(StatusCode),
    #[error("request URL does not match paired server origin")]
    WrongOrigin,
}

#[cfg(test)]
mod tests {
    use super::{HttpTransport, ServerUrl, ServerUrlError};

    #[test]
    fn server_url_should_normalize_https_origin() {
        let server = ServerUrl::parse("HTTPS://Example.COM:443").unwrap();

        assert_eq!(server.as_str(), "https://example.com/");
    }

    #[test]
    fn server_url_should_reject_non_origin_or_non_https_values() {
        for value in [
            "http://example.com",
            "https://user@example.com",
            "https://example.com/path",
            "https://example.com?key=value",
            "https://example.com/#fragment",
        ] {
            assert!(ServerUrl::parse(value).is_err(), "accepted {value}");
        }
    }

    #[test]
    fn endpoint_should_reject_authority_and_path_escape() {
        let server = ServerUrl::parse("https://example.com").unwrap();

        for path in [
            "",
            "/v1/enroll",
            "//other.test/v1/enroll",
            "../x",
            "v1/../x",
        ] {
            assert!(matches!(
                server.endpoint(path),
                Err(ServerUrlError::InvalidEndpoint)
            ));
        }
        assert_eq!(
            server.endpoint("v1/enroll").unwrap().as_str(),
            "https://example.com/v1/enroll"
        );
    }

    #[test]
    fn transport_should_build_for_validated_https_origin() {
        let transport = HttpTransport::new("https://example.com").unwrap();

        assert_eq!(transport.server().as_str(), "https://example.com/");
    }
}
