mod support;

use std::process::{Command, Output};

fn inspect_hml(args: &[&str]) -> Output {
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

fn assert_no_forbidden_keys(value: &serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                assert!(!["body", "raw", "image_bytes", "base64"].contains(&key.as_str()));
                assert_no_forbidden_keys(child);
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                assert_no_forbidden_keys(child);
            }
        }
        _ => {}
    }
}

#[test]
fn real_hml_summary_uses_a_one_line_bounded_envelope() {
    let output = inspect_hml(&[
        "inspect-hml",
        "work/pct_a/pct_a_v5.hml",
        "--kind",
        "summary",
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, b"");
    let value = json(&output);
    assert!(support::command_schema_accepts(&value));
    assert!(support::hml_inspect_schema_accepts(&value["result"]));
    assert_eq!(value["command"], "inspect-hml");
    assert_eq!(value["ok"], true);
    assert_eq!(value["result"]["source"], "pct_a_v5.hml");
    assert_eq!(value["result"]["encoding"], "cp949");
    assert_eq!(value["result"]["summary"]["section_count"], "60");
    assert_eq!(
        value["result"]["summary"]["top_level_paragraph_count"],
        "1149"
    );
    assert_eq!(value["result"]["summary"]["table_count"], "2");
    assert_eq!(value["result"]["summary"]["row_count"], "19");
    assert_eq!(value["result"]["summary"]["cell_count"], "87");
    assert_eq!(value["result"]["summary"]["image_count"], "281");
    assert_eq!(value["result"]["items"], serde_json::json!([]));
    assert_no_forbidden_keys(&value);
    assert!(
        !String::from_utf8_lossy(&output.stdout)
            .contains(support::repo_root().to_string_lossy().as_ref())
    );
}

#[test]
fn hml_sections_tables_cells_and_images_are_independently_pageable() {
    let path = "work/pct_a/pct_a_v5.hml";
    let sections = json(&inspect_hml(&[
        "inspect-hml",
        path,
        "--kind",
        "sections",
        "--limit",
        "2",
    ]));
    assert_eq!(sections["result"]["items"][0]["id"], "section:1");
    assert_eq!(sections["result"]["items"][0]["title"], "【발명의 설명】");
    assert_eq!(sections["result"]["next_cursor"], "2");

    let tables = json(&inspect_hml(&[
        "inspect-hml",
        path,
        "--kind",
        "tables",
        "--limit",
        "1",
    ]));
    assert_eq!(tables["result"]["items"][0]["max_columns"], 5);
    assert_eq!(tables["result"]["items"][0]["row_count"], 16);

    let cells = json(&inspect_hml(&[
        "inspect-hml",
        path,
        "--kind",
        "cells",
        "--limit",
        "1",
    ]));
    assert_eq!(cells["result"]["items"][0]["id"], "cell:1:1:1");
    assert_eq!(cells["result"]["items"][0]["text"], "화합물");
    assert_eq!(cells["result"]["items"][0]["width_raw"], 3696);
    assert_eq!(
        cells["result"]["items"][0]["inlines"][0]["formats"],
        serde_json::json!(["bold"])
    );

    let images = json(&inspect_hml(&[
        "inspect-hml",
        path,
        "--kind",
        "images",
        "--limit",
        "1",
    ]));
    assert_eq!(images["result"]["items"][0]["id"], "image:0");
    assert_eq!(images["result"]["items"][0]["occurrence"], 0);

    for value in [&sections, &tables, &cells, &images] {
        assert!(support::hml_inspect_schema_accepts(&value["result"]));
        assert_no_forbidden_keys(value);
    }
}

#[test]
fn hml_paragraph_pages_retain_shape_and_inline_order() {
    let first_content = json(&inspect_hml(&[
        "inspect-hml",
        "work/pct_a/pct_a_v5.hml",
        "--kind",
        "paragraphs",
        "--cursor",
        "2",
        "--limit",
        "1",
    ]));
    let paragraph = &first_content["result"]["items"][0];
    assert_eq!(paragraph["id"], "paragraph:3");
    assert_eq!(paragraph["section_id"], "section:2");
    assert_eq!(paragraph["indent_raw"], 1000);
    assert_eq!(paragraph["align"], "left");
    let text: Vec<char> = paragraph["text"].as_str().unwrap().chars().collect();
    assert!(
        text.windows(6)
            .any(|window| digest(&window.iter().collect::<String>()) == "719cf4da39292561")
    );
    assert!(support::hml_inspect_schema_accepts(
        &first_content["result"]
    ));
}

#[test]
fn inspect_hml_rejects_invalid_kind_and_limits_with_usage_envelopes() {
    for args in [
        vec!["inspect-hml", "work/pct_a/pct_a_v5.hml", "--kind", "runs"],
        vec!["inspect-hml", "work/pct_a/pct_a_v5.hml", "--limit", "0"],
        vec!["inspect-hml", "work/pct_a/pct_a_v5.hml", "--limit", "501"],
    ] {
        let output = inspect_hml(&args);
        assert_eq!(output.status.code(), Some(64));
        let value = json(&output);
        assert_eq!(value["command"], "inspect-hml");
        assert_eq!(value["error"]["code"], "verification_failed");
    }
}

/// 실물 문서의 글은 저장소에 싣지 않는다 — SHA-256 앞 16자로 대조한다.
#[allow(dead_code)]
fn digest(text: &str) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(text.as_bytes()))[..16].to_string()
}

/// 글 앞 `chars` 글자가 그 SHA-256(앞 16자)인가.
#[allow(dead_code)]
fn opens_with(text: &str, chars: usize, expected: &str) -> bool {
    digest(&text.chars().take(chars).collect::<String>()) == expected
}

/// 실물 본문 첫 문단(기술분야)과 둘째 문단(배경)의 첫머리 — 글 대신 지문.
#[allow(dead_code)]
const TECH_FIELD: &str = "4230137345539271";
#[allow(dead_code)]
const BACKGROUND: &str = "1a352eee416dc74b";
