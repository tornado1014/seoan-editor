mod support;

use std::{fs, path::Path, process::Command};

use hlt_core::{VerifyOptions, document_revision, verify_path};

const SOURCE_TEXT: &str = "재질";
const SOURCE_TEXT_SHA256: &str = "E2A2344DFE2BB3055B60C22387345B7572F10670C1202BDD09E98C3A196524AE";

#[test]
fn inspect_cell_formats_emits_one_bounded_schema_valid_line_without_cell_text() {
    let source = support::pct_table_fixture();
    let output = cli(&[
        "inspect-cell-formats",
        source.path().to_str().unwrap(),
        "--cursor",
        "0",
        "--limit",
        "2",
    ]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, b"");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    let value: serde_json::Value = serde_json::from_str(stdout.trim_end()).unwrap();
    validate_result_schema(&value, "cell-format-inventory-v1.schema.json");
    assert_eq!(value["command"], "inspect-cell-formats");
    assert_eq!(value["result"]["cell_count"], "16");
    assert_eq!(value["result"]["limit"], 2);
    assert_eq!(value["result"]["items"].as_array().unwrap().len(), 2);
    assert_sha_and_filename_only(
        &value["result"]["source_sha256"],
        &value["result"]["source"],
    );
    assert_no_forbidden_content(&value, &stdout);
}

#[test]
fn cell_format_plan_and_save_emit_strict_results_and_publish_only_a_new_file() {
    let source = support::pct_table_fixture();
    let directory = tempfile::tempdir().unwrap();
    let operations = write_operations(source.path(), directory.path());

    let plan_output = cli(&[
        "cell-format-plan",
        source.path().to_str().unwrap(),
        "--operations",
        operations.to_str().unwrap(),
    ]);
    assert_eq!(plan_output.status.code(), Some(0));
    assert_eq!(plan_output.stderr, b"");
    let stdout = String::from_utf8(plan_output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    let value: serde_json::Value = serde_json::from_str(stdout.trim_end()).unwrap();
    validate_result_schema(&value, "cell-format-edit-plan-v1.schema.json");
    assert_eq!(value["command"], "cell-format-plan");
    assert_eq!(value["result"]["ready"], 1);
    assert_eq!(value["result"]["skipped"], 0);
    assert_no_forbidden_content(&value, &stdout);

    let output_path = directory.path().join("pct-cell-cli-save.HLT");
    let save_output = cli(&[
        "save-cell-formats-as-new",
        source.path().to_str().unwrap(),
        output_path.to_str().unwrap(),
        "--operations",
        operations.to_str().unwrap(),
    ]);
    assert_eq!(save_output.status.code(), Some(0));
    assert_eq!(save_output.stderr, b"");
    let stdout = String::from_utf8(save_output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    let value: serde_json::Value = serde_json::from_str(stdout.trim_end()).unwrap();
    validate_result_schema(&value, "cell-format-edit-save-v1.schema.json");
    assert_eq!(value["command"], "save-cell-formats-as-new");
    assert_eq!(value["result"]["output_source"], "pct-cell-cli-save.HLT");
    assert_ne!(
        value["result"]["base_revision"],
        value["result"]["next_revision"]
    );
    for key in [
        "header_equal",
        "body_size_delta_equal",
        "counts_equal",
        "fixed_blocks_equal",
        "target_formats_equal",
        "non_target_formats_equal",
        "cell_text_equal",
        "paragraph_text_equal",
        "paragraph_shapes_equal",
        "tables_equal",
        "images_equal",
        "wipo_equal",
        "char_spans_valid",
        "body_changed",
    ] {
        assert_eq!(value["result"]["structure"][key], true, "{key}");
    }
    assert!(output_path.exists());
    assert_no_forbidden_content(&value, &stdout);
}

#[test]
fn cell_format_commands_reject_usage_and_loader_failures_without_creating_output() {
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
        vec!["inspect-cell-formats"],
        vec!["cell-format-plan", source.path().to_str().unwrap()],
        vec![
            "save-cell-formats-as-new",
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
            "cell-format-plan",
            source.path().to_str().unwrap(),
            "--operations",
            malformed.to_str().unwrap(),
        ],
        vec![
            "save-cell-formats-as-new",
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
    let path = directory.join("cell-format-operations.json");
    let value = serde_json::json!({
        "expected_revision": revision.to_string(),
        "operations": [{
            "cell_id": "cell:0:1:0",
            "expected_text_sha256": SOURCE_TEXT_SHA256,
            "runs": [{"start": 0, "end": 2, "formats": ["bold"]}]
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

fn assert_sha_and_filename_only(sha: &serde_json::Value, source: &serde_json::Value) {
    let sha = sha.as_str().unwrap();
    assert_eq!(sha.len(), 64);
    assert!(
        sha.bytes()
            .all(|value| value.is_ascii_digit() || (b'A'..=b'F').contains(&value))
    );
    let source = source.as_str().unwrap();
    assert!(!source.contains('/'));
    assert!(!source.contains('\\'));
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
                            "end_byte",
                            "text"
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
    assert!(!serialized.contains(support::repo_root().to_string_lossy().as_ref()));
}
