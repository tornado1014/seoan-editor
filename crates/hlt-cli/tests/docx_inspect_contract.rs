mod support;

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hlt-cli"))
        .args(args)
        .current_dir(support::repo_root())
        .output()
        .unwrap()
}

fn json(output: &Output) -> serde_json::Value {
    assert_eq!(String::from_utf8_lossy(&output.stdout).lines().count(), 1);
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn real_docx_summary_is_safe_bounded_and_schema_valid() {
    let output = run(&[
        "inspect-docx",
        "work/pct_a/diagnostics/pct_a_source.docx",
        "--kind",
        "summary",
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, b"");
    let value = json(&output);
    assert!(support::command_schema_accepts(&value));
    assert!(support::docx_inspect_schema_accepts(&value["result"]));
    assert_eq!(value["command"], "inspect-docx");
    assert_eq!(value["result"]["source"], "pct_a_source.docx");
    assert_eq!(value["result"]["package"]["entry_count"], 80);
    assert_eq!(value["result"]["package"]["media_count"], 69);
    assert_eq!(value["result"]["page_break_count"], 58);
    assert_eq!(
        value["result"]["semantic_sha256"],
        "7DD8B6ACA163097A7706DE15FBFE4FBB3E332940B109B0FBE75A9A49E3C677F4"
    );
    assert_eq!(value["result"]["page"]["summary"]["section_count"], "151");
    assert_eq!(value["result"]["page"]["summary"]["image_count"], "69");
    assert_eq!(value["result"]["media_items"], serde_json::json!([]));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("C:\\Users\\user"));
}

#[test]
fn docx_media_is_pageable_without_payload_bytes() {
    let value = json(&run(&[
        "inspect-docx",
        "work/pct_a/diagnostics/pct_a_source.docx",
        "--kind",
        "media",
        "--limit",
        "1",
    ]));
    assert!(support::docx_inspect_schema_accepts(&value["result"]));
    assert_eq!(value["result"]["kind"], "media");
    assert_eq!(value["result"]["page"], serde_json::Value::Null);
    assert_eq!(value["result"]["media_items"].as_array().unwrap().len(), 1);
    let media = &value["result"]["media_items"][0];
    assert!(media["sha256"].as_str().unwrap().len() == 64);
    assert!(media.get("payload").is_none());
    assert!(media.get("base64").is_none());
    assert_eq!(value["result"]["next_cursor"], "1");
}

#[test]
fn inspect_docx_rejects_invalid_usage_and_bad_input() {
    for args in [
        vec!["inspect-docx", "README.md", "--limit", "0"],
        vec!["inspect-docx", "README.md", "--kind", "runs"],
    ] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(64));
        assert_eq!(json(&output)["command"], "inspect-docx");
    }
    let output = run(&["inspect-docx", "README.md"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json(&output)["error"]["code"], "unsupported_format");
}
