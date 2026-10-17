//! 편집기 세션(P5) — 합성 문서로 도는 것. 실물 문서로 재는 세션 테스트는 저장소 밖에 둔다.

use hlt_core::EditorSession;

/// 새 의견서 정렬 단추: 의견서 틀은 양쪽 정렬 모양뿐이라 가운데 · 왼쪽이 양쪽으로 떨어졌다 → 정렬만 바꾼 모양을 덧붙인다.
#[test]
fn aligns_new_opinion_paragraphs_center_and_left() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("의견서.HLT");
    hlt_core::save_new_opinion_as_new(&path, hlt_core::WriteOptions::default()).unwrap();
    let mut session = EditorSession::open(&path).unwrap();
    let body = session.paragraphs()[1].id.clone();
    session
        .replace_text(&body, "가운데 문단\n왼쪽 문단")
        .unwrap();
    let paragraphs = session.paragraphs();
    session.set_alignment(&paragraphs[1].id, "center").unwrap();
    session.set_alignment(&paragraphs[2].id, "left").unwrap();
    let out = temp.path().join("정렬.HLT");
    session.save_as(&out).unwrap();
    let xml = hlt_core::export_kipo_xml_path(&out, &Default::default())
        .unwrap()
        .xml;
    assert!(xml.contains("align=\"CENTER\""), "{xml}");
    assert!(xml.contains("align=\"LEFT\""), "{xml}");
}

/// 의견서 .hlz 열기.
/// 새 의견서 → 글 · 서식 · 정렬 → hlz → 다시 열기 → 다시 hlz: 의견서 XML 이 같다(쪽 표시 · 쪽 수 포함).
#[test]
fn reopens_an_opinion_hlz_it_wrote() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("의견서.HLT");
    hlt_core::save_new_opinion_as_new(&path, hlt_core::WriteOptions::default()).unwrap();
    let mut session = EditorSession::open(&path).unwrap();
    let body = session.paragraphs()[1].id.clone();
    session
        .replace_text(
            &body,
            "Ⅰ. 통지내용\n본 출원인은 A & B 를 <받았>습니다.\n\n가운데 줄",
        )
        .unwrap();
    let ids: Vec<String> = session.paragraphs().iter().map(|p| p.id.clone()).collect();
    session
        .toggle_format(&ids[1], 0, 7, hlt_core::CharacterFormat::Bold)
        .unwrap();
    // 한 문단에 구간 둘 — 구간마다 따로 쓰면 마지막 것만 남았다.
    session
        .toggle_format_spans(
            &[(ids[2].clone(), 0, 2), (ids[2].clone(), 7, 12)],
            hlt_core::CharacterFormat::Underline,
        )
        .unwrap();
    session
        .set_paragraph_shapes(&[(ids[4].clone(), "center".into(), 0)])
        .unwrap();
    let written = temp.path().join("작성.HLT");
    session.save_as(&written).unwrap();
    let hlz_of = |path: &std::path::Path| {
        let export = hlt_core::export_fin_path(
            path,
            &Default::default(),
            &hlt_core::FinExportOptions {
                hlz_name: "의견서.hlz".into(),
                app_date: "2026-10-16".into(),
                app_time: "1".into(),
                image_time: String::new(),
                amendments: Vec::new(),
            },
        )
        .unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(export.hlz.clone())).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut archive.by_index(0).unwrap(), &mut xml).unwrap();
        (export.hlz, xml)
    };
    let (hlz, xml) = hlz_of(&written);
    assert!(xml.contains("<B>Ⅰ. 통지내용</B>"), "{xml}");
    assert_eq!(
        xml.matches("<U>").count(),
        2,
        "한 문단의 밑줄 구간 둘: {xml}"
    );
    let hlz_path = temp.path().join("의견서.hlz");
    std::fs::write(&hlz_path, hlz).unwrap();
    let reopened = EditorSession::open(&hlz_path).expect("의견서 hlz 를 연다");
    assert!(reopened.editable(), "의견서로 고칠 수 있다");
    let again = temp.path().join("다시.HLT");
    let mut reopened = reopened;
    reopened.save_as(&again).unwrap();
    let (_, xml_again) = hlz_of(&again);
    assert_eq!(xml_again, xml, "hlz → 열기 → hlz 왕복");
}
