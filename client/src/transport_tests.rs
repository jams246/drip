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

#[tokio::test]
async fn transport_rejects_different_origins_before_network_io() {
    let transport = HttpTransport::new("https://example.com").unwrap();
    for url in [
        "https://other.example/v1/enroll",
        "http://example.com/v1/enroll",
        "https://example.com:444/v1/enroll",
    ] {
        let request = reqwest::Request::new(reqwest::Method::POST, url::Url::parse(url).unwrap());
        assert!(matches!(
            transport.send(request).await,
            Err(super::TransportError::WrongOrigin)
        ));
    }
}

#[tokio::test]
#[ignore = "requires the owned HTTPS security fixture and its isolated certificate"]
async fn owned_https_fixture_rejects_redirects() {
    let transport = HttpTransport::new(&fixture_origin("DRIP_TEST_SERVER_URL")).unwrap();
    let request = transport
        .request(reqwest::Method::GET, "v1/security-redirect")
        .unwrap()
        .bearer_auth("synthetic-security-credential")
        .build()
        .unwrap();
    assert!(matches!(
        transport.send(request).await,
        Err(super::TransportError::Redirect(reqwest::StatusCode::FOUND))
    ));
}

#[tokio::test]
#[ignore = "requires the owned HTTPS security fixture with an untrusted certificate"]
async fn owned_https_fixture_rejects_untrusted_certificates() {
    let origin = fixture_origin("DRIP_TEST_UNTRUSTED_SERVER_URL");
    let url = url::Url::parse(&origin).unwrap();
    let socket = tokio::net::TcpStream::connect((
        url.host_str().unwrap(),
        url.port_or_known_default().unwrap(),
    ))
    .await
    .unwrap();
    drop(socket);
    let transport = HttpTransport::new(&origin).unwrap();
    let request = transport
        .request(reqwest::Method::GET, "v1/security-certificate")
        .unwrap()
        .build()
        .unwrap();
    match transport.send(request).await {
        Err(super::TransportError::Request(error)) => assert!(error.is_connect()),
        _ => panic!("the untrusted HTTPS fixture must fail its TLS connection"),
    }
}

fn fixture_origin(variable: &str) -> String {
    let origin = std::env::var(variable).unwrap();
    let url = url::Url::parse(&origin).unwrap();
    assert_eq!(url.scheme(), "https");
    assert!(matches!(url.host_str(), Some("localhost" | "127.0.0.1")));
    assert!(std::env::var_os("DRIP_TEST_ROOT_CERT").is_some());
    origin
}
