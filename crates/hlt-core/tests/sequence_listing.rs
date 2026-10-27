//! 서열목록 넣기(통특기 「입력 › 서열목록 삽입」) — 합성 국내 명세서로. 기대값은 통특기 실측(합성 문서 · ST.26)에서 왔다:
//! 【청구범위】 앞에 【서열목록】 · 「서열목록 전자파일 첨부(경로)」 · 빈 줄 · 책갈피는 첨부 줄 · 제출파일 hlz 에 `st26sdosl.xml`.

use std::io::Read;
use std::path::{Path, PathBuf};

use hlt_core::{EditorSession, GenerationDialect, WriteOptions};

const SPEC: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<KIPO><PatentCAFDOC>
<description><invention-title>시험 핵산{TEST NUCLEIC ACID}</invention-title>
<technical-field><p num="0001">본 발명은 시험 핵산에 관한 것이다.</p></technical-field>
<description-of-embodiments><p num="0002">서열번호 1의 핵산을 사용한다.</p></description-of-embodiments>
</description>
<claims>
<claim num="1"><claim-text>서열번호 1의 핵산.</claim-text></claim>
</claims>
<abstract><summary><p num="0001a">시험 핵산.</p></summary></abstract>
</PatentCAFDOC></KIPO>"#;

const ST26: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ST26SequenceListing dtdVersion=\"V1_3\" fileName=\"seq.xml\"><SequenceTotalQuantity>1</SequenceTotalQuantity></ST26SequenceListing>\n";

fn bake(dir: &Path) -> PathBuf {
    let xml = dir.join("spec.xml");
    std::fs::write(&xml, SPEC).unwrap();
    let (document, payloads, _) = hlt_core::load_generation_input(&xml).unwrap();
    let hlt = dir.join("spec.HLT");
    hlt_core::save_generated_hlt_as_new(
        &xml,
        &document,
        GenerationDialect::PatentSpecificationKo,
        &payloads,
        &hlt,
        WriteOptions::default(),
    )
    .unwrap();
    hlt
}

/// (책갈피 제목, 그 책갈피가 걸린 문단 글) — 저장한 파일에서 읽는다.
fn bookmarks(path: &Path) -> Vec<(String, String)> {
    let document = hlt_core::inspect_path(path, Default::default()).unwrap();
    document
        .sections
        .iter()
        .map(|section| {
            let text = section
                .anchor_paragraph_id
                .as_ref()
                .and_then(|id| document.paragraphs.iter().find(|p| &p.id == id))
                .map(|p| p.text.trim_start_matches('\u{c}').to_string())
                .unwrap_or_else(|| "<없음>".into());
            (section.title.clone(), text)
        })
        .collect()
}

#[test]
fn inserts_before_the_claims_and_survives_a_later_edit() {
    let temp = tempfile::tempdir().unwrap();
    let hlt = bake(temp.path());
    let seq = temp.path().join("서열 목록.xml");
    std::fs::write(&seq, ST26).unwrap();
    let link = format!("서열목록 전자파일 첨부({})", seq.display());

    let mut session = EditorSession::open(&hlt).unwrap();
    let link_id = session.insert_sequence_listing(&seq).unwrap();
    let texts: Vec<String> = session
        .paragraphs()
        .iter()
        .map(|p| p.text.clone())
        .collect();
    let at = texts.iter().position(|t| t == "【서열목록】").unwrap();
    assert_eq!(texts[at - 1], "서열번호 1의 핵산을 사용한다.");
    assert_eq!(texts[at + 1], link);
    assert_eq!(texts[at + 2], "");
    assert!(texts[at + 3].contains("【청구범위】"));
    assert_eq!(session.paragraphs()[at + 1].id, link_id);
    // 첨부 줄 = 줄 전체 밑줄 · 10pt(화면 구간은 서식 · 크기를 따로 낸다).
    let spans = &session.paragraphs()[at + 1].spans;
    let whole = link.chars().count() as u32;
    assert!(
        spans
            .iter()
            .any(|s| (s.start, s.end, s.style & 1) == (0, whole, 1)),
        "{spans:?}"
    );
    assert!(
        spans
            .iter()
            .any(|s| (s.start, s.end, s.size) == (0, whole, Some(10))),
        "{spans:?}"
    );

    let saved = temp.path().join("서열.HLT");
    session.save_as(&saved).unwrap();
    let marks = bookmarks(&saved);
    let expected: Vec<(String, String)> = [
        ("【발명의 설명】", "【발명의 설명】"),
        ("【발명의 명칭】", "【발명의 명칭】"),
        ("【기술분야】", "【기술분야】"),
        (
            "【발명을 실시하기 위한 구체적인 내용】",
            "【발명을 실시하기 위한 구체적인 내용】",
        ),
        ("【서열목록】", link.as_str()),
        ("【청구범위】", "【청구범위】"),
        ("【청구항 1】", "【청구항 1】"),
        ("【요약서】", "【요약서】"),
        ("【요약】", "【요약】"),
    ]
    .iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect();
    assert_eq!(marks, expected);

    // 다른 곳을 고쳐 다시 저장해도 책갈피가 밀리지 않는다(통특기가 넣은 문서를 서안으로 고치면 하나씩 밀렸다).
    let mut again = EditorSession::open(&saved).unwrap();
    let body = again
        .paragraphs()
        .iter()
        .find(|p| p.text.starts_with("본 발명"))
        .unwrap()
        .id
        .clone();
    again
        .replace_text(&body, "본 발명은 시험 핵산이다.")
        .unwrap();
    let resaved = temp.path().join("다시.HLT");
    again.save_as(&resaved).unwrap();
    assert_eq!(bookmarks(&resaved), expected);
}

#[test]
fn inserting_again_only_changes_the_path() {
    let temp = tempfile::tempdir().unwrap();
    let hlt = bake(temp.path());
    let (first, second) = (temp.path().join("a.xml"), temp.path().join("b.xml"));
    std::fs::write(&first, ST26).unwrap();
    std::fs::write(&second, ST26).unwrap();
    let mut session = EditorSession::open(&hlt).unwrap();
    session.insert_sequence_listing(&first).unwrap();
    let count = session.paragraphs().len();
    let link = session.insert_sequence_listing(&second).unwrap();
    assert_eq!(session.paragraphs().len(), count);
    let text = |session: &EditorSession| {
        session
            .paragraphs()
            .iter()
            .find(|p| p.id == link)
            .unwrap()
            .text
            .clone()
    };
    assert_eq!(
        text(&session),
        format!("서열목록 전자파일 첨부({})", second.display())
    );
    // 첨부 줄을 비워도 그 자리를 다시 쓴다(PCT 서식처럼 표제 아래가 빈 줄인 문서).
    session.replace_text(&link, "").unwrap();
    session.insert_sequence_listing(&first).unwrap();
    assert_eq!(session.paragraphs().len(), count);
    let saved = temp.path().join("s.HLT");
    session.save_as(&saved).unwrap();
    assert_eq!(
        bookmarks(&saved)
            .iter()
            .filter(|(title, _)| title == "【서열목록】")
            .count(),
        1
    );
}

#[test]
fn refuses_st25_and_paths_outside_cp949() {
    let temp = tempfile::tempdir().unwrap();
    let hlt = bake(temp.path());
    let mut session = EditorSession::open(&hlt).unwrap();
    let st25 = temp.path().join("seq.app");
    std::fs::write(&st25, "<110>  applicant\n<210>  1\n").unwrap();
    let reason = |error: hlt_core::CoreError| error.details["reason"].clone();
    assert_eq!(
        reason(session.insert_sequence_listing(&st25).unwrap_err()),
        "sequence_not_st26"
    );
    let odd = temp.path().join("서열🧬.xml");
    std::fs::write(&odd, ST26).unwrap();
    assert_eq!(
        reason(session.insert_sequence_listing(&odd).unwrap_err()),
        "sequence_path_cp949"
    );
    assert!(
        !session
            .paragraphs()
            .iter()
            .any(|p| p.text == "【서열목록】")
    );
}

#[test]
fn submission_carries_the_file_and_the_sequence_list_element() {
    let temp = tempfile::tempdir().unwrap();
    let hlt = bake(temp.path());
    let seq = temp.path().join("seq.xml");
    std::fs::write(&seq, ST26).unwrap();
    let mut session = EditorSession::open(&hlt).unwrap();
    session.insert_sequence_listing(&seq).unwrap();
    let saved = temp.path().join("s.HLT");
    session.save_as(&saved).unwrap();
    let export = hlt_core::export_fin_path(
        &saved,
        &Default::default(),
        &hlt_core::FinExportOptions {
            hlz_name: "s.hlz".into(),
            app_date: "2026-10-27".into(),
            app_time: "1761500000".into(),
            image_time: String::new(),
            amendments: Vec::new(),
        },
    )
    .unwrap();
    let mut hlz = zip::ZipArchive::new(std::io::Cursor::new(export.hlz.clone())).unwrap();
    let mut bytes = Vec::new();
    hlz.by_name("st26sdosl.xml")
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, ST26.as_bytes());
    let xml_name = (0..hlz.len())
        .map(|i| hlz.by_index(i).unwrap().name().to_string())
        .find(|name| name != "st26sdosl.xml")
        .unwrap();
    let mut xml = String::new();
    hlz.by_name(&xml_name)
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    // 통특기 실측과 같은 꼴 · 번호는 본문 뒤로 이어서(0001 · 0002 다음) · 경로는 안 싣는다.
    assert!(
        xml.contains("</description-of-embodiments>\n<sequence-list-doc><sequence-list file=\"st26sdosl.xml\" seq-file-type=\"ST.26\">\n<p num=\"0003\">서열목록 전자파일 첨부</p>\n</sequence-list></sequence-list-doc></description>"),
        "{xml}"
    );
    assert!(!xml.contains("seq.xml"), "{xml}");

    // 만든 제출파일을 다시 연다 — hlz 의 서열목록 파일을 명세서 XML 로 읽어 열기가 깨졌다(실물 보고).
    let (fin, hlz) = (temp.path().join("s.fin"), temp.path().join("s.hlz"));
    std::fs::write(&fin, &export.fin).unwrap();
    std::fs::write(&hlz, &export.hlz).unwrap();
    for package in [&fin, &hlz] {
        let work = tempfile::tempdir().unwrap();
        let out = temp.path().join(format!(
            "열기-{}.HLT",
            package.extension().unwrap().to_str().unwrap()
        ));
        hlt_core::hlt_from_package(package, work.path(), &out).unwrap();
        let reopened = EditorSession::open(&out).unwrap();
        // 제출파일을 바로 열면 【서열목록】이 되살아나지 않는다 — 제출 전 점검이 알린다.
        let opened = EditorSession::open(package).unwrap();
        assert!(
            opened
                .submission_checks()
                .iter()
                .any(|issue| issue.kind == hlt_core::SubmitKind::SequenceFile
                    && issue.detail.contains("다시 넣으세요")),
            "{package:?}"
        );
        assert!(
            reopened
                .paragraphs()
                .iter()
                .any(|p| p.text.contains("서열번호 1의 핵산을 사용한다.")),
            "{package:?}"
        );
    }

    // 파일이 없어지면 제출파일을 만들지 않는다.
    std::fs::remove_file(&seq).unwrap();
    let error = hlt_core::export_fin_path(
        &saved,
        &Default::default(),
        &hlt_core::FinExportOptions {
            hlz_name: "s.hlz".into(),
            app_date: "2026-10-27".into(),
            app_time: "1761500000".into(),
            image_time: String::new(),
            amendments: Vec::new(),
        },
    )
    .unwrap_err();
    assert_eq!(error.details["reason"], "sequence_file_missing");
}

#[test]
fn submission_checks_name_a_missing_or_wrong_sequence_file() {
    let temp = tempfile::tempdir().unwrap();
    let hlt = bake(temp.path());
    let seq = temp.path().join("seq.xml");
    std::fs::write(&seq, ST26).unwrap();
    let mut session = EditorSession::open(&hlt).unwrap();
    let link = session.insert_sequence_listing(&seq).unwrap();
    let sequence_issues = |session: &EditorSession| -> Vec<String> {
        session
            .submission_checks()
            .into_iter()
            .filter(|issue| issue.kind == hlt_core::SubmitKind::SequenceFile)
            .map(|issue| issue.detail)
            .collect()
    };
    assert!(sequence_issues(&session).is_empty());
    // 첨부 줄은 빈 식별번호로 걸리지 않는다.
    assert!(
        !session
            .submission_checks()
            .iter()
            .any(|issue| issue.paragraph_id.as_deref() == Some(link.as_str())
                && issue.kind == hlt_core::SubmitKind::Unnumbered)
    );
    std::fs::write(&seq, "<110> x\n").unwrap();
    assert_eq!(
        sequence_issues(&session),
        ["「seq.xml」 — ST.26(XML) 서열목록이 아니다"]
    );
    std::fs::remove_file(&seq).unwrap();
    assert_eq!(
        sequence_issues(&session),
        ["「seq.xml」 — 파일이 없다(옮겼거나 지웠다)"]
    );
    session.replace_text(&link, "").unwrap();
    assert_eq!(sequence_issues(&session).len(), 1);
    assert!(sequence_issues(&session)[0].contains("첨부 줄이 없다"));
}
