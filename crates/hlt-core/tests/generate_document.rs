use hlt_core::{GenerationDialect, generate_hlt_document, inspect_hml_path};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// ⛔ 국내 헤더는 **옛 파이썬 생성기와 일부러 갈라진다.** 그것은 내장 헤더를 그대로 써서
/// 문서 종류에 맨 140 을 쓰는데, 실물 국내 출원서는 `0x8000` 표시를 달고
/// 국내 출원서 44건이 쓰는 값은 32908 이다(전수).
/// 압축 스트림(`inflated`)은 여전히 바이트 동일이다. 갈라진 것은 헤더 `0x20` 네 바이트뿐이다.
///
/// 2026-08-27 에 문단모양 8, 10, 11 의 줄 높이를 정본 `domestic_출원서.HLT` 값으로
/// 옮기면서 이 해시가 바뀌었다. 바뀐 것은 문단모양 테이블의 **네 워드**뿐이고
/// (오프셋 608, 740, 752, 824).
#[test]
fn assembles_counterpart_inflated_bytes_exactly() {
    assert_generated(
        "generate_counterpart.hml",
        GenerationDialect::PatentSpecificationKo,
        // 2026-09-06 밤: 모양 12(표 안 가운데, 실물 center_cell.HLT)를 자산에 붙여 72바이트가
        // 늘었다.
        // 2026-09-07: 스타일 테이블을 실물에서 다시 떠 해시가 바뀌었다(길이 불변).
        4_842,
        "6A5D4E413C574CA8236C1581088ECD045F342C1B0BCE388592F97DE8898D1101",
        "4A9D966276E342F624840AC5F0F2D31E886D82C68CA86D7590A213760D95A962",
    );
}

#[test]
fn assembles_pct_table_inflated_bytes_exactly() {
    assert_generated(
        "generate_pct.hml",
        GenerationDialect::PctPatent,
        // 2026-09-06 밤: PCT 모양 22(표 안 가운데)를 붙여 72바이트 늘었다.
        // 2026-10-15: 가운데 식별항목이 모양 8(가운데 · 800)에서 모양 3(가운데 · 0)으로.
        // 통특기 PCT 저장본 20여 벌은 전부 (0, 280).
        6_840,
        "8F20564805B8DA2598B9E0E3BA4D25C290940016706A05B5C81827ECCADECECF",
        "7793CFECB9B769368431B590B33AC63D9A3AA074841E76B8E20D14AF6507E4BA",
    );
}

fn assert_generated(
    fixture: &str,
    dialect: GenerationDialect,
    inflated_bytes: usize,
    header_sha256: &str,
    inflated_sha256: &str,
) {
    let document = inspect_hml_path(&fixture_path(fixture)).unwrap();
    let generated = generate_hlt_document(&document, dialect, &[]).unwrap();

    assert_eq!(generated.inflated.len(), inflated_bytes);
    assert_eq!(hex(&Sha256::digest(generated.header)), header_sha256);
    assert_eq!(hex(&Sha256::digest(&generated.inflated)), inflated_sha256);
    assert_eq!(generated.line_count, generated.span_count);
    assert_eq!(generated.section_count, document.sections.len() as u32);
    assert_eq!(generated.image_count, 0);
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|value| format!("{value:02X}")).collect()
}
