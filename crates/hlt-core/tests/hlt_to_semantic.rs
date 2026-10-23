//! HLT → 의미 모델 역변환. 이것이 있어야 이미 받은 HLT 안의 표를 고칠 수 있다.
//! 바이트 동일은 목표가 아니다. 통특기 저장본조차 최소 변경이 아니기 때문이다.
//! 받아들일 조건은 **다시 구운 파일을 판독한 값이 원본 판독값과 맞는 것**이다.

mod support;

use hlt_core::{
    GenerationDialect, InspectOptions, WriteOptions, hml_document_from_hlt, inspect_path,
    save_generated_hlt_as_new,
};
use std::path::{Path, PathBuf};

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

#[test]
fn recovers_the_semantic_model_of_a_real_domestic_specification() {
    let fixture = support::domestic_specification_fixture();
    let source = inspect_path(
        fixture.path(),
        InspectOptions {
            include_image_payloads: true,
            ..InspectOptions::default()
        },
    )
    .unwrap();
    let (document, payloads) = hml_document_from_hlt(fixture.path()).unwrap();

    // 절, 문단, 표, 셀의 수가 판독값과 같아야 한다.
    assert_eq!(document.sections.len(), source.sections.len());
    // 판독기의 마지막 문단은 본문이 `0x15` 줄바꿈으로 끝나서 생기는 파싱 부산물이다.
    // 역변환기는 그것을 안 싣는다. 실으면 다시 구울 때마다 빈 문단이 하나씩 쌓인다.
    assert_eq!(document.paragraphs.len() + 1, source.paragraphs.len());
    assert!(source.paragraphs.last().unwrap().text.is_empty());
    assert_eq!(document.tables.len(), source.tables.len());
    assert_eq!(payloads.len(), source.image_payloads.len());

    // 표제 문단은 marker 를 달고, 표 문단은 table_id 를 단다.
    let markers = document
        .paragraphs
        .iter()
        .filter(|p| p.marker.is_some())
        .count();
    assert!(markers >= source.sections.len());
    let anchored = document
        .paragraphs
        .iter()
        .filter(|p| p.table_id.is_some())
        .count();
    assert_eq!(anchored, source.tables.len());
}

#[test]
#[ignore = "실물 work/pct_a 필요(공개 저장소에 없음 · 있으면 --ignored 로)"]
fn a_rebuilt_file_reads_back_with_the_same_geometry() {
    let source_path = repo("work/pct_a/diagnostics/pct_a_source.HLT");
    let before = inspect_path(&source_path, InspectOptions::default()).unwrap();
    let (document, payloads) = hml_document_from_hlt(&source_path).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("roundtrip.HLT");
    save_generated_hlt_as_new(
        &source_path,
        &document,
        GenerationDialect::PctPatent,
        &payloads,
        &output,
        WriteOptions::default(),
    )
    .unwrap();
    let after = inspect_path(&output, InspectOptions::default()).unwrap();

    assert_eq!(after.summary.table_count, before.summary.table_count);
    assert_eq!(after.summary.cell_count, before.summary.cell_count);
    assert_eq!(after.summary.image_count, before.summary.image_count);
    assert_eq!(
        after.cells.iter().map(|c| c.width).collect::<Vec<_>>(),
        before.cells.iter().map(|c| c.width).collect::<Vec<_>>()
    );
    assert_eq!(
        after.cells.iter().map(|c| c.start).collect::<Vec<_>>(),
        before.cells.iter().map(|c| c.start).collect::<Vec<_>>()
    );
}

/// 왕복 게이트는 표 기하까지만 보고 있었다. 글자 크기와 문단 서식도 살아남아야 한다.
/// 크기를 잃으면 표 셀 10pt 가 12pt 로 눌리고, 서식을 잃으면 기울임과 위첨자가 사라진다.
#[test]
#[ignore = "실물 work/pct_a 필요(공개 저장소에 없음 · 있으면 --ignored 로)"]
fn a_rebuilt_file_keeps_its_point_sizes_and_character_formats() {
    let source_path = repo("work/pct_a/diagnostics/pct_a_source.HLT");
    let before = hlt_core::inspect_format_inventory(&source_path).unwrap();
    let (document, payloads) = hml_document_from_hlt(&source_path).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("roundtrip.HLT");
    save_generated_hlt_as_new(
        &source_path,
        &document,
        GenerationDialect::PctPatent,
        &payloads,
        &output,
        WriteOptions::default(),
    )
    .unwrap();
    let after = hlt_core::inspect_format_inventory(&output).unwrap();

    let formats = |inventory: &hlt_core::FormatInventory| {
        let mut counted: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for paragraph in &inventory.paragraphs {
            for span in &paragraph.spans {
                for format in &span.formats {
                    *counted.entry(format!("{format:?}")).or_default() +=
                        (span.end - span.start) as usize;
                }
            }
        }
        counted
    };
    assert_eq!(formats(&after), formats(&before), "문자 서식 글자수");

    let sizes = |inventory: &hlt_core::FormatInventory| {
        let mut counted: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
        for paragraph in &inventory.paragraphs {
            for run in &paragraph.sizes {
                *counted.entry(run.size).or_default() += (run.end - run.start) as usize;
            }
        }
        counted
    };
    assert_eq!(sizes(&after), sizes(&before), "글자 크기별 글자수");
}

/// 문단 글자 자체가 한 자도 안 바뀌어야 한다.
/// ⛔ 앞머리 탭과 꼬리 공백이 함정이다. 판독기의 `ParagraphRecord.text` 는 앞뒤 ASCII 공백을
/// 잘라내고, `FormatParagraph.text` 는 탭과 페이지브레이크를 안 싣는다. 어느 한쪽만 믿으면 샌다.
#[test]
#[ignore = "실물 work/pct_a 필요(공개 저장소에 없음 · 있으면 --ignored 로)"]
fn a_rebuilt_file_keeps_every_paragraph_character() {
    let source_path = repo("work/pct_a/diagnostics/pct_a_source.HLT");
    let before = inspect_path(&source_path, InspectOptions::default()).unwrap();
    let (document, payloads) = hml_document_from_hlt(&source_path).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("roundtrip.HLT");
    save_generated_hlt_as_new(
        &source_path,
        &document,
        GenerationDialect::PctPatent,
        &payloads,
        &output,
        WriteOptions::default(),
    )
    .unwrap();
    let after = inspect_path(&output, InspectOptions::default()).unwrap();

    let differing: Vec<&str> = before
        .paragraphs
        .iter()
        .zip(&after.paragraphs)
        .filter(|(a, b)| a.text != b.text)
        .map(|(a, _)| a.id.as_str())
        .collect();
    assert!(differing.is_empty(), "글자가 달라진 문단 {differing:?}");
    assert_eq!(before.paragraphs.len(), 562);
}

/// 줄나눔과 문단 수. 통특기는 본문 문단을 다시 안 흘리므로 원본의 줄나눔이
/// 그대로 화면에 남는다. 즉 왕복이 줄을 잃으면 문서 모양이 달라진 채로 굳는다.
/// ⛔ 폭으로 다시 계산해서 맞출 수 없다. 실측(1,845개 줄 경계)으로 균일 폭 모델이
/// 모순임을 확인했다: 다음 글자가 한글인 경계만 봐도 줄 폭 최대가 39.5, 최소가 33.0 이라
/// 한계값 L 이 39.5 이상이면서 34.0 미만이어야 한다. 진짜 규칙은 비례 폰트 메트릭이다.
/// 그래서 원본 줄나눔을 **계산하지 않고 옮긴다.**
#[test]
#[ignore = "실물 work/pct_a 필요(공개 저장소에 없음 · 있으면 --ignored 로)"]
fn a_rebuilt_file_keeps_the_source_line_breaks() {
    let source_path = repo("work/pct_a/diagnostics/pct_a_source.HLT");
    let before = inspect_path(&source_path, InspectOptions::default()).unwrap();
    let (document, payloads) = hml_document_from_hlt(&source_path).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("roundtrip.HLT");
    save_generated_hlt_as_new(
        &source_path,
        &document,
        GenerationDialect::PctPatent,
        &payloads,
        &output,
        WriteOptions::default(),
    )
    .unwrap();
    let after = inspect_path(&output, InspectOptions::default()).unwrap();

    assert_eq!(after.paragraphs.len(), before.paragraphs.len(), "문단 수");
    assert_eq!(after.lines.len(), before.lines.len(), "줄 수");
}

/// 국내 왕복도 줄 수를 지켜야 한다. PCT 만 재고 있었다.
///
/// ⛔ 실측(2026-08-27): 편집 없이 다시 구운 국내 명세서가 줄 866 에서 **868** 로 늘었다.
/// `【청구범위】` 와 `【요약서】` 앞에 **없던 페이지브레이크 줄을 생성기가 만들었다.**
/// 표제 이름으로 하드코딩된 규칙인데, 그 국내 정본에는 `0x0c` 가 하나도 없다.
/// 규칙 자체는 원본 줄나눔을 모르는 입력(`.docx`, `.hml`, KIPO XML)에 필요하므로,
/// **원본을 아는 경우**(`source_line_bytes` 가 있는 문단)에만 끄는 것이 맞다.
#[test]
fn a_rebuilt_domestic_specification_keeps_its_line_count() {
    let fixture = support::application_table_fixture();
    let before = inspect_path(fixture.path(), InspectOptions::default()).unwrap();
    let (document, payloads) = hml_document_from_hlt(fixture.path()).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("roundtrip.HLT");
    save_generated_hlt_as_new(
        fixture.path(),
        &document,
        GenerationDialect::PatentSpecificationKo,
        &payloads,
        &output,
        WriteOptions::default(),
    )
    .unwrap();
    let after = inspect_path(&output, InspectOptions::default()).unwrap();

    // 국내도 【청구범위】·【요약서】 앞에서 쪽을 나눈다(사용자 결정 2026-10-16): 866 → 868, 늘어난 것은 그 두 줄뿐.
    let breaks = support::page_break_lines(&after);
    assert_eq!(breaks.len(), 2, "쪽나눔 줄 = 【청구범위】·【요약서】 앞 둘");
    assert_eq!(
        after.lines.len(),
        before.lines.len() + breaks.len(),
        "줄 수"
    );
    assert_eq!(after.paragraphs.len(), before.paragraphs.len(), "문단 수");
}
