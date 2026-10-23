mod support;

use hlt_core::{CharacterFormat, inspect_format_inventory};
use std::path::Path;

const FIXTURE_SIZE: u64 = 1_102;
const FIXTURE_SHA256: &str = "61DB46D72C1D8EAB18FAE81D8E70C66DE2E625A842BD655AAA2E8C787465711D";

#[test]
fn exposes_runtime_character_attributes_without_sibling_xml() {
    let source = support::formatted_style_fixture();
    assert_eq!(source.as_file().metadata().unwrap().len(), FIXTURE_SIZE);

    let inventory = inspect_format_inventory(source.path()).unwrap();
    assert_eq!(inventory.source_sha256, FIXTURE_SHA256);
    let paragraph = inventory
        .paragraphs
        .iter()
        .find(|value| value.paragraph_id == "paragraph:1")
        .unwrap();
    assert_eq!(paragraph.text, "Abold both H2O");
    assert!(!paragraph.spans.iter().any(|span| span.start == 0));
    assert_eq!(paragraph.spans.len(), 3);
    assert_eq!(
        paragraph
            .spans
            .iter()
            .map(|span| (span.start, span.end, span.raw_code, span.raw_attr))
            .collect::<Vec<_>>(),
        vec![(1, 6, 2, 2), (6, 10, 7, 6), (12, 13, 5, 32)]
    );
    assert_eq!(
        paragraph
            .spans
            .iter()
            .find(|span| span.text == "bold ")
            .unwrap()
            .formats,
        vec![CharacterFormat::Bold]
    );
    assert_eq!(
        paragraph
            .spans
            .iter()
            .find(|span| span.text == "both")
            .unwrap()
            .formats,
        vec![CharacterFormat::Bold, CharacterFormat::Italic]
    );
    assert_eq!(
        paragraph
            .spans
            .iter()
            .find(|span| span.text == "2")
            .unwrap()
            .formats,
        vec![CharacterFormat::Subscript]
    );
    assert!(inventory.unresolved.is_empty());
}

#[test]
#[ignore = "실물 work/pct_a 필요(공개 저장소에 없음 · 있으면 --ignored 로)"]
fn reads_formats_from_the_native_pct_file_with_formatted_paragraph_marks() {
    // 네이티브 PCT 저장본 실측: 문단 5('본 출원은 …')의 종결자 몫 슬롯이 0 이 아니라
    // 코드 2(밑줄)다. 앞 문단의 밑줄 표제가 문단 부호까지 물들인 것으로, 통특기가 실제로
    // 저장하는 모습이다. 리더가 "종결자는 반드시 0" 을 강제하면 이 파일을 못 읽는다.
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("work/pct_a/diagnostics/pct_a_source.HLT");
    let inventory = inspect_format_inventory(&path).unwrap();

    let underlined: Vec<_> = inventory
        .paragraphs
        .iter()
        .flat_map(|paragraph| paragraph.spans.iter())
        .filter(|span| span.formats == [CharacterFormat::Underline])
        .map(|span| span.text.trim().to_string())
        .collect();
    assert!(
        underlined.iter().any(|text| text == "관련 출원"),
        "밑줄 표제를 못 읽었다: {underlined:?}"
    );
}

/// 글자 크기 축. 통특기 문자모양 레코드의 `+4` 가 포인트 크기다(실측).
/// 네이티브 저장본은 **표 셀이 10pt, 본문이 12pt** 이고
/// 같은 굵게(attr=2)가 본문에서 12, 표 셀에서 10 이다. 크기는 서식 종류로 정해지지 않는다.
/// 크기를 안 읽으면 역변환 왕복에서 표 셀 10pt 가 12pt 로 눌린다.
#[test]
#[ignore = "실물 work/pct_a 필요(공개 저장소에 없음 · 있으면 --ignored 로)"]
fn reports_the_point_size_of_every_run() {
    let inventory =
        inspect_format_inventory(Path::new("../../work/pct_a/diagnostics/pct_a_source.HLT"))
            .unwrap();

    let sizes = |paragraph_id: &str| -> Vec<(u32, u32, u32)> {
        inventory
            .paragraphs
            .iter()
            .find(|value| value.paragraph_id == paragraph_id)
            .map(|value| {
                value
                    .sizes
                    .iter()
                    .map(|run| (run.start, run.end, run.size))
                    .collect()
            })
            .unwrap_or_default()
    };

    // 표 문단. 셀 글자는 10pt 이고, 표 뒤 꼬리 본문은 12pt 다.
    let table = sizes("paragraph:147");
    assert!(!table.is_empty(), "표 문단에 크기 구간이 없다");
    assert_eq!(table[0].2, 10);
    assert_eq!(table.last().unwrap().2, 12);

    // 평범한 본문 문단은 통째로 12pt 다.
    let body = sizes("paragraph:81");
    assert_eq!(body.len(), 1);
    assert_eq!(body[0].2, 12);
}
