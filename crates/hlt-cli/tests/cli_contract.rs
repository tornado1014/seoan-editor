mod support;

use std::process::Command;

#[test]
fn a_reference_beyond_the_window_is_one_error_envelope_with_exit_two() {
    let fixture = support::write_temp_container_with_far_backreference();
    let output = Command::new(env!("CARGO_BIN_EXE_hlt-cli"))
        .args(["verify", fixture.path().to_str().unwrap()])
        .current_dir(support::repo_root())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output.stderr, b"");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    let value: serde_json::Value = serde_json::from_str(stdout.trim_end()).unwrap();
    support::validate_error_envelope(&value);
    assert_eq!(value["ok"], false);
    assert_eq!(value["result"], serde_json::Value::Null);
    assert_eq!(value["error"]["code"], "window_not_proven");
    assert_eq!(
        value["error"]["source"],
        fixture.path().file_name().unwrap().to_str().unwrap()
    );
    assert!(!stdout.contains(fixture.path().parent().unwrap().to_str().unwrap()));
}

#[test]
fn incorrect_arguments_return_one_usage_envelope_and_exit_64() {
    let output = Command::new(env!("CARGO_BIN_EXE_hlt-cli"))
        .current_dir(support::repo_root())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(64));
    assert_eq!(output.stderr, b"");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    let value: serde_json::Value = serde_json::from_str(stdout.trim_end()).unwrap();
    support::validate_error_envelope(&value);
    assert_eq!(value["error"]["code"], "verification_failed");
}

#[test]
fn human_mode_keeps_json_on_stdout_and_writes_prose_only_to_stderr() {
    let fixture = support::write_temp_container_with_far_backreference();
    let output = Command::new(env!("CARGO_BIN_EXE_hlt-cli"))
        .args(["verify", fixture.path().to_str().unwrap(), "--human"])
        .current_dir(support::repo_root())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    assert!(serde_json::from_str::<serde_json::Value>(stdout.trim_end()).is_ok());
    assert!(!stderr.trim().is_empty());
    assert!(serde_json::from_str::<serde_json::Value>(stderr.trim()).is_err());
}

#[test]
fn command_schema_rejects_an_unknown_error_code() {
    let fixture = support::write_temp_container_with_far_backreference();
    let output = Command::new(env!("CARGO_BIN_EXE_hlt-cli"))
        .args(["verify", fixture.path().to_str().unwrap()])
        .current_dir(support::repo_root())
        .output()
        .unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    value["error"]["code"] = serde_json::Value::String("unknown_error".to_string());

    assert!(!support::command_schema_accepts(&value));
}
