mod support;

use std::{fs, path::Path, process::Command};

use hlt_core::{VerifyOptions, document_revision, verify_path};

const SOURCE_TEXT: &str = "구성품 시험 결과";
const SOURCE_TEXT_SHA256: &str = "393C92B1A0DBD493E4AF38AA24EE339BC7968547D21440F27B7C356596D74662";
const REPLACEMENT_TEXT: &str = "구성품 분석 결과";

#[test]
fn edit_plan_emits_one_bounded_schema_valid_line_without_text_or_offsets() {
    let source = support::pct_table_fixture();
    let directory = tempfile::tempdir().unwrap();
    let operations = write_operations(source.path(), directory.path());

    let output = cli(&[
        "edit-plan",
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
    assert_eq!(value["command"], "edit-plan");
    assert_eq!(value["ok"], true);
    assert_eq!(value["result"]["operation_count"], 1);
    assert_eq!(value["result"]["ready"], 1);
    assert_eq!(value["result"]["skipped"], 0);
    assert_eq!(value["result"]["decisions"][0]["status"], "ready");
    assert_no_forbidden_content(&value, &stdout);
}

#[test]
fn save_as_new_edits_the_fixture_and_returns_verified_structure_only() {
    let source = support::pct_table_fixture();
    let directory = tempfile::tempdir().unwrap();
    let operations = write_operations(source.path(), directory.path());
    let output_path = directory.path().join("PCT_cli_exact_edit.HLT");

    let output = cli(&[
        "save-as-new",
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
    validate_result_schema(&value, "edit-save-v1.schema.json");
    assert_eq!(value["command"], "save-as-new");
    assert_eq!(value["ok"], true);
    assert_eq!(value["result"]["output_source"], "PCT_cli_exact_edit.HLT");
    assert_ne!(
        value["result"]["base_revision"],
        value["result"]["next_revision"]
    );
    assert_eq!(value["result"]["structure"]["body_changed"], true);
    assert_eq!(
        value["result"]["structure"]["non_target_paragraphs_equal"],
        true
    );
    assert_eq!(value["result"]["verification"]["window_ok"], true);
    assert!(output_path.exists());
    assert_no_forbidden_content(&value, &stdout);
}

#[test]
fn operations_file_is_bounded_and_unknown_fields_are_rejected() {
    let source = support::pct_table_fixture();
    let directory = tempfile::tempdir().unwrap();
    let too_large = directory.path().join("too-large.json");
    fs::write(&too_large, vec![b' '; 5 * 1024 * 1024 + 1]).unwrap();
    let unknown = directory.path().join("unknown.json");
    fs::write(
        &unknown,
        br#"{"expected_revision":"0","operations":[],"unexpected":true}"#,
    )
    .unwrap();
    let too_many = directory.path().join("too-many.json");
    let one = serde_json::json!({
        "paragraph_id": "paragraph:3",
        "expected_text_sha256": SOURCE_TEXT_SHA256,
        "replacement": "A"
    });
    fs::write(
        &too_many,
        serde_json::to_vec(&serde_json::json!({
            "expected_revision": "0",
            "operations": vec![one; 501]
        }))
        .unwrap(),
    )
    .unwrap();
    let replacement_too_large = directory.path().join("replacement-too-large.json");
    fs::write(
        &replacement_too_large,
        serde_json::to_vec(&serde_json::json!({
            "expected_revision": "0",
            "operations": [{
                "paragraph_id": "paragraph:3",
                "expected_text_sha256": SOURCE_TEXT_SHA256,
                "replacement": "A".repeat(4 * 1024 * 1024 + 1)
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    for (operations, expected_code) in [
        (&too_large, "resource_too_large"),
        (&unknown, "conversion_failed"),
        (&too_many, "resource_too_large"),
        (&replacement_too_large, "resource_too_large"),
    ] {
        let output = cli(&[
            "edit-plan",
            source.path().to_str().unwrap(),
            "--operations",
            operations.to_str().unwrap(),
        ]);
        assert_eq!(output.status.code(), Some(2));
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        support::validate_error_envelope(&value);
        assert_eq!(value["command"], "edit-plan");
        assert_eq!(value["error"]["code"], expected_code);
        assert_eq!(value["error"]["phase"], "operations");
    }
}

#[test]
fn edit_commands_reject_malformed_arguments_without_creating_output() {
    let source = support::pct_table_fixture();
    let directory = tempfile::tempdir().unwrap();
    let operations = write_operations(source.path(), directory.path());
    let output_path = directory.path().join("must-not-exist.HLT");
    let cases = [
        vec!["edit-plan", source.path().to_str().unwrap()],
        vec![
            "save-as-new",
            source.path().to_str().unwrap(),
            output_path.to_str().unwrap(),
        ],
        vec![
            "save-as-new",
            source.path().to_str().unwrap(),
            output_path.to_str().unwrap(),
            "--operations",
            operations.to_str().unwrap(),
            "extra",
        ],
    ];

    for arguments in cases {
        let output = cli(&arguments);
        assert_eq!(output.status.code(), Some(64));
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        support::validate_error_envelope(&value);
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
    let path = directory.join("operations.json");
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
