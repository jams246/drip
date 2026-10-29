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
