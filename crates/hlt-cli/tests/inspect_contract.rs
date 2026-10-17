mod support;

use std::process::{Command, Output};

fn inspect(args: &[&str]) -> Output {
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
fn pct_table_summary_and_table_page_use_the_bounded_contract() {
    let fixture = support::pct_table_fixture();
    let path = fixture.path().to_str().unwrap();
    let summary = json(&inspect(&["inspect", path, "--kind", "summary"]));
    assert_eq!(summary["ok"], true);
    assert_eq!(summary["result"]["summary"]["section_count"], "3");
    assert_eq!(summary["result"]["summary"]["line_count"], "29");
    assert_eq!(summary["result"]["summary"]["span_count"], "29");
    assert_eq!(summary["result"]["summary"]["table_count"], "1");
    assert_eq!(summary["result"]["summary"]["cell_count"], "16");
    assert_eq!(summary["result"]["summary"]["image_count"], "0");
    assert_eq!(summary["result"]["summary"]["document_type_raw"], 32881);
    assert_eq!(
        summary["result"]["summary"]["document_tree_kind"],
        "pct_patent"
    );
    assert_eq!(
        summary["result"]["summary"]["document_tree_label"],
        "PCT특허"
    );
    support::validate_inspect_envelope(&summary);
    assert_no_forbidden_keys(&summary);

    let tables = json(&inspect(&[
        "inspect", path, "--kind", "tables", "--limit", "100",
    ]));
    assert_eq!(tables["ok"], true);
    assert_eq!(tables["result"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(tables["result"]["items"][0]["row_count"], 8);
    assert_eq!(tables["result"]["items"][0]["column_count"], 2);
    support::validate_inspect_envelope(&tables);
    assert_no_forbidden_keys(&tables);
}

#[test]
fn inspect_summary_is_one_schema_valid_bounded_envelope() {
    let output = inspect(&[
        "inspect",
        "work/pct_a/pct_a_full_v5.HLT",
        "--kind",
        "summary",
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, b"");
    let value = json(&output);
    support::validate_inspect_envelope(&value);
    assert_eq!(value["command"], "inspect");
    assert_eq!(value["ok"], true);
    assert_eq!(value["result"]["source"], "pct_a_full_v5.HLT");
    assert_eq!(value["result"]["summary"]["section_count"], "60");
    assert_eq!(
        value["result"]["summary"]["document_title"],
        "【발명의 설명】"
    );
    assert_eq!(value["result"]["summary"]["document_type_raw"], 140);
    assert_eq!(
        value["result"]["summary"]["document_tree_kind"],
        "patent_specification_ko"
    );
    assert_eq!(
        value["result"]["summary"]["document_tree_label"],
        "특허 명세서등(국어)"
    );
    assert_eq!(
        value["result"]["summary"]["fixed_blocks"]["style_table_sha256"],
        "7A8333966E717DF4805BCE62291FF9CEAC48355B617BF1FC2EA07DD2E25D5511"
    );
    assert_eq!(value["result"]["summary"]["table_count"], "2");
    assert_eq!(value["result"]["summary"]["image_count"], "281");
    assert_eq!(value["result"]["summary"]["paragraph_wipoid_nonzero"], "0");
    assert_eq!(value["result"]["items"], serde_json::json!([]));
    assert_no_forbidden_keys(&value);
    assert!(
        !String::from_utf8_lossy(&output.stdout)
            .contains(support::repo_root().to_string_lossy().as_ref())
    );
}

#[test]
fn adjacent_section_pages_have_no_duplicate_ids() {
    let first = json(&inspect(&[
        "inspect",
        "work/pct_a/pct_a_full_v5.HLT",
        "--kind",
        "sections",
        "--cursor",
        "0",
        "--limit",
        "2",
    ]));
    let second = json(&inspect(&[
        "inspect",
        "work/pct_a/pct_a_full_v5.HLT",
        "--kind",
        "sections",
        "--cursor",
        "2",
        "--limit",
        "2",
    ]));
    assert_eq!(first["result"]["next_cursor"], "2");
    assert_eq!(second["result"]["next_cursor"], "4");
    let first_ids: Vec<_> = first["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect();
    let second_ids: Vec<_> = second["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect();
    assert_eq!(first_ids, ["section:1", "section:2"]);
    assert_eq!(second_ids, ["section:3", "section:4"]);
}

#[test]
fn table_and_image_pages_expose_geometry_and_occurrence_not_payloads() {
    let tables = json(&inspect(&[
        "inspect",
        "work/pct_a/pct_a_full_v5.HLT",
        "--kind",
        "tables",
        "--cursor",
        "0",
        "--limit",
        "2",
    ]));
    // pct_a_full_v5 는 파이썬 세대 생성 후보라 표마다 빈 행이 하나 더 있다(실측).
    // 통특기 저장본 align_a 는 같은 문서인데도 16 행이다.
    assert_eq!(tables["result"]["items"][0]["row_count"], 17);
    assert_eq!(tables["result"]["items"][0]["column_count"], 5);
    let images = json(&inspect(&[
        "inspect",
        "work/pct_a/pct_a_full_v5.HLT",
        "--kind",
        "images",
        "--cursor",
        "0",
        "--limit",
        "1",
    ]));
    assert_eq!(images["result"]["items"][0]["id"], "image:0");
    assert_eq!(images["result"]["items"][0]["occurrence"], "0");
    assert_no_forbidden_keys(&tables);
    assert_no_forbidden_keys(&images);
}

#[test]
fn rejects_zero_or_limit_over_500_with_one_usage_envelope() {
    for limit in ["0", "501"] {
        let output = inspect(&["inspect", "work/pct_a/pct_a_full_v5.HLT", "--limit", limit]);
        assert_eq!(output.status.code(), Some(64));
        let value = json(&output);
        assert_eq!(value["command"], "inspect");
        assert_eq!(value["error"]["code"], "verification_failed");
    }
}

#[test]
fn cursor_beyond_last_page_is_empty_and_terminal() {
    let value = json(&inspect(&[
        "inspect",
        "work/pct_a/pct_a_full_v5.HLT",
        "--kind",
        "sections",
        "--cursor",
        "999",
        "--limit",
        "10",
    ]));
    assert_eq!(value["result"]["items"], serde_json::json!([]));
    assert_eq!(value["result"]["has_more"], false);
    assert_eq!(value["result"]["next_cursor"], serde_json::Value::Null);
}

#[test]
fn inspect_schema_rejects_body_or_base64_in_items() {
    let output = inspect(&[
        "inspect",
        "work/pct_a/pct_a_full_v5.HLT",
        "--kind",
        "sections",
        "--cursor",
        "0",
        "--limit",
        "1",
    ]);
    let mut result = json(&output)["result"].clone();
    assert!(support::inspect_schema_accepts(&result));
    result["items"][0]["body"] = serde_json::json!("forbidden");
    assert!(!support::inspect_schema_accepts(&result));
    result["items"][0].as_object_mut().unwrap().remove("body");
    result["items"][0]["base64"] = serde_json::json!("forbidden");
    assert!(!support::inspect_schema_accepts(&result));
}
