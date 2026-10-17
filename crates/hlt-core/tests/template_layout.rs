//! 「새 문서」 레이아웃 규칙의 양성 대조 — 통특기가 저장한 문서(저장소 픽스처)는 R17 의 고정점이어야 한다.

mod support;

use hlt_core::{DocumentRegions, document_regions, inflate_hlt, new_document_from_template};

/// (행 높이들, 칸 높이들). 행 표 = 셀 표 바로 앞 `bf n` + 80B(+20 높이) · 셀 표 80B(+48 높이).
fn heights(inflated: &[u8], regions: &DocumentRegions) -> (Vec<u32>, Vec<u32>) {
    let u = |at: usize| u32::from_le_bytes(inflated[at..at + 4].try_into().unwrap());
    let cells = u(regions.cell_table + 1) as usize;
    let rows = (0..)
        .find(|n| {
            let at = regions.cell_table - 5 - 80 * n;
            inflated[at] == 0xbf && u(at + 1) as usize == *n
        })
        .unwrap();
    let row_table = regions.cell_table - 5 - 80 * rows;
    (
        (0..rows).map(|r| u(row_table + 5 + 80 * r + 20)).collect(),
        (0..cells)
            .map(|c| u(regions.cell_table + 5 + 80 * c + 48))
            .collect(),
    )
}

#[test]
fn nkeditor_saved_table_heights_are_a_fixed_point() {
    let fixture = support::pct_table_fixture();
    let (header, source) = inflate_hlt(fixture.path()).unwrap();
    let before = heights(&source, &document_regions(&header, &source).unwrap());
    // 12pt · 줄 간격 280 칸인데도 한 줄 칸 101 · 첫·끝 행 104 · 행 하한 117 — 서식(10pt)과 같은 식.
    assert_eq!(before.0[1..9], [117; 8]);
    assert_eq!(before.1[1..3], [104, 104]);
    assert_eq!(before.1[3..15], [101; 12]);

    let new = new_document_from_template(fixture.path()).unwrap();
    let after = heights(
        &new.inflated,
        &document_regions(&new.header, &new.inflated).unwrap(),
    );
    assert_eq!(after, before);
}
