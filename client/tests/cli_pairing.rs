use std::process::{Command, Stdio};

#[test]
fn invalid_http_server_url_fails_before_token_prompt() {
    let output = Command::new(env!("CARGO_BIN_EXE_drip"))
        .args(["pair", "--server", "http://server.example"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(stderr.contains("server URL must use HTTPS"));
    assert!(!stderr.contains("Enrollment token:"));
}
