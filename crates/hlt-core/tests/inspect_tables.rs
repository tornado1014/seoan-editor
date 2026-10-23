use std::{
    io::Write,
    path::{Path, PathBuf},
};

use hlt_core::{InspectOptions, inspect_path};

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

fn hex_fixture(encoded: &str) -> tempfile::NamedTempFile {
    let digits: Vec<u8> = encoded
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    assert_eq!(digits.len() % 2, 0);
    let bytes: Vec<u8> = digits
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).expect("fixture hex");
            u8::from_str_radix(text, 16).expect("fixture byte")
        })
        .collect();
    let mut file = tempfile::NamedTempFile::new().expect("temporary HLT");
    file.write_all(&bytes).expect("write fixture");
    file
}

fn pct_table_fixture() -> tempfile::NamedTempFile {
    hex_fixture(include_str!(
        "../../../fixtures/phase2/pct_table_probe.HLT.hex"
    ))
}

/// 통특기가 빈 명세서에서 새로 만든 표. 여섯 행 모두 본문이 비어 있다.
/// 저장본 tail 의 셀 테이블 field[1] 이 행 번호 1~6 을 명시하므로 여섯 행이 정답이다.
fn nk_new_table_fixture() -> tempfile::NamedTempFile {
    hex_fixture(include_str!(
        "../../../fixtures/phase2/NK_new_table_probe.HLT.hex"
    ))
}

/// 세로병합 표시는 셀 레코드 field[16] 의 **비트 16**이다. 값 전체를 보면 안 된다.
/// 실측: `align_a.HLT` 는 32/56 을 쓰고 통특기 v6.3 이 새로 만든 `표.HLT` 는 0/16 을 쓴다.
/// 56 과 16 은 둘 다 비트 16 을 품고, 32 와 0 은 안 품는다.
#[test]
fn reads_vertical_merges_in_both_flag_dialects() {
    let fixture = nk_new_table_fixture();
    let document = inspect_path(fixture.path(), InspectOptions::default()).unwrap();

    // 이어짐 칸 둘이 각각 주 셀로 접히므로 15 가 아니라 13 이다.
    assert_eq!(document.cells.len(), 13);
    let merged: Vec<(u32, u32, u32)> = document
        .cells
        .iter()
        .filter(|cell| cell.rowspan > 1)
        .map(|cell| (cell.row, cell.start, cell.rowspan))
        .collect();
    assert_eq!(merged, [(2, 6_408, 2), (4, 0, 2)]);
    let per_row: Vec<usize> = (0..6)
        .map(|row| document.cells.iter().filter(|cell| cell.row == row).count())
        .collect();
    assert_eq!(per_row, [2, 2, 3, 3, 2, 1]);
}

#[test]
fn keeps_a_trailing_all_empty_table_row() {
    let fixture = nk_new_table_fixture();
    let document = inspect_path(fixture.path(), InspectOptions::default()).unwrap();

    // 이 테스트가 지키는 것은 행 수다. 칸 수와 세로병합은
    // reads_vertical_merges_in_both_flag_dialects 가 본다.
    assert_eq!(document.tables.len(), 1);
    assert_eq!(document.tables[0].row_count, 6);
}

#[test]
fn inspects_pct_table_tail_and_hidden_boundary_cells() {
    let fixture = pct_table_fixture();
    let document = inspect_path(fixture.path(), InspectOptions::default()).unwrap();

    assert_eq!(document.summary.section_count, 3);
    assert_eq!(document.summary.line_count, 29);
    assert_eq!(document.summary.span_count, 29);
    // 표본 실측: `shape_count` 22, 곧 색인 1~21 의 21개다. 예전 기대값 18 은
    // 정렬 상위 비트(레코드 19, 20, 21 의 34816, 33792)를 「테이블 끝」으로 보고
    // 자르던 자리였다(2026-08-27 수리).
    assert_eq!(document.shapes.len(), 21);
    assert_eq!(document.tables.len(), 1);
    assert_eq!(document.cells.len(), 16);
    assert_eq!(
        (
            document.tables[0].row_count,
            document.tables[0].column_count
        ),
        (8, 2)
    );
    assert_eq!(
        document.summary.fixed_blocks.shapes_sha256,
        "4FA4FCC33F2AAEF3137F96261999F7A27EDA5377AA0307C7CEEBE2BDB1145EC0"
    );
    assert_eq!(
        document.summary.fixed_blocks.shape_order_sha256,
        "64B4746DBFFC7F078F718945B70A823A03DC6A966050ED43593E87D55288C4F8"
    );
    assert_eq!(
        document.summary.fixed_blocks.style_table_sha256,
        "2C10EEAB1F6578F30F4F858320E498C65D163E2DE2655394DB0F06A41A866DDC"
    );
}

#[test]
fn exposes_two_tables_and_literal_cell_geometry() {
    // 행 수는 파일마다 다르다. align_a 는 통특기 저장본이고, pct_a_full_v5 는 파이썬 세대
    // 생성 후보라 표마다 빈 행이 하나씩 더 붙어 있다(실측: tail 셀 테이블에 행 17, 21 이
    // 높이 104 와 실제 줄 번호를 가지고 존재한다). 그 차이를 파서가 감추지 않는다.
    let cases = [
        (
            "fixtures/align/align_a.HLT",
            [2987, 4063, 1246],
            [2, 1, 1],
            16,
            3,
        ),
        (
            "work/pct_a/pct_a_full_v5.HLT",
            [3696, 5021, 1543],
            [1, 1, 1],
            17,
            4,
        ),
    ];
    for (relative, widths, rowspans, first_rows, second_rows) in cases {
        let document = inspect_path(&repo(relative), InspectOptions::default()).unwrap();
        assert_eq!(document.tables.len(), 2);
        assert_eq!(document.tables[0].id, "table:0");
        assert_eq!(
            (
                document.tables[0].row_count,
                document.tables[0].column_count
            ),
            (first_rows, 5)
        );
        assert_eq!(
            (
                document.tables[1].row_count,
                document.tables[1].column_count
            ),
            (second_rows, 3)
        );
        assert_eq!(
            document.tables[0].anchor_section_id.as_deref(),
            Some("section:9")
        );
        assert_eq!(
            document.tables[1].anchor_section_id.as_deref(),
            Some("section:10")
        );

        let first_row: Vec<_> = document.tables[0]
            .cell_ids
            .iter()
            .filter_map(|id| document.cells.iter().find(|cell| &cell.id == id))
            .filter(|cell| cell.row == 0)
            .collect();
        assert_eq!(first_row.len(), 3);
        assert_eq!(
            first_row.iter().map(|cell| cell.width).collect::<Vec<_>>(),
            widths
        );
        assert_eq!(
            first_row
                .iter()
                .map(|cell| cell.colspan)
                .collect::<Vec<_>>(),
            [1, 3, 1]
        );
        assert_eq!(
            first_row
                .iter()
                .map(|cell| cell.rowspan)
                .collect::<Vec<_>>(),
            rowspans
        );
    }
}

#[test]
fn table_cells_have_stable_ids_ranges_and_line_links() {
    let document = inspect_path(
        &repo("work/pct_a/pct_a_full_v5.HLT"),
        InspectOptions::default(),
    )
    .unwrap();
    // 87 이 아니라 95 다. 표 1 의 17행, 표 2 의 4행이 실제로 파일에 들어 있다.
    assert_eq!(document.cells.len(), 95);
    for cell in &document.cells {
        let table_number = cell.table_id.strip_prefix("table:").unwrap();
        assert!(cell.id.starts_with(&format!("cell:{table_number}:")));
        assert!(cell.source_range.start_byte < cell.source_range.end_byte);
        assert!(!cell.line_ids.is_empty());
    }
}

/// 줄별 문단모양. 줄 배열 12바이트 레코드의 첫 워드가 문단모양 색인이다(실측).
/// 판독기는 문단의 **첫 줄만** 읽고 있었는데, 표 셀은 칸마다 다른 모양을 쓸 수 있어서
/// 역변환기가 셀의 정렬, 들여쓰기, 줄간격을 되살리려면 줄마다 필요하다.
/// 네이티브 PCT 저장본 실측: 표 셀 줄은 전부 `shape:14`(justified) 다.
#[test]
#[ignore = "실물 work/pct_a 필요(공개 저장소에 없음 · 있으면 --ignored 로)"]
fn every_line_reports_its_paragraph_shape() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("work/pct_a/diagnostics/pct_a_source.HLT");
    let document = hlt_core::inspect_path(&path, hlt_core::InspectOptions::default()).unwrap();

    let cell_lines: Vec<&str> = document
        .cells
        .iter()
        .flat_map(|cell| cell.line_ids.iter())
        .map(String::as_str)
        .collect();
    assert_eq!(cell_lines.len(), 16);
    for id in cell_lines {
        let line = document.lines.iter().find(|line| line.id == id).unwrap();
        assert_eq!(line.shape_id.as_deref(), Some("shape:14"), "{id}");
    }
}
