//! `edit-as-new`: 편집기 고침(줄 나누기 · 앞 줄에 붙이기 · 그림 바꾸기)을 걸고 새 이름으로 저장한다 — 보정서생성 앞 단계.

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

#[test]
fn splits_and_merges_lines_and_refuses_to_overwrite() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("의견서.HLT");
    hlt_core::save_new_opinion_as_new(&source, hlt_core::WriteOptions::default()).unwrap();
    let session = hlt_core::EditorSession::open(&source).unwrap();
    let paragraphs = session.paragraphs();
    let before = paragraphs.len();
    let texts: Vec<String> = paragraphs.iter().map(|p| p.text.clone()).collect();
    let operations = temp.path().join("ops.json");
    let output = temp.path().join("고침.HLT");
    // id 는 입력 파일 기준: 문단 1 을 세 줄로 나누고, 문단 3 을 문단 2 에 붙인다(적힌 차례와 상관없이).
    std::fs::write(
        &operations,
        serde_json::json!({"operations": [
            {"paragraph_id": "paragraph:1", "text": "첫 줄\n둘째 줄\n셋째 줄"},
            {"merge": "paragraph:3"},
        ]})
        .to_string(),
    )
    .unwrap();
    let args = [
        "edit-as-new",
        source.to_str().unwrap(),
        output.to_str().unwrap(),
        "--operations",
        operations.to_str().unwrap(),
    ];
    let (code, value) = cli(&args);
    assert_eq!(code, Some(0), "{value}");
    assert_eq!(value["result"]["operation_count"], 2);
    let edited = hlt_core::EditorSession::open(&output).unwrap();
    let after: Vec<String> = edited.paragraphs().into_iter().map(|p| p.text).collect();
    assert_eq!(after.len(), before + 1, "{after:?}");
    assert_eq!(after[1..4], ["첫 줄", "둘째 줄", "셋째 줄"], "{after:?}");
    assert_eq!(
        after[4],
        format!("{}{}", texts[2], texts[3]),
        "{texts:?} → {after:?}"
    );

    let (code, value) = cli(&args);
    assert_ne!(code, Some(0));
    assert_eq!(value["error"]["code"], "output_exists");
}

/// `--amendment`: 최종본(.fin)을 고친 그 세션에서 보정서를 낸다 — 문단을 나눠도(식별번호가 세션에만 있다) 거절하지 않는다.
#[test]
fn writes_the_amendment_from_the_same_session() {
    let temp = tempfile::tempdir().unwrap();
    let path = |name: &str| temp.path().join(name).to_str().unwrap().to_string();
    std::fs::write(
        path("spec.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<KIPO><PatentCAFDOC>
<description><invention-title>시험 장치{TEST APPARATUS}</invention-title>
<technical-field><p num="0001">본 발명은 시험 장치에 관한 것이다.</p></technical-field>
</description>
<claims><claim num="1"><claim-text>몸체를 포함하는 장치.</claim-text></claim></claims>
<abstract><summary><p num="0001a">시험 장치.</p></summary></abstract>
</PatentCAFDOC></KIPO>"#,
    )
    .unwrap();
    let ok = |args: &[&str]| {
        let (code, value) = cli(args);
        assert_eq!(code, Some(0), "{value}");
        value
    };
    ok(&[
        "generate-hlt",
        &path("spec.xml"),
        &path("spec.HLT"),
        "--dialect",
        "patent-specification-ko",
    ]);
    ok(&[
        "export-fin",
        &path("spec.HLT"),
        &path("spec.fin"),
        "--hlz-name",
        "spec.hlz",
        "--app-date",
        "2026-10-22",
        "--app-time",
        "1761100000",
    ]);
    let session = hlt_core::EditorSession::open(std::path::Path::new(&path("spec.fin"))).unwrap();
    let body = session
        .paragraphs()
        .into_iter()
        .find(|p| p.text.starts_with("본 발명은"))
        .unwrap()
        .id;
    std::fs::write(
        path("ops.json"),
        serde_json::json!({"operations": [
            {"paragraph_id": body, "text": "본 발명은 시험 장치에 관한 것이다.\n특히 덮개에 관한 것이다."},
        ]})
        .to_string(),
    )
    .unwrap();
    let value = ok(&[
        "edit-as-new",
        &path("spec.fin"),
        &path("after.HLT"),
        "--operations",
        &path("ops.json"),
        "--amendment",
        &path("spec_1차.fin"),
        "--dta-name",
        "spec_1차.dta",
        "--date",
        "2026-10-22",
    ]);
    assert_eq!(value["result"]["amendment_items"], 1, "{value}");
    let amended =
        hlt_core::EditorSession::open(std::path::Path::new(&path("spec_1차.fin"))).unwrap();
    let texts: Vec<String> = amended.paragraphs().into_iter().map(|p| p.text).collect();
    let at = texts
        .iter()
        .position(|t| t.starts_with("본 발명은"))
        .unwrap();
    assert_eq!(texts[at + 1], "특히 덮개에 관한 것이다.", "{texts:?}");
}

/// ops 의 `{"review": 보고서.docx, "proposal": 1}`: 검토보고서 보정안을 걸고 같은 세션에서 보정서까지.
#[test]
fn applies_a_review_report_and_writes_the_amendment() {
    use std::io::Write;
    let temp = tempfile::tempdir().unwrap();
    let path = |name: &str| temp.path().join(name).to_str().unwrap().to_string();
    std::fs::write(
        path("spec.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<KIPO><PatentCAFDOC>
<description><invention-title>시험 장치{TEST APPARATUS}</invention-title>
<technical-field><p num="0001">본 발명은 시험 장치에 관한 것이다.</p></technical-field>
</description>
<claims><claim num="1"><claim-text>몸체를 포함하는 장치.</claim-text></claim>
<claim num="2"><claim-text>제1항에 있어서, 덮개를 더 포함하는 장치.</claim-text></claim></claims>
<abstract><summary><p num="0001a">시험 장치.</p></summary></abstract>
</PatentCAFDOC></KIPO>"#,
    )
    .unwrap();
    let ok = |args: &[&str]| {
        let (code, value) = cli(args);
        assert_eq!(code, Some(0), "{value}");
        value
    };
    ok(&[
        "generate-hlt",
        &path("spec.xml"),
        &path("spec.HLT"),
        "--dialect",
        "patent-specification-ko",
    ]);
    ok(&[
        "export-fin",
        &path("spec.HLT"),
        &path("spec.fin"),
        "--hlz-name",
        "spec.hlz",
        "--app-date",
        "2026-10-22",
        "--app-time",
        "1761100000",
    ]);
    let p = |runs: &str| format!("<w:p>{runs}</w:p>");
    let r = |text: &str| format!("<w:r><w:t>{text}</w:t></w:r>");
    let xml = format!(
        "<w:document xmlns:w=\"w\"><w:body><w:tbl><w:tr><w:tc>{}</w:tc><w:tc>{}{}{}{}</w:tc></w:tr></w:tbl>{}</w:body></w:document>",
        p(&r("보정안 1")),
        p(&r("【청구항 1】")),
        p(&format!(
            "{}<w:del><w:r><w:delText>를</w:delText></w:r></w:del><w:ins><w:r><w:t>와 바닥을</w:t></w:r></w:ins>{}",
            r("몸체"),
            r(" 포함하는 장치.")
        )),
        p("<w:del><w:r><w:delText>【청구항 2】</w:delText></w:r></w:del>"),
        p(
            "<w:del><w:r><w:delText>제1항에 있어서, 덮개를 더 포함하는 장치.</w:delText></w:r></w:del>"
        ),
        p(&r("단락 [0001]의 “시험 장치”는 “검사 장치”의 오기입니다.")),
    );
    {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path("report.docx")).unwrap());
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    let (_, report) = cli(&["review-report", &path("report.docx")]);
    assert_eq!(
        report["result"]["proposals"][0]["claims"][1]["after"],
        serde_json::json!([])
    );
    std::fs::write(
        path("ops.json"),
        serde_json::json!({"operations": [{"review": path("report.docx"), "proposal": 1}]})
            .to_string(),
    )
    .unwrap();
    let value = ok(&[
        "edit-as-new",
        &path("spec.fin"),
        &path("after.HLT"),
        "--operations",
        &path("ops.json"),
        "--amendment",
        &path("spec_1차.fin"),
        "--dta-name",
        "spec_1차.dta",
        "--date",
        "2026-10-22",
    ]);
    assert_eq!(
        value["result"]["review"]["claims_changed"],
        serde_json::json!(["1"]),
        "{value}"
    );
    assert_eq!(
        value["result"]["review"]["claims_deleted"],
        serde_json::json!(["2"]),
        "{value}"
    );
    assert_eq!(
        value["result"]["review"]["notes_applied"],
        serde_json::json!([[0, 1]]),
        "{value}"
    );
    assert_eq!(value["result"]["amendment_items"], 3, "{value}");
}
