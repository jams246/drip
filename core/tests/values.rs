use drip_core::{Blake3Digest, ProtocolVersion, Timestamp};

#[test]
fn digest_serializes_as_lowercase_hex() {
    let digest = Blake3Digest::new([0xab; 32]);

    assert_eq!(
        serde_json::to_string(&digest).unwrap(),
        format!("\"{}\"", "ab".repeat(32))
    );
}

#[test]
fn digest_rejects_uppercase_hex() {
    assert!(Blake3Digest::from_hex(&"AB".repeat(32)).is_err());
}

#[test]
fn digest_round_trips_all_bytes() {
    let bytes = std::array::from_fn(|index| index as u8);
    let digest = Blake3Digest::new(bytes);

    assert_eq!(Blake3Digest::from_hex(&digest.to_hex()).unwrap(), digest);
}

#[test]
fn timestamp_accepts_last_valid_nanosecond() {
    assert_eq!(
        Timestamp::new(-1, 999_999_999).unwrap().nanos(),
        999_999_999
    );
}

#[test]
fn timestamp_deserialization_rejects_out_of_range_nanoseconds() {
    let timestamp = serde_json::from_str::<Timestamp>(r#"{"unix_seconds":0,"nanos":1000000000}"#);

    assert!(timestamp.is_err());
}

#[test]
fn protocol_version_rejects_unsupported_values() {
    assert!(ProtocolVersion::new(2).validate().is_err());
}
