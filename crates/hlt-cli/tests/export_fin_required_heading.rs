//! `export-fin` 필수 표제 관문: 명세서에 필수 식별항목(E-245)이 빠지면 제출파일을 만들지 않는다.
//! 원문에 그 절이 정말 없을 때만 `--allow-missing-heading "표제=사유"` 로 지나가고, 사유는 결과에 남는다.
//! E-249(묶음 표제 바로 아래 본문)는 접수된 실물이 있어 막지 않고 경고만.

use std::process::Command;

fn cli(arguments: &[&str]) -> (Option<i32>, serde_json::Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_hlt-cli"))
        .args(arguments)
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    (
        output.status.code(),
        serde_json::from_str(stdout.trim_end()).unwrap(),
    )
}

const SUMMARY: &str = "<summary-of-invention><tech-solution><p num=\"0003\">해결 문단이다.</p></tech-solution></summary-of-invention>";

/// 국내 명세서 필수 표제를 다 갖춘 합성 XML — `summary` 가 비면 【발명의 내용】이 없다.
fn spec_xml(summary: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<KIPO><PatentCAFDOC>
<description><invention-title>시험 장치{{TEST APPARATUS}}</invention-title>
<technical-field><p num="0001">본 발명은 시험 장치에 관한 것이다.</p></technical-field>
<background-art><p num="0002">배경 문단이다.</p></background-art>
{summary}
<description-of-embodiments><p num="0004">구체 문단이다.</p></description-of-embodiments>
</description>
<claims><claim num="1"><claim-text>몸체를 포함하는 장치.</claim-text></claim></claims>
<abstract><summary><p num="0001a">시험 장치.</p></summary></abstract>
</PatentCAFDOC></KIPO>"#
    )
}

struct Case {
    dir: tempfile::TempDir,
}

impl Case {
    fn new(xml: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let case = Case { dir };
        std::fs::write(case.path("spec.xml"), xml).unwrap();
        let (code, value) = cli(&[
            "generate-hlt",
            &case.path("spec.xml"),
            &case.path("spec.HLT"),
            "--dialect",
            "patent-specification-ko",
        ]);
        assert_eq!(code, Some(0), "{value}");
        case
    }

    fn path(&self, name: &str) -> String {
        self.dir.path().join(name).to_str().unwrap().to_string()
    }

    fn export(&self, input: &str, extra: &[&str]) -> (Option<i32>, serde_json::Value) {
        let (hlt, fin) = (self.path(input), self.path("spec.fin"));
        let mut args = vec![
            "export-fin",
            hlt.as_str(),
            fin.as_str(),
            "--hlz-name",
            "spec.hlz",
            "--app-date",
            "2026-11-09",
            "--app-time",
            "1762650000",
        ];
        args.extend_from_slice(extra);
        cli(&args)
    }
}

#[test]
fn refuses_a_specification_without_a_required_heading() {
    let case = Case::new(&spec_xml(""));
    let (code, value) = case.export("spec.HLT", &[]);
    assert_ne!(code, Some(0), "{value}");
    assert_eq!(
        value["error"]["details"]["reason"],
        "required_heading_missing"
    );
    assert_eq!(
        value["error"]["details"]["missing"], "발명의 내용",
        "{value}"
    );
    assert!(!std::path::Path::new(&case.path("spec.fin")).exists());
}

#[test]
fn passes_when_the_missing_heading_is_allowed_with_a_reason() {
    let case = Case::new(&spec_xml(""));
    let (code, value) = case.export(
        "spec.HLT",
        &["--allow-missing-heading", "발명의 내용=원문에 없음"],
    );
    assert_eq!(code, Some(0), "{value}");
    assert_eq!(
        value["result"]["allowed_missing"],
        serde_json::json!([{"heading": "발명의 내용", "reason": "원문에 없음"}])
    );
}

#[test]
fn allowing_another_heading_does_not_let_this_one_through() {
    let case = Case::new(&spec_xml(""));
    let (code, value) = case.export(
        "spec.HLT",
        &["--allow-missing-heading", "기술분야=원문에 없음"],
    );
    assert_ne!(code, Some(0), "{value}");
    assert_eq!(value["error"]["details"]["missing"], "발명의 내용");
}

#[test]
fn allow_needs_a_heading_and_a_reason() {
    let case = Case::new(&spec_xml(""));
    for bad in ["발명의 내용", "발명의 내용=", "=사유"] {
        let (hlt, fin) = (case.path("spec.HLT"), case.path("spec.fin"));
        let status = Command::new(env!("CARGO_BIN_EXE_hlt-cli"))
            .args([
                "export-fin",
                &hlt,
                &fin,
                "--hlz-name",
                "spec.hlz",
                "--app-date",
                "2026-11-09",
                "--app-time",
                "1762650000",
                "--allow-missing-heading",
                bad,
            ])
            .output()
            .unwrap()
            .status;
        assert_ne!(status.code(), Some(0), "{bad}");
        assert!(!std::path::Path::new(&fin).exists(), "{bad}");
    }
}

#[test]
fn a_complete_specification_passes_unchanged() {
    let case = Case::new(&spec_xml(SUMMARY));
    let (code, value) = case.export("spec.HLT", &[]);
    assert_eq!(code, Some(0), "{value}");
    assert_eq!(value["result"]["allowed_missing"], serde_json::json!([]));
}

#[test]
fn body_right_under_a_container_heading_only_warns() {
    let case = Case::new(&spec_xml(SUMMARY));
    let session =
        hlt_core::EditorSession::open(std::path::Path::new(&case.path("spec.HLT"))).unwrap();
    let sub = session
        .paragraphs()
        .into_iter()
        .find(|p| p.text.trim() == "【과제의 해결 수단】")
        .unwrap()
        .id;
    std::fs::write(
        case.path("ops.json"),
        serde_json::json!({"operations": [{"paragraph_id": sub, "text": "묶음 바로 아래 본문."}]})
            .to_string(),
    )
    .unwrap();
    let (code, value) = cli(&[
        "edit-as-new",
        &case.path("spec.HLT"),
        &case.path("e249.HLT"),
        "--operations",
        &case.path("ops.json"),
    ]);
    assert_eq!(code, Some(0), "{value}");
    let (code, value) = case.export("e249.HLT", &[]);
    assert_eq!(code, Some(0), "{value}");
    let warnings = value["warnings"].as_array().unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().unwrap().contains("E-249")),
        "{value}"
    );
}
