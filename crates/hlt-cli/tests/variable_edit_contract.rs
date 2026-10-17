mod support;

use std::{fs, path::Path, process::Command};

use hlt_core::{VerifyOptions, document_revision, verify_path};

const SOURCE_TEXT: &str = "구성품 시험 결과";
const SOURCE_TEXT_SHA256: &str = "393C92B1A0DBD493E4AF38AA24EE339BC7968547D21440F27B7C356596D74662";
const REPLACEMENT_TEXT: &str = "구성품 심층 분석 결과";

#[test]
fn variable_plan_emits_one_bounded_schema_valid_line_without_text_or_offsets() {
    let source = support::pct_table_fixture();
    let directory = tempfile::tempdir().unwrap();
    let operations = write_operations(source.path(), directory.path());

    let output = cli(&[
        "edit-plan-variable",
        source.path().to_str().unwrap(),
        "--operations",
        operations.to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, b"");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    let value: serde_json::Value = serde_json::from_str(stdout.trim_end()).unwrap();
    validate_result_schema(&value, "edit-plan-v1.schema.json");
    assert_eq!(value["command"], "edit-plan-variable");
    assert_eq!(value["ok"], true);
    assert_eq!(value["result"]["ready"], 1);
    assert_eq!(value["result"]["skipped"], 0);
    assert_no_forbidden_content(&value, &stdout);
}

#[test]
fn variable_save_grows_the_fixture_and_returns_verified_structure_only() {
    let source = support::pct_table_fixture();
    let directory = tempfile::tempdir().unwrap();
    let operations = write_operations(source.path(), directory.path());
    let output_path = directory.path().join("PCT_cli_variable_edit.HLT");

    let output = cli(&[
        "save-variable-as-new",
        source.path().to_str().unwrap(),
        output_path.to_str().unwrap(),
        "--operations",
        operations.to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, b"");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    let value: serde_json::Value = serde_json::from_str(stdout.trim_end()).unwrap();
    validate_result_schema(&value, "variable-edit-save-v1.schema.json");
    assert_eq!(value["command"], "save-variable-as-new");
    assert_eq!(value["ok"], true);
    assert_eq!(
        value["result"]["output_source"],
        "PCT_cli_variable_edit.HLT"
    );
    assert_eq!(value["result"]["expected_inflated_delta"], "89");
    assert_ne!(
        value["result"]["base_revision"],
        value["result"]["next_revision"]
    );
    for key in [
        "header_equal",
        "inflated_delta_equal",
        "counts_equal",
        "fixed_blocks_equal",
        "target_paragraphs_equal",
        "non_target_semantics_equal",
        "tables_equal",
        "images_equal",
        "wipo_equal",
        "char_spans_valid",
        "body_changed",
    ] {
        assert_eq!(value["result"]["structure"][key], true, "{key}");
    }
    assert_eq!(value["result"]["verification"]["window_ok"], true);
    assert!(output_path.exists());
    assert_no_forbidden_content(&value, &stdout);
}

#[test]
fn variable_commands_reject_usage_and_loader_failures_without_output() {
    let source = support::pct_table_fixture();
    let directory = tempfile::tempdir().unwrap();
    let output_path = directory.path().join("must-not-exist.HLT");
    let malformed = directory.path().join("malformed.json");
    fs::write(
        &malformed,
        br#"{"expected_revision":"0","operations":[],"extra":true}"#,
    )
    .unwrap();

    for arguments in [
        vec!["edit-plan-variable", source.path().to_str().unwrap()],
        vec![
            "save-variable-as-new",
            source.path().to_str().unwrap(),
            output_path.to_str().unwrap(),
        ],
    ] {
        let output = cli(&arguments);
        assert_eq!(output.status.code(), Some(64));
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        support::validate_error_envelope(&value);
    }

    for arguments in [
        vec![
            "edit-plan-variable",
            source.path().to_str().unwrap(),
            "--operations",
            malformed.to_str().unwrap(),
        ],
        vec![
            "save-variable-as-new",
            source.path().to_str().unwrap(),
            output_path.to_str().unwrap(),
            "--operations",
            malformed.to_str().unwrap(),
        ],
    ] {
        let output = cli(&arguments);
        assert_eq!(output.status.code(), Some(2));
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        support::validate_error_envelope(&value);
        assert_eq!(value["error"]["phase"], "operations");
    }
    assert!(!output_path.exists());
}

fn cli(arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_hlt-cli"))
        .args(arguments)
        .current_dir(support::repo_root())
        .output()
        .unwrap()
}

fn write_operations(source: &Path, directory: &Path) -> std::path::PathBuf {
    let verified = verify_path(source, VerifyOptions::default()).unwrap();
    let revision = document_revision(&verified.input_sha256).unwrap();
    let path = directory.join("variable-operations.json");
    let value = serde_json::json!({
        "expected_revision": revision.to_string(),
        "operations": [{
            "paragraph_id": "paragraph:3",
            "expected_text_sha256": SOURCE_TEXT_SHA256,
            "replacement": REPLACEMENT_TEXT
        }]
    });
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    path
}

fn validate_result_schema(value: &serde_json::Value, result_schema_name: &str) {
    support::validate_error_envelope(value);
    let schema: serde_json::Value = serde_json::from_slice(
        &fs::read(
            support::repo_root()
                .join("contracts")
                .join(result_schema_name),
        )
        .unwrap(),
    )
    .unwrap();
    let validator = jsonschema::JSONSchema::compile(&schema).unwrap();
    if let Err(errors) = validator.validate(&value["result"]) {
        let messages: Vec<_> = errors.map(|error| error.to_string()).collect();
        panic!("schema validation failed: {}", messages.join("; "));
    }
}

fn assert_no_forbidden_content(value: &serde_json::Value, serialized: &str) {
    fn visit(value: &serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, child) in map {
                    assert!(
                        ![
                            "body",
                            "raw",
                            "image_bytes",
                            "base64",
                            "replacement",
                            "start_byte",
                            "end_byte"
                        ]
                        .contains(&key.as_str())
                    );
                    visit(child);
                }
            }
            serde_json::Value::Array(values) => values.iter().for_each(visit),
            _ => {}
        }
    }
    visit(value);
    assert!(!serialized.contains(SOURCE_TEXT));
    assert!(!serialized.contains(REPLACEMENT_TEXT));
    assert!(!serialized.contains(support::repo_root().to_string_lossy().as_ref()));
}
