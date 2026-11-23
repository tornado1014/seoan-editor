//! 한글 원본(.hwp · .hwpx) 입력: rhwp 로 읽어 워드 길로 넘긴 결과를 본다(2026-11-23).

use std::path::{Path, PathBuf};

use hlt_core::{GenerationDialect, HmlDocument, KIPO_BODY_INDENT_RAW, WriteOptions};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/hwp")
        .join(name)
}

fn paragraph<'a>(document: &'a HmlDocument, text: &str) -> &'a hlt_core::HmlParagraph {
    document
        .paragraphs
        .iter()
        .find(|p| p.text == text)
        .unwrap_or_else(|| panic!("문단 없음: {text}"))
}

fn assert_kipo_form(document: &HmlDocument) {
    let titles: Vec<&str> = document.sections.iter().map(|s| s.title.as_str()).collect();
    for heading in [
        "【발명의 설명】",
        "【발명의 명칭】",
        "【기술분야】",
        "【표 1】",
        "【청구범위】",
        "【청구항 1】",
    ] {
        assert!(titles.contains(&heading), "{heading} 없음: {titles:?}");
    }
    // 원고는 들여쓰기 0 · 양쪽 정렬 → 본문은 표준 들여쓰기가 입혀진다.
    let body = paragraph(
        document,
        "본 발명은 한글 파일 불러오기를 시험하는 장치에 관한 것이다.",
    );
    assert_eq!(
        (body.align.as_str(), body.indent_raw),
        ("justified", KIPO_BODY_INDENT_RAW)
    );
    assert_eq!(paragraph(document, "【기술분야】").indent_raw, 0);
    // 【표 1】 뒤의 표는 표로 온다(4칸 × 3줄, 1~12).
    assert_eq!(document.tables.len(), 1);
    let table = &document.tables[0];
    assert_eq!((table.max_columns, table.row_count), (4, 3));
    let cells: Vec<&str> = document.cells.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(
        cells,
        [
            "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12"
        ]
    );
    // 첫 표제 앞의 표는 글 문단으로 풀린다(생성기는 절 밖 표를 받지 않는다).
    assert_eq!(document.paragraphs[0].text, "1\t2\t3\t4");
    assert!(document.paragraphs[0].section_id.is_none());
}

#[test]
fn kipo_form_hwpx_reads_headings_table_and_standard_layout() {
    let input = fixture("kipo-form.hwpx");
    let (document, _, _) = hlt_core::load_generation_input(&input).unwrap();
    assert_eq!(document.source, "kipo-form.hwpx");
    assert_kipo_form(&document);

    let dir = tempfile::tempdir().unwrap();
    let hlt = dir.path().join("한글.HLT");
    hlt_core::save_generated_hlt_as_new(
        &input,
        &document,
        GenerationDialect::PatentSpecificationKo,
        &[],
        &hlt,
        WriteOptions::default(),
    )
    .unwrap();
    let (reopened, _, _) = hlt_core::load_generation_input(&hlt).unwrap();
    let body = paragraph(
        &reopened,
        "본 발명은 한글 파일 불러오기를 시험하는 장치에 관한 것이다.",
    );
    assert_eq!((body.align.as_str(), body.indent_raw), ("justified", 800));
    assert_eq!(reopened.tables.len(), 1);
}

#[test]
fn binary_hwp_takes_the_same_path() {
    let bytes = std::fs::read(fixture("kipo-form.hwpx")).unwrap();
    let parsed = rhwp::parse_document(&bytes).unwrap();
    let hwp = rhwp::serialize_document(&parsed).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("명세서.hwp");
    std::fs::write(&input, hwp).unwrap();
    let (document, _, _) = hlt_core::load_generation_input(&input).unwrap();
    assert_eq!(document.source, "명세서.hwp");
    assert_kipo_form(&document);
}

#[test]
fn hwpx_figure_is_carried_with_its_picture_bytes() {
    let input = fixture("kipo-figure.hwpx");
    let (document, payloads, _) = hlt_core::load_generation_input(&input).unwrap();
    let figure = document
        .paragraphs
        .iter()
        .position(|p| p.text == "【도 1】")
        .unwrap();
    let picture = &document.paragraphs[figure + 1];
    assert_eq!(picture.image_ids.len(), 1);
    let image = document
        .images
        .iter()
        .find(|image| image.id == picture.image_ids[0])
        .unwrap();
    assert!(image.width.is_some() && image.height.is_some());
    let payload = payloads
        .iter()
        .find(|payload| payload.image_id == image.id)
        .unwrap();
    image::load_from_memory(&payload.bytes).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let hlt = dir.path().join("그림.HLT");
    hlt_core::save_generated_hlt_as_new(
        &input,
        &document,
        GenerationDialect::PatentSpecificationKo,
        &payloads,
        &hlt,
        WriteOptions::default(),
    )
    .unwrap();
}

#[test]
fn unreadable_hwp_is_refused_with_a_reason() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("깨짐.hwp");
    std::fs::write(&input, b"not a hangul document").unwrap();
    let error = hlt_core::load_generation_input(&input).unwrap_err();
    assert_eq!(error.code, hlt_core::ErrorCode::UnsupportedFormat);
    assert_eq!(
        error.details.get("reason").map(String::as_str),
        Some("hwp_unreadable")
    );
}
