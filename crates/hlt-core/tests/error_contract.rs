use hlt_core::{CoreError, ErrorCode};

#[test]
fn error_code_serializes_as_stable_machine_name() {
    let error = CoreError::new(ErrorCode::WindowNotProven, "window", false);
    let json = serde_json::to_value(error).unwrap();

    assert_eq!(json["code"], "window_not_proven");
    assert_eq!(json["phase"], "window");
    assert_eq!(json["source"], serde_json::Value::Null);
    assert_eq!(json["offset"], serde_json::Value::Null);
}

#[test]
fn u64_error_offset_serializes_as_a_decimal_string() {
    let mut error = CoreError::new(ErrorCode::DeflateInvalid, "inflate", false);
    error.offset = Some(u64::MAX);

    let json = serde_json::to_value(error).unwrap();

    assert_eq!(json["offset"], "18446744073709551615");
}
