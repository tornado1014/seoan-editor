mod support;

use std::process::{Command, Output};

fn inspect_xml(args: &[&str]) -> Output {
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
fn real_xml_summary_is_one_safe_strict_semantic_envelope() {
    let output = inspect_xml(&[
        "inspect-kipo-xml",
        "work/pct_a/pct_a.xml",
        "--kind",
        "summary",
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, b"");
    let value = json(&output);
    assert!(support::command_schema_accepts(&value));
    assert!(support::kipo_xml_inspect_schema_accepts(&value["result"]));
    assert_eq!(value["command"], "inspect-kipo-xml");
    assert_eq!(value["ok"], true);
    assert_eq!(value["result"]["source"], "pct_a.xml");
    assert_eq!(value["result"]["input_encoding"], "utf-8");
    assert_eq!(value["result"]["document_root"], "PCTPatentCAFDOC");
    assert_eq!(value["result"]["dialect"], "pct_patent_cafdoc");
    assert_eq!(
        value["result"]["source_sha256"],
        "F0F3F2C17ECFE3F90CA78D905E4ADCB2E792D988A1FF43CCF018306072F14D3C"
    );
    assert_eq!(
        value["result"]["semantic_sha256"],
        "C979B658264C2F4251F69348B04B6F4B1B17A69E35B95D964DE2062DDD5F2C2D"
    );
    assert_eq!(
        digest(value["result"]["invention_title"].as_str().unwrap()),
        "31c7039b7d2a1da6"
    );
    assert_eq!(value["result"]["page"]["summary"]["section_count"], "60");
    assert_eq!(value["result"]["page"]["summary"]["table_count"], "2");
    assert_eq!(value["result"]["page"]["summary"]["image_count"], "281");
    assert_eq!(value["result"]["page"]["items"], serde_json::json!([]));
    assert_no_forbidden_keys(&value);
    assert!(
        !String::from_utf8_lossy(&output.stdout)
            .contains(support::repo_root().to_string_lossy().as_ref())
    );
}

#[test]
fn xml_semantic_items_are_pageable_with_the_hml_contract() {
    let path = "work/pct_a/pct_a.xml";
    let sections = json(&inspect_xml(&[
        "inspect-kipo-xml",
        path,
        "--kind",
        "sections",
        "--limit",
        "2",
    ]));
    assert_eq!(sections["result"]["page"]["items"][0]["id"], "section:1");
    assert_eq!(
        sections["result"]["page"]["items"][0]["title"],
        "【발명의 설명】"
    );
    assert_eq!(sections["result"]["page"]["next_cursor"], "2");

    let tables = json(&inspect_xml(&[
        "inspect-kipo-xml",
        path,
        "--kind",
        "tables",
        "--limit",
        "1",
    ]));
    assert_eq!(tables["result"]["page"]["items"][0]["max_columns"], 5);
    assert_eq!(tables["result"]["page"]["items"][0]["row_count"], 16);

    let cells = json(&inspect_xml(&[
        "inspect-kipo-xml",
        path,
        "--kind",
        "cells",
        "--limit",
        "1",
    ]));
    assert_eq!(cells["result"]["page"]["items"][0]["id"], "cell:1:1:1");
    assert_eq!(cells["result"]["page"]["items"][0]["text"], "화합물");
    assert_eq!(cells["result"]["page"]["items"][0]["hwidth_raw"], 3696);

    let images = json(&inspect_xml(&[
        "inspect-kipo-xml",
        path,
        "--kind",
        "images",
        "--limit",
        "1",
    ]));
    assert_eq!(images["result"]["page"]["items"][0]["id"], "image:0");
    assert_eq!(images["result"]["page"]["items"][0]["occurrence"], 0);

    for value in [&sections, &tables, &cells, &images] {
        assert!(support::kipo_xml_inspect_schema_accepts(&value["result"]));
        assert_no_forbidden_keys(value);
    }
}

#[test]
fn inspect_xml_rejects_invalid_kind_limits_and_bad_sources_with_one_envelope() {
    for args in [
        vec!["inspect-kipo-xml", "work/pct_a/pct_a.xml", "--kind", "runs"],
        vec!["inspect-kipo-xml", "work/pct_a/pct_a.xml", "--limit", "0"],
        vec!["inspect-kipo-xml", "work/pct_a/pct_a.xml", "--limit", "501"],
    ] {
        let output = inspect_xml(&args);
        assert_eq!(output.status.code(), Some(64));
        let value = json(&output);
        assert_eq!(value["command"], "inspect-kipo-xml");
        assert_eq!(value["error"]["code"], "verification_failed");
    }

    let output = inspect_xml(&["inspect-kipo-xml", "README.md"]);
    assert_eq!(output.status.code(), Some(2));
    let value = json(&output);
    assert_eq!(value["command"], "inspect-kipo-xml");
    assert_eq!(value["error"]["code"], "structure_invalid");
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
