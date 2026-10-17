//! `new-from-template`: 통특기 서식 → 「새 문서」 .HLT. KIPO 서식은 저장소에 없어서 저장소 픽스처
//! (현행 0x33 .HLT)를 서식 자리에 넣는다.

mod support;

use std::process::Command;

fn run(args: &[&str]) -> (Option<i32>, serde_json::Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_hlt-cli"))
        .args(args)
        .current_dir(support::repo_root())
        .output()
        .unwrap();
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    (
        output.status.code(),
        serde_json::from_str(stdout.trim()).unwrap(),
    )
}

#[test]
fn writes_a_verified_new_document_and_never_overwrites() {
    let template = support::pct_table_fixture();
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("new.HLT");
    let args = [
        "new-from-template",
        template.path().to_str().unwrap(),
        output.to_str().unwrap(),
    ];

    let (code, value) = run(&args);
    assert_eq!(code, Some(0), "{value}");
    assert_eq!(value["command"], "new-from-template");
    assert_eq!(value["result"]["verification"]["window_ok"], true);
    assert!(value["result"]["verification"]["window_proof"].is_string());
    let written = std::fs::read(&output).unwrap();

    let (code, value) = run(&args);
    assert_eq!(code, Some(2));
    assert_eq!(value["error"]["code"], "output_exists");
    assert_eq!(std::fs::read(&output).unwrap(), written);
}

#[test]
fn missing_arguments_are_a_usage_error() {
    let (code, value) = run(&["new-from-template"]);
    assert_eq!(code, Some(64));
    assert_eq!(value["ok"], false);
}
