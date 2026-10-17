use hlt_core::{
    GeneratedCellLineRange, GeneratedCellRecord, GeneratedLineEnding, GeneratedRowRecord,
    GeneratedTerminalCell, GenerationDialect, HmlCell, HmlDocument, HmlSummary, HmlTable,
    generate_cell_table, generate_font_table, generate_section_register, inspect_docx_path,
    plan_hlt_body, plan_table_tail,
};
use std::path::PathBuf;

mod support;

fn words(record: &[u8]) -> Vec<u32> {
    record
        .chunks_exact(4)
        .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
        .collect()
}

#[test]
fn maps_semantic_table_geometry_and_line_ranges_to_tail_records() {
    let mut document = HmlDocument {
        source: "x".into(),
        source_sha256: "0".repeat(64),
        encoding: "cp949".into(),
        title: "x".into(),
        fonts: vec![],
        summary: HmlSummary {
            section_count: 0,
            top_level_paragraph_count: 0,
            content_paragraph_count: 0,
            table_count: 1,
            row_count: 2,
            cell_count: 4,
            image_count: 0,
        },
        sections: vec![],
        paragraphs: vec![],
        tables: vec![HmlTable {
            id: "table:1".into(),
            occurrence: 1,
            paragraph_id: "paragraph:1".into(),
            max_columns: 2,
            row_count: 2,
            cell_ids: vec![
                "cell:1:1:1".into(),
                "cell:1:1:2".into(),
                "cell:1:2:1".into(),
                "cell:1:2:2".into(),
            ],
        }],
        cells: vec![],
        images: vec![],
    };
    document.cells = vec![
        // 폭은 `hwidth_raw`(KIPO XML colwidth) 로 준다. 네이티브는 통특기 불러오기 규칙
        // `colwidth_to_cell_width` 가 낸다: 2735 -> 2210, 2733 -> 2209.
        // ⛔ 2026-09-07 전에는 2188/2186 을 기대했다 — 그것은 정본 .fin 의 **내보내기** 쌍이고
        //    통특기 들여오기는 그 역이 아니다(format_rules 테스트 39쌍).
        cell("cell:1:1:1", 1, 1, Some(2735), 2, false),
        cell("cell:1:1:2", 1, 2, Some(2733), 1, false),
        cell("cell:1:2:1", 2, 1, Some(2735), 1, true),
        cell("cell:1:2:2", 2, 2, Some(2733), 1, false),
    ];
    let ranges = [
        GeneratedCellLineRange {
            cell_id: "cell:1:1:1".into(),
            line_start: 5,
            line_end: 5,
        },
        GeneratedCellLineRange {
            cell_id: "cell:1:1:2".into(),
            line_start: 6,
            line_end: 6,
        },
        GeneratedCellLineRange {
            cell_id: "cell:1:2:1".into(),
            line_start: 7,
            line_end: 7,
        },
        GeneratedCellLineRange {
            cell_id: "cell:1:2:2".into(),
            line_start: 8,
            line_end: 8,
        },
    ];
    let regular =
        plan_table_tail(&document, GenerationDialect::PatentSpecificationKo, &ranges).unwrap();
    assert_eq!(
        regular.rows,
        [
            GeneratedRowRecord {
                first_cell: 1,
                last_cell: 2,
                height: 117,
                table: 1
            },
            GeneratedRowRecord {
                first_cell: 3,
                last_cell: 4,
                height: 117,
                table: 1
            }
        ]
    );
    assert_eq!(
        (
            regular.cells[0].start,
            regular.cells[0].width,
            regular.cells[0].height,
            regular.cells[0].rowspan
        ),
        // 높이 221 = 가장자리 104 + 117. 서식 140 실측으로 가장자리는 98 이다.
        (0, 2210, 215, 2)
    );
    assert_eq!(
        (regular.cells[1].start, regular.cells[1].width),
        (2210, 2209)
    );
    assert_eq!(
        (regular.cells[2].height, regular.cells[2].span_flag),
        (0, 56)
    );
    // 국내 서식도 센티널을 단다(아래 `a_domestic_table_gets_a_sentinel_row_like_nkeditor`).
    // 안 달았더니 통특기가 마지막 실제 행을 표 종결자로 먹었다.
    assert_eq!(regular.terminal_rows, [(1, 5, 6)]);
    let pct = plan_table_tail(&document, GenerationDialect::PctPatent, &ranges).unwrap();
    assert_eq!(
        pct.cells.iter().map(|cell| cell.height).collect::<Vec<_>>(),
        [218, 101, 0, 101]
    );
    assert_eq!(pct.terminal_rows, [(1, 5, 6)]);
    assert_eq!(
        pct.terminal_cells,
        [
            GeneratedTerminalCell {
                row: 3,
                start: 1080,
                width: 2210
            },
            GeneratedTerminalCell {
                row: 3,
                start: 3290,
                width: 2209
            }
        ]
    );
}

#[test]
fn real_docx_table_generation_matches_native_hlt_cell_geometry_and_shapes() {
    let path = repo_root().join("work/pct_a/diagnostics/pct_a_source.docx");
    let document = inspect_docx_path(&path).unwrap().document;
    let body = plan_hlt_body(&document, GenerationDialect::PctPatent).unwrap();
    let table =
        plan_table_tail(&document, GenerationDialect::PctPatent, &body.cell_ranges).unwrap();

    assert_eq!(
        table
            .cells
            .chunks_exact(2)
            .map(|row| (row[0].width, row[1].width))
            .collect::<Vec<_>>(),
        vec![(3600, 3510); 8]
    );
    assert_eq!(
        table
            .cells
            .chunks_exact(2)
            .map(|row| (row[0].height, row[1].height))
            .collect::<Vec<_>>(),
        [
            (101, 101),
            (99, 99),
            (99, 99),
            (99, 99),
            (99, 99),
            (99, 99),
            (99, 99),
            (101, 101),
        ]
    );
    assert_eq!(
        body.lines
            .iter()
            .filter(|line| line.ending == GeneratedLineEnding::Cell)
            .map(|line| line.shape_index)
            .collect::<Vec<_>>(),
        vec![14; 16]
    );
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn cell(
    id: &str,
    row: u32,
    occurrence: u32,
    hwidth: Option<u32>,
    rowspan: u32,
    continuation: bool,
) -> HmlCell {
    HmlCell {
        id: id.into(),
        table_id: "table:1".into(),
        row,
        occurrence,
        text: String::new(),
        start_raw: None,
        width_raw: None,
        hwidth_raw: hwidth,
        column_begin: Some(occurrence),
        row_begin: Some(row),
        rowspan,
        colspan: 1,
        merge_continuation: continuation,
        align: "left".into(),
        indent_raw: 0,
        lspace: 280,
        image_ids: vec![],
        inlines: vec![],
    }
}

#[test]
fn generates_counterpart_row_and_cell_linked_lists_with_merge_fields() {
    let rows = [
        GeneratedRowRecord {
            first_cell: 1,
            last_cell: 2,
            height: 117,
            table: 1,
        },
        GeneratedRowRecord {
            first_cell: 3,
            last_cell: 4,
            height: 207,
            table: 1,
        },
        GeneratedRowRecord {
            first_cell: 5,
            last_cell: 5,
            height: 117,
            table: 2,
        },
    ];
    let row_bytes = generate_cell_table(&rows, &[], false).unwrap();
    assert_eq!(row_bytes.len(), 5 + 4 * 80);
    assert_eq!(row_bytes[0], 0xbf);
    assert_eq!(u32::from_le_bytes(row_bytes[1..5].try_into().unwrap()), 4);
    let r1 = words(&row_bytes[85..165]);
    let r2 = words(&row_bytes[165..245]);
    let r3 = words(&row_bytes[245..325]);
    assert_eq!(
        (r1[2], r1[3], r1[5], r1[6], r1[7]),
        (1, 2, 117, u32::MAX, 2)
    );
    assert_eq!(
        (r2[2], r2[3], r2[5], r2[6], r2[7]),
        (3, 4, 207, 1, u32::MAX)
    );
    assert_eq!(
        (r3[2], r3[3], r3[5], r3[6], r3[7]),
        (5, 5, 117, u32::MAX, u32::MAX)
    );

    let cells = [
        GeneratedCellRecord {
            row: 1,
            start: 0,
            width: 1000,
            line_start: 0,
            line_end: 0,
            height: 221,
            span_flag: 32,
            rowspan: 2,
            colspan: 1,
        },
        GeneratedCellRecord {
            row: 1,
            start: 1000,
            width: 2000,
            line_start: 1,
            line_end: 1,
            height: 104,
            span_flag: 32,
            rowspan: 1,
            colspan: 2,
        },
        GeneratedCellRecord {
            row: 2,
            start: 0,
            width: 1000,
            line_start: 2,
            line_end: 2,
            height: 0,
            span_flag: 56,
            rowspan: 1,
            colspan: 1,
        },
    ];
    let font = generate_font_table(&cells, &[], false).unwrap();
    assert_eq!(font.len(), 5 + 4 * 80);
    let c1 = words(&font[85..165]);
    let c2 = words(&font[165..245]);
    let c3 = words(&font[245..325]);
    assert_eq!(
        (c1[1], c1[2], c1[3], c1[4], c1[5]),
        (1, 2, u32::MAX, 0, 1000)
    );
    assert_eq!(
        (&c1[6..11], c1[11], c1[12], c1[16], c1[17], c1[18]),
        // 서식 140 상수. `(15,15,15,15,15,40)` 과 `f16=32` 는 의견서(111) 값이었다.
        (&[15, 5, 5, 5, 5][..], 15, 221, 0, 2, 1)
    );
    assert_eq!((c2[2], c2[3], c2[16], c2[18]), (u32::MAX, 1, 0, 2));
    assert_eq!((c3[2], c3[3], c3[12], c3[16]), (u32::MAX, u32::MAX, 0, 56));
}

#[test]
fn generates_native_pct_fields_and_hidden_terminal_boundaries() {
    let rows = [GeneratedRowRecord {
        first_cell: 1,
        last_cell: 2,
        height: 117,
        table: 1,
    }];
    let row_bytes = generate_cell_table(&rows, &[(1, 3, 4)], true).unwrap();
    assert_eq!(u32::from_le_bytes(row_bytes[1..5].try_into().unwrap()), 3);
    let boundary = words(&row_bytes[165..245]);
    assert_eq!(
        (
            boundary[2],
            boundary[3],
            boundary[5],
            boundary[6],
            boundary[7]
        ),
        (3, 4, 0, 1, u32::MAX)
    );
    assert_eq!((&boundary[8..16]), &[1080, 0, 108, 15, 10, 10, 10, 10]);

    let cells = [GeneratedCellRecord {
        row: 1,
        start: 500,
        width: 3000,
        line_start: 4,
        line_end: 4,
        height: 104,
        span_flag: 32,
        rowspan: 1,
        colspan: 1,
    }];
    let terminals = [GeneratedTerminalCell {
        row: 2,
        start: 500,
        width: 3000,
    }];
    let font = generate_font_table(&cells, &terminals, true).unwrap();
    let actual = words(&font[85..165]);
    let terminal = words(&font[165..245]);
    assert_eq!(
        (&actual[6..12], actual[16]),
        (&[15, 10, 10, 10, 10, 108][..], 0)
    );
    assert_eq!(
        (
            terminal[1],
            terminal[4],
            terminal[5],
            terminal[12],
            terminal[13],
            terminal[14],
            terminal[17],
            terminal[18]
        ),
        (2, 500, 3000, 0, u32::MAX, u32::MAX, 1, 1)
    );
}

#[test]
fn generates_builtin_section_and_next_line_wipo_register_slots() {
    let titles = [1, 0, 0, 2, 0];
    let wipo = [None, Some(1), None, None, Some(2)];
    let bytes = generate_section_register(&titles, &wipo, true).unwrap();
    assert_eq!(&bytes[..5], b"\xbf\0\0\0\0");
    assert_eq!(words(&bytes[5..]), [1, 1, 0, 0, 0, 0, 2, 2, 0]);
    let legacy = generate_section_register(&titles, &wipo, false).unwrap();
    assert_eq!(&legacy[..4], b"\xbf\0\0\0");
    assert_eq!(words(&legacy[4..]), [1, 1, 0, 0, 0, 0, 2, 2, 0]);
}

/// ⛔ **국내 서식도 표 뒤에 센티널 행이 붙는다.** PCT 만인 줄 알았다.
///
/// 사용자 통특기 판정(2026-08-27): 우리가 12행으로 구운 국내 표가 **11행까지만** 보였다.
/// 마지막 행이 잘렸다. 통특기가 우리 마지막 실제 행을 표 종결자로 먹은 것이다.
///
/// 실측 근거. 통특기가 만든 국내 파일 두 건에서 표 뒤에 **높이 0, 줄 번호 `0xFFFFFFFF`** 인
/// 칸 한 줄이 있고, 행 레코드도 높이 0 짜리가 하나 더 있다.
///
/// | 파일 | 센티널 start | 실제 start | 차이 |
/// | --- | --- | --- | ---: |
/// | `domestic_출원서`(표 2개 다) | 3, 2800, 5104 | 0, 2797, 5101 | +3 |
/// | `amend_a` | 1, 2779, 4967, 7153 | 0, 2778, 4966, 7152 | +1 |
/// | PCT | | | +1080 |
///
/// ⚠️ **차이값의 규칙은 아직 모른다.** 파일마다 다르고 열 수나 행 수와도 안 맞는다.
/// 실을 값은 국내 정본에서 잰 +3 을 쓴다. 렌더에 걸리는 것은 **센티널의 존재**이지
/// 이 오프셋이 아닌 것으로 보인다(PCT 가 +1080 으로도 통과한다).
#[test]
fn a_domestic_table_gets_a_sentinel_row_like_nkeditor() {
    let fixture = support::application_table_fixture();
    let (document, _) = hlt_core::hml_document_from_hlt(fixture.path()).unwrap();
    let body =
        hlt_core::plan_hlt_body(&document, GenerationDialect::PatentSpecificationKo).unwrap();
    let tail = plan_table_tail(
        &document,
        GenerationDialect::PatentSpecificationKo,
        &body.cell_ranges,
    )
    .unwrap();

    assert_eq!(tail.terminal_rows.len(), 1, "센티널 행 레코드");
    assert_eq!(tail.terminal_cells.len(), 3, "센티널 칸");
    let last_row: Vec<_> = tail.cells.iter().rev().take(3).collect();
    for (sentinel, real) in tail.terminal_cells.iter().zip(last_row.iter().rev()) {
        assert_eq!(sentinel.start, real.start + 3, "센티널 start");
        assert_eq!(sentinel.width, real.width, "센티널 폭");
    }
}
