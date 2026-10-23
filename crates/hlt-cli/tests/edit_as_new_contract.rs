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
    // 보정 표시 요소 = 보정서 항목(3) · 인용 문제 없음.
    let marks = &value["result"]["marks"];
    let count = |key: &str| marks[key].as_array().map_or(0, Vec::len);
    assert_eq!(
        count("changed") + count("added") + count("removed"),
        3,
        "{value}"
    );
    assert_eq!(marks["removed"], serde_json::json!(["청구항 2"]), "{value}");
    assert_eq!(
        value["result"]["claim_issues"],
        serde_json::json!([]),
        "{value}"
    );
    // 보정 근거: 바뀐 청구항 1 의 새 말이 실린다.
    assert_eq!(value["result"]["basis"][0]["claim"], 1, "{value}");
    assert!(
        value["result"]["basis"][0]["phrases"]
            .as_array()
            .is_some_and(|phrases| !phrases.is_empty()),
        "{value}"
    );
}

/// 「…보정서첨부.fin」 + 옆 「…보정서.dta」 → 보정이 반영된 채로 연다(통특기처럼) · xresult 가 부르는데 zip 에 없는 dta 는
/// fin 옆에서 찾는다 · 그 세션에서 낸 다음 최종본엔 옆 dta 가 실려 옆 파일 없이 열린다.
#[test]
fn opens_an_attached_final_with_the_amendment_beside_it() {
    use std::io::{Read, Write};
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
    // 1차 보정(문단 하나를 나눔) → spec_1차.fin(안에 spec_1.dta).
    let edit = |fin: &str, out: &str, dta: &str, from: &str, to: &str| {
        let session = hlt_core::EditorSession::open(std::path::Path::new(fin)).unwrap();
        let id = session
            .paragraphs()
            .into_iter()
            .find(|p| p.text == from)
            .unwrap()
            .id;
        std::fs::write(
            path("ops.json"),
            serde_json::json!({"operations": [{"paragraph_id": id, "text": to}]}).to_string(),
        )
        .unwrap();
        let _ = std::fs::remove_file(path("after.HLT"));
        ok(&[
            "edit-as-new",
            fin,
            &path("after.HLT"),
            "--operations",
            &path("ops.json"),
            "--amendment",
            out,
            "--dta-name",
            dta,
            "--date",
            "2026-10-23",
        ]);
    };
    edit(
        &path("spec.fin"),
        &path("spec_1.fin"),
        "spec_1.dta",
        "본 발명은 시험 장치에 관한 것이다.",
        "본 발명은 시험 장치에 관한 것이다.\n특히 덮개에 관한 것이다.",
    );
    // 보정 표시 기준: 연 상태(기본)엔 바뀐 것이 없고 · 원출원에 견주면 1차 보정(나눈 문단)이 보인다 · 더 고치면 둘 다.
    {
        let mut session =
            hlt_core::EditorSession::open(std::path::Path::new(&path("spec_1.fin"))).unwrap();
        let count = |marks: &hlt_core::AmendmentMarks| {
            marks.changed.len() + marks.added.len() + marks.removed.len()
        };
        assert_eq!(count(&session.amendment_marks_from(false).unwrap()), 0);
        let original = session.amendment_marks_from(true).unwrap();
        assert!(count(&original) > 0, "{original:?}");
        let split = session
            .paragraphs()
            .into_iter()
            .find(|p| p.text == "특히 덮개에 관한 것이다.")
            .unwrap()
            .id;
        assert!(
            original.paragraphs.iter().any(|mark| mark.id == split),
            "{original:?}"
        );
        // 요약서 「0001a」 같은 글자 붙은 번호도 원출원 쪽에 살아 있어야 한다(없으면 「새로 넣은 것」으로 잘못 뜬다 — 실물).
        assert!(
            original.added.iter().all(|item| !item.contains("0001a")),
            "{original:?}"
        );
        assert!(session.amendment_basis_from(true).is_some());
        // 원출원은 담아 두고 다시 굽지 않는다(같은 답).
        assert_eq!(session.amendment_marks_from(true).unwrap(), original);
    }
    // zip 항목 읽기 · 다시 쓰기(이름 바이트 그대로).
    let entries = |fin: &str| -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut archive = zip::ZipArchive::new(std::fs::File::open(fin).unwrap()).unwrap();
        (0..archive.len())
            .map(|i| {
                let mut entry = archive.by_index(i).unwrap();
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).unwrap();
                (entry.name_raw().to_vec(), bytes)
            })
            .collect()
    };
    let write_zip = |out: &std::path::Path, items: &[(Vec<u8>, Vec<u8>)]| {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(out).unwrap());
        for (name, bytes) in items {
            zip.start_file(
                String::from_utf8(name.clone()).unwrap(),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    };
    let dta = entries(&path("spec_1.fin"))
        .into_iter()
        .find(|(name, _)| name.ends_with(b".dta"))
        .unwrap()
        .1;
    let texts = |fin: &std::path::Path| -> Vec<String> {
        hlt_core::EditorSession::open(fin)
            .unwrap()
            .paragraphs()
            .into_iter()
            .map(|p| p.text)
            .collect()
    };
    let amended = "특히 덮개에 관한 것이다.".to_string();

    // ② 「x보정서첨부.fin」(보정 전 최종본) 옆 「x보정서.dta」.
    let attached = temp.path().join("attached");
    std::fs::create_dir(&attached).unwrap();
    std::fs::copy(path("spec.fin"), attached.join("x보정서첨부.fin")).unwrap();
    assert!(
        !texts(&attached.join("x보정서첨부.fin")).contains(&amended),
        "옆 dta 가 없으면 보정 전"
    );
    std::fs::write(attached.join("x보정서.dta"), &dta).unwrap();
    assert!(texts(&attached.join("x보정서첨부.fin")).contains(&amended));

    // 실물처럼 dta 를 이미 담은 첨부 fin 옆에 같은 dta 가 다른 이름으로 있어도 두 번 걸지 않는다.
    let twice = temp.path().join("twice");
    std::fs::create_dir(&twice).unwrap();
    std::fs::copy(path("spec_1.fin"), twice.join("z보정서첨부.fin")).unwrap();
    std::fs::write(twice.join("z보정서.dta"), &dta).unwrap();
    assert_eq!(
        texts(&twice.join("z보정서첨부.fin")),
        texts(std::path::Path::new(&path("spec_1.fin")))
    );
    // 다음 보정을 내면 새 최종본엔 dta 가 둘(안의 1차 + 새 2차) — 옆 것을 또 싣지 않는다.
    edit(
        twice.join("z보정서첨부.fin").to_str().unwrap(),
        &path("z_2.fin"),
        "z_2.dta",
        "특히 덮개에 관한 것이다.",
        "특히 작은 덮개에 관한 것이다.",
    );
    assert_eq!(
        entries(&path("z_2.fin"))
            .iter()
            .filter(|(name, _)| name.ends_with(b".dta"))
            .count(),
        2
    );

    // ① xresult 가 부르는 spec_1.dta 를 zip 에서 뺀 최종본 → 옆에 있으면 열리고 없으면 거절.
    let missing = temp.path().join("missing");
    std::fs::create_dir(&missing).unwrap();
    let kept: Vec<_> = entries(&path("spec_1.fin"))
        .into_iter()
        .filter(|(name, _)| !name.ends_with(b".dta"))
        .collect();
    write_zip(&missing.join("y.fin"), &kept);
    let error = hlt_core::EditorSession::open(&missing.join("y.fin"))
        .err()
        .expect("옆 dta 없이 열렸다");
    assert_eq!(error.details["reason"], "previous_dta", "{error:?}");
    std::fs::write(missing.join("spec_1.dta"), &dta).unwrap();
    assert!(texts(&missing.join("y.fin")).contains(&amended));

    // 다음 보정: 첨부 fin 세션에서 2차 → 새 최종본에 옆 dta 가 실려 다른 폴더에서도 1 · 2차가 다 보인다.
    edit(
        attached.join("x보정서첨부.fin").to_str().unwrap(),
        &path("x_2.fin"),
        "x_2.dta",
        "특히 덮개에 관한 것이다.",
        "특히 큰 덮개에 관한 것이다.",
    );
    let alone = temp.path().join("alone");
    std::fs::create_dir(&alone).unwrap();
    std::fs::rename(path("x_2.fin"), alone.join("x_2.fin")).unwrap();
    assert_eq!(
        entries(alone.join("x_2.fin").to_str().unwrap())
            .iter()
            .filter(|(name, _)| name.ends_with(b".dta"))
            .count(),
        2
    );
    let after = texts(&alone.join("x_2.fin"));
    assert!(
        after.contains(&"특히 큰 덮개에 관한 것이다.".to_string()),
        "{after:?}"
    );
    assert!(
        after.contains(&"본 발명은 시험 장치에 관한 것이다.".to_string()),
        "{after:?}"
    );
}
