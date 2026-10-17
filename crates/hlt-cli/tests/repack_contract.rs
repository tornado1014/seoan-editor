mod support;

use std::{fs, process::Command};

#[test]
fn repack_emits_one_bounded_schema_valid_line_and_creates_only_the_output() {
    let source = support::pct_table_fixture();
    let output_dir = tempfile::tempdir().unwrap();
    let output_path = output_dir.path().join("PCT_cli_repacked.HLT");
    let output = Command::new(env!("CARGO_BIN_EXE_hlt-cli"))
        .args([
            "repack-as-new",
            source.path().to_str().unwrap(),
            output_path.to_str().unwrap(),
        ])
        .current_dir(support::repo_root())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, b"");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    let value: serde_json::Value = serde_json::from_str(stdout.trim_end()).unwrap();
    support::validate_repack_envelope(&value);
    assert_eq!(value["command"], "repack-as-new");
    assert_eq!(value["ok"], true);
    assert_eq!(
        value["result"]["input_source"],
        source.path().file_name().unwrap().to_str().unwrap()
    );
    assert_eq!(value["result"]["output_source"], "PCT_cli_repacked.HLT");
    assert_eq!(value["result"]["structural_parity"]["inflated_equal"], true);
    assert_eq!(value["result"]["verification"]["window_ok"], true);
    assert!(output_path.exists());
    assert_no_forbidden_content(&value, &stdout);
}

#[test]
fn repack_refuses_same_or_existing_outputs_with_exit_two() {
    let source = support::pct_table_fixture();
    let existing_dir = tempfile::tempdir().unwrap();
    let existing = existing_dir.path().join("existing.HLT");
    fs::write(&existing, b"protected").unwrap();

    for (output, expected_code) in [
        (source.path(), "output_same_as_input"),
        (existing.as_path(), "output_exists"),
    ] {
        let command = Command::new(env!("CARGO_BIN_EXE_hlt-cli"))
            .args([
                "repack-as-new",
                source.path().to_str().unwrap(),
                output.to_str().unwrap(),
            ])
            .current_dir(support::repo_root())
            .output()
            .unwrap();
        assert_eq!(command.status.code(), Some(2));
        assert_eq!(command.stderr, b"");
        let value: serde_json::Value = serde_json::from_slice(&command.stdout).unwrap();
        support::validate_error_envelope(&value);
        assert_eq!(value["command"], "repack-as-new");
        assert_eq!(value["error"]["code"], expected_code);
    }
    assert_eq!(fs::read(existing).unwrap(), b"protected");
}

#[test]
fn repack_rejects_missing_or_extra_arguments_without_creating_output() {
    let source = support::pct_table_fixture();
    let output_dir = tempfile::tempdir().unwrap();
    let output_path = output_dir.path().join("must-not-exist.HLT");
    for arguments in [
        vec!["repack-as-new", source.path().to_str().unwrap()],
        vec![
            "repack-as-new",
            source.path().to_str().unwrap(),
            output_path.to_str().unwrap(),
            "extra",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_hlt-cli"))
            .args(arguments)
            .current_dir(support::repo_root())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(64));
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        support::validate_error_envelope(&value);
        assert_eq!(value["command"], "repack-as-new");
    }
    assert!(!output_path.exists());
}

fn assert_no_forbidden_content(value: &serde_json::Value, serialized: &str) {
    fn visit(value: &serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, child) in map {
                    assert!(!["body", "raw", "image_bytes", "base64"].contains(&key.as_str()));
                    visit(child);
                }
            }
            serde_json::Value::Array(values) => {
                for child in values {
                    visit(child);
                }
            }
            _ => {}
        }
    }
    visit(value);
    assert!(!serialized.contains(support::repo_root().to_string_lossy().as_ref()));
}
