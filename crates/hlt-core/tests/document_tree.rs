mod support;

use std::io::{Seek, SeekFrom, Write};

use hlt_core::{DocumentTreeKind, InspectOptions, inspect_path};

#[test]
fn pct_header_type_exposes_the_nkeditor_pct_tree_root() {
    let source = support::pct_table_fixture();
    let summary = inspect_path(source.path(), InspectOptions::default())
        .unwrap()
        .summary;

    assert_eq!(summary.document_title, "특허 출원서의 명세서등 서식");
    assert_eq!(summary.document_type_raw, 32_881);
    assert_eq!(summary.document_tree_kind, DocumentTreeKind::PctPatent);
    assert_eq!(summary.document_tree_label, "PCT특허");
}

#[test]
/// 표시(`0x8000`)가 없는 맨 140 도 국내로 읽어야 한다. 우리 옛 생성기가 그 값을 썼으므로
/// 그렇게 구워진 파일이 밖에 남아 있다. 실물 통특기 파일은 32908 을 쓴다(아래 테스트).
fn the_same_pct_title_with_type_140_is_the_korean_specification_tree() {
    let source = support::pct_table_fixture();
    let mut patched = tempfile::Builder::new().suffix(".HLT").tempfile().unwrap();
    let bytes = std::fs::read(source.path()).unwrap();
    patched.write_all(&bytes).unwrap();
    patched.seek(SeekFrom::Start(0x20)).unwrap();
    patched.write_all(&140_u32.to_le_bytes()).unwrap();
    patched.flush().unwrap();

    let summary = inspect_path(patched.path(), InspectOptions::default())
        .unwrap()
        .summary;

    assert_eq!(summary.document_title, "특허 출원서의 명세서등 서식");
    assert_eq!(summary.document_type_raw, 140);
    assert_eq!(
        summary.document_tree_kind,
        DocumentTreeKind::PatentSpecificationKo
    );
    assert_eq!(summary.document_tree_label, "특허 명세서등(국어)");
}

#[test]
fn an_unknown_header_type_is_retained_without_guessing_a_tree() {
    let source = support::pct_table_fixture();
    let mut patched = tempfile::Builder::new().suffix(".HLT").tempfile().unwrap();
    let bytes = std::fs::read(source.path()).unwrap();
    patched.write_all(&bytes).unwrap();
    patched.seek(SeekFrom::Start(0x20)).unwrap();
    patched.write_all(&0xDEAD_BEEF_u32.to_le_bytes()).unwrap();
    patched.flush().unwrap();

    let summary = inspect_path(patched.path(), InspectOptions::default())
        .unwrap()
        .summary;

    assert_eq!(summary.document_type_raw, 0xDEAD_BEEF);
    assert_eq!(summary.document_tree_kind, DocumentTreeKind::Unknown);
    assert_eq!(summary.document_tree_label, "unknown");
}

/// 실물 국내 출원서의 헤더 종류는 **32908** 이다. 우리가 알던 140 이 아니다.
///
/// 실물 190건 전수(2026-08-27): 32908 이 44건, 32879 가 42건, 32881 이 30건, 7 이 74건이고
/// **140 은 0건**이다. 140 은 우리가 만든 탐침 파일과 이 파일 위쪽의 합성 테스트에만 있다.
///
/// 값을 분해하면 규칙이 보인다. 32908 = `0x8000 + 140`, 32881 = `0x8000 + 113`,
/// 32879 = `0x8000 + 111`. 낮은 15비트가 서식 번호고 `0x8000` 은 따로 붙는 표시다.
/// 그래서 판정은 `0x8000` 을 떼고 한다. 떼기 전에는 실물 190건 중 **86건(45%)이
/// `Unknown`** 으로 떨어졌다.
#[test]
fn a_real_domestic_application_header_is_the_korean_specification_tree() {
    let source = support::application_table_fixture();
    let summary = inspect_path(source.path(), InspectOptions::default())
        .unwrap()
        .summary;

    assert_eq!(summary.document_type_raw, 32_908);
    assert_eq!(
        summary.document_tree_kind,
        DocumentTreeKind::PatentSpecificationKo
    );
}

/// 우리가 구운 국내 명세서의 헤더 종류가 통특기와 같아야 한다.
///
/// ⛔ 실측: 실물 국내, PCT 출원서는 `0x8000` 표시를 단다(32908, 32879, 32881).
/// 서식 7(의견서)은 안 단다. 표시가 서식마다 다르므로 "전부 단다"로 일반화하지 마라.
/// 우리 PCT 생성은 32881 로 표시를 다는데 국내 생성만 맨 140 을 썼다. 내장 헤더가
/// 통특기 저장본이 아니라 우리 탐침 파일에서 온 탓이다.
/// 국내 출원서 실물 44건이 쓰는 값은 `0x8000 + 140` = **32908** 이다.
#[test]
fn a_generated_domestic_specification_carries_the_same_header_type_as_nkeditor() {
    use hlt_core::{
        GenerationDialect, WriteOptions, hml_document_from_hlt, save_generated_hlt_as_new,
    };

    let source = support::application_table_fixture();
    let (document, payloads) = hml_document_from_hlt(source.path()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("domestic.HLT");
    save_generated_hlt_as_new(
        source.path(),
        &document,
        GenerationDialect::PatentSpecificationKo,
        &payloads,
        &output,
        WriteOptions::default(),
    )
    .unwrap();

    let summary = inspect_path(&output, InspectOptions::default())
        .unwrap()
        .summary;
    assert_eq!(summary.document_type_raw, 32_908);
}
