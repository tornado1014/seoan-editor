mod support;

use std::{fs, path::Path, process::Command};

/// 표 구조 편집은 생성 소스를 입력으로 받는다. 여기서는 1행 2열 표가 든 HML 픽스처를 쓴다.
fn fixture() -> std::path::PathBuf {
    support::repo_root().join("crates/hlt-core/tests/fixtures/generate_pct.hml")
}

fn cli(arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_hlt-cli"))
        .args(arguments)
        .current_dir(support::repo_root())
        .output()
        .unwrap()
}

fn write_operations(directory: &Path, body: &str) -> std::path::PathBuf {
    let path = directory.join("table-operations.json");
    fs::write(&path, body).unwrap();
    path
}

fn one_line_json(output: &std::process::Output) -> serde_json::Value {
    assert_eq!(output.stderr, b"");
    let stdout = String::from_utf8(output.stdout.clone()).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    serde_json::from_str(stdout.trim_end()).unwrap()
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

#[test]
fn table_plan_reports_decisions_without_touching_anything() {
    let directory = tempfile::tempdir().unwrap();
    let operations = write_operations(
        directory.path(),
        r#"{"operations":[
            {"kind":"insert_row","table_id":"table:1","index":1},
            {"kind":"insert_column","table_id":"table:1","index":1},
            {"kind":"delete_row","table_id":"table:1","index":0},
            {"kind":"split","table_id":"table:1","row":0,"column":0}
        ]}"#,
    );
    let output = cli(&[
        "table-plan",
        fixture().to_str().unwrap(),
        "--operations",
        operations.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(0));
    let value = one_line_json(&output);
    validate_result_schema(&value, "table-edit-plan-v1.schema.json");
    assert_eq!(value["command"], "table-plan");
    assert_eq!(value["ok"], true);
    assert_eq!(value["result"]["operation_count"], 4);
    assert_eq!(value["result"]["ready"], 3);
    assert_eq!(value["result"]["skipped"], 1);
    assert_eq!(value["result"]["decisions"][3]["reason"], "not_merged");
    // 파일 이름만 새고 경로는 새지 않는다.
    let source = value["result"]["source"].as_str().unwrap();
    assert!(!source.contains('/') && !source.contains('\\'));
}

#[test]
fn save_table_as_new_publishes_only_a_new_file_and_keeps_the_source() {
    let directory = tempfile::tempdir().unwrap();
    let operations = write_operations(
        directory.path(),
        r#"{"operations":[
            {"kind":"insert_row","table_id":"table:1","index":1},
            {"kind":"merge","table_id":"table:1","row":0,"column":0,"row_end":1,"column_end":0}
        ]}"#,
    );
    let output_path = directory.path().join("table-edited.HLT");
    let source_before = fs::read(fixture()).unwrap();

    let output = cli(&[
        "save-table-as-new",
        fixture().to_str().unwrap(),
        output_path.to_str().unwrap(),
        "--dialect",
        "pct-patent",
        "--operations",
        operations.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(0));
    let value = one_line_json(&output);
    validate_result_schema(&value, "table-edit-save-v1.schema.json");
    assert_eq!(value["command"], "save-table-as-new");
    assert_eq!(value["result"]["edit_plan"]["ready"], 2);
    assert_eq!(value["result"]["verification"]["footer_crc_ok"], true);
    assert_eq!(value["result"]["summary"]["table_count"], "1");
    assert!(output_path.exists());
    assert_eq!(fs::read(fixture()).unwrap(), source_before);
}

#[test]
fn save_table_as_new_refuses_an_existing_output() {
    let directory = tempfile::tempdir().unwrap();
    let operations = write_operations(
        directory.path(),
        r#"{"operations":[{"kind":"insert_row","table_id":"table:1","index":1}]}"#,
    );
    let output_path = directory.path().join("occupied.HLT");
    fs::write(&output_path, b"do not overwrite me").unwrap();

    let output = cli(&[
        "save-table-as-new",
        fixture().to_str().unwrap(),
        output_path.to_str().unwrap(),
        "--dialect",
        "pct-patent",
        "--operations",
        operations.to_str().unwrap(),
    ]);
    assert_ne!(output.status.code(), Some(0));
    let value = one_line_json(&output);
    assert_eq!(value["ok"], false);
    assert_eq!(fs::read(&output_path).unwrap(), b"do not overwrite me");
}

#[test]
fn a_bad_argument_shape_is_a_usage_error() {
    let output = cli(&["table-plan", fixture().to_str().unwrap()]);
    // EXIT_USAGE = 64 (sysexits).
    assert_eq!(output.status.code(), Some(64));
    let value = one_line_json(&output);
    assert_eq!(value["command"], "table-plan");
    assert_eq!(value["ok"], false);
}

/// 백로그의 목적은 **이미 받은 HLT 안의 표를 고치는 것**이다. 표 편집이 `.HLT` 를 직접
/// 받아 역변환기로 의미 모델을 되돌리고, 편집한 뒤 다시 굽는다.
#[test]
fn table_plan_accepts_a_native_hlt_input() {
    let source = support::repo_root().join("work/pct_a/diagnostics/pct_a_source.HLT");
    let directory = tempfile::tempdir().unwrap();
    let operations = write_operations(
        directory.path(),
        r#"{"operations":[{"kind":"insert_row","table_id":"table:0","index":8}]}"#,
    );
    let output = cli(&[
        "table-plan",
        source.to_str().unwrap(),
        "--operations",
        operations.to_str().unwrap(),
    ]);
    let envelope = one_line_json(&output);
    assert_eq!(envelope["ok"], true, "{envelope}");
    assert_eq!(envelope["result"]["decisions"][0]["status"], "ready");
}
