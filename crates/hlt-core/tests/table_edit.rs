//! 표 구조 편집. 의미층 연산과, 편집한 모델로 실제 HLT 를 다시 만드는 왕복을 함께 본다.

use std::path::{Path, PathBuf};

use hlt_core::{
    GenerationDialect, HmlDocument, InspectOptions, NEW_TABLE_TOTAL_WIDTH, TableEditDecisionStatus,
    TableEditSkipReason, TableOperation, WriteOptions, apply_table_edits, cell_width_to_colwidth,
    colwidth_to_cell_width, inspect_hml_path, inspect_path, plan_table_edits,
    save_generated_hlt_as_new,
};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/generate_pct.hml")
}

fn document() -> HmlDocument {
    inspect_hml_path(&fixture()).unwrap()
}

/// 편집한 모델을 실제로 HLT 로 굽고 다시 읽어 첫 표의 행, 열과 문서 전체 셀 수를 돌려준다.
fn regenerate(document: &HmlDocument, name: &str) -> (u32, u32, u64) {
    let inspected = regenerate_all(document, name);
    (inspected.0[0].0, inspected.0[0].1, inspected.1)
}

type TableShapes = (Vec<(u32, u32)>, u64);

fn regenerate_all(document: &HmlDocument, name: &str) -> TableShapes {
    let output_dir = tempfile::tempdir().unwrap();
    let output = output_dir.path().join(format!("{name}.HLT"));
    save_generated_hlt_as_new(
        &fixture(),
        document,
        GenerationDialect::PctPatent,
        &[],
        &output,
        WriteOptions::default(),
    )
    .unwrap();
    let inspected = inspect_path(Path::new(&output), InspectOptions::default()).unwrap();
    (
        inspected
            .tables
            .iter()
            .map(|table| (table.row_count, table.column_count))
            .collect(),
        inspected.summary.cell_count,
    )
}

fn table_id(document: &HmlDocument) -> String {
    document.tables[0].id.clone()
}

#[test]
fn the_fixture_starts_as_one_row_of_two_cells() {
    let document = document();
    assert_eq!(regenerate(&document, "baseline"), (1, 2, 2));
}

#[test]
fn inserts_a_row_and_bakes_it_into_a_new_hlt() {
    let mut document = document();
    let id = table_id(&document);
    let plan = apply_table_edits(
        &mut document,
        &[TableOperation::InsertRow {
            table_id: id,
            index: 1,
        }],
    )
    .unwrap();
    assert_eq!(plan.ready, 1);
    assert_eq!(plan.decisions[0].row_count, 2);
    assert_eq!(regenerate(&document, "inserted-row"), (2, 2, 4));
}

#[test]
fn deletes_a_row_only_while_more_than_one_remains() {
    let mut document = document();
    let id = table_id(&document);
    let plan = apply_table_edits(
        &mut document,
        &[
            TableOperation::InsertRow {
                table_id: id.clone(),
                index: 1,
            },
            TableOperation::DeleteRow {
                table_id: id.clone(),
                index: 1,
            },
            TableOperation::DeleteRow {
                table_id: id,
                index: 0,
            },
        ],
    )
    .unwrap();
    assert_eq!(plan.ready, 2);
    assert_eq!(plan.decisions[2].reason, Some(TableEditSkipReason::LastRow));
    assert_eq!(regenerate(&document, "deleted-row"), (1, 2, 2));
}

#[test]
fn inserting_and_deleting_a_column_preserves_the_total_width() {
    let mut document = document();
    let id = table_id(&document);
    let total = |document: &HmlDocument| -> u32 {
        document
            .cells
            .iter()
            .filter(|cell| cell.row == 1)
            .filter_map(|cell| cell.width_raw)
            .sum()
    };
    let before = total(&document);
    assert_eq!(before, 3_600);

    apply_table_edits(
        &mut document,
        &[TableOperation::InsertColumn {
            table_id: id.clone(),
            index: 1,
        }],
    )
    .unwrap();
    assert_eq!(total(&document), before);
    assert_eq!(regenerate(&document, "inserted-column"), (1, 3, 3));

    apply_table_edits(
        &mut document,
        &[TableOperation::DeleteColumn {
            table_id: id,
            index: 1,
        }],
    )
    .unwrap();
    assert_eq!(total(&document), before);
    assert_eq!(regenerate(&document, "deleted-column"), (1, 2, 2));
}

#[test]
fn merges_a_block_then_splits_it_back() {
    let mut document = document();
    let id = table_id(&document);
    apply_table_edits(
        &mut document,
        &[
            TableOperation::InsertRow {
                table_id: id.clone(),
                index: 1,
            },
            TableOperation::Merge {
                table_id: id.clone(),
                row: 0,
                column: 0,
                row_end: 1,
                column_end: 0,
            },
        ],
    )
    .unwrap();
    let master = document
        .cells
        .iter()
        .find(|cell| cell.row == 1 && cell.occurrence == 1)
        .unwrap();
    assert_eq!(master.rowspan, 2);
    assert!(
        document
            .cells
            .iter()
            .any(|cell| cell.row == 2 && cell.merge_continuation)
    );
    // 세로 병합은 본문 셀 하나로 읽힌다. 2행 2열에서 한 칸이 합쳐졌으니 셋이다.
    assert_eq!(regenerate(&document, "merged"), (2, 2, 3));

    apply_table_edits(
        &mut document,
        &[TableOperation::Split {
            table_id: id,
            row: 1,
            column: 0,
        }],
    )
    .unwrap();
    assert!(!document.cells.iter().any(|cell| cell.merge_continuation));
    assert_eq!(regenerate(&document, "split"), (2, 2, 4));
}

#[test]
fn refuses_a_merge_that_would_cut_an_existing_one() {
    let mut document = document();
    let id = table_id(&document);
    apply_table_edits(
        &mut document,
        &[
            TableOperation::InsertRow {
                table_id: id.clone(),
                index: 1,
            },
            TableOperation::Merge {
                table_id: id.clone(),
                row: 0,
                column: 0,
                row_end: 1,
                column_end: 0,
            },
        ],
    )
    .unwrap();
    let plan = apply_table_edits(
        &mut document,
        &[TableOperation::Merge {
            table_id: id,
            row: 1,
            column: 0,
            row_end: 1,
            column_end: 1,
        }],
    )
    .unwrap();
    assert_eq!(plan.ready, 0);
    assert_eq!(
        plan.decisions[0].reason,
        Some(TableEditSkipReason::MergeCrossesBoundary)
    );
}

#[test]
fn splitting_an_unmerged_cell_is_a_skip_not_an_error() {
    let mut document = document();
    let id = table_id(&document);
    let plan = apply_table_edits(
        &mut document,
        &[TableOperation::Split {
            table_id: id,
            row: 0,
            column: 0,
        }],
    )
    .unwrap();
    assert_eq!(plan.ready, 0);
    assert_eq!(
        plan.decisions[0].reason,
        Some(TableEditSkipReason::NotMerged)
    );
}

#[test]
fn creates_a_new_table_with_the_measured_default_widths() {
    let mut document = document();
    // 표가 붙지 않은 빈 문단을 고른다.
    let paragraph = document
        .paragraphs
        .iter()
        .find(|value| value.table_id.is_none() && value.text.trim().is_empty())
        .map(|value| value.id.clone());
    let paragraph = match paragraph {
        Some(id) => id,
        None => {
            let source = document.paragraphs.last().unwrap().clone();
            let mut blank = source.clone();
            blank.id = "paragraph:blank".to_string();
            blank.text = String::new();
            blank.tail_text = String::new();
            blank.inlines = Vec::new();
            blank.image_ids = Vec::new();
            blank.table_id = None;
            document.paragraphs.push(blank);
            "paragraph:blank".to_string()
        }
    };

    let plan = apply_table_edits(
        &mut document,
        &[TableOperation::NewTable {
            paragraph_id: paragraph,
            rows: 2,
            columns: 3,
        }],
    )
    .unwrap();
    assert_eq!(plan.ready, 1);
    let id = plan.decisions[0].table_id.clone().unwrap();
    let widths: Vec<u32> = document
        .cells
        .iter()
        .filter(|cell| cell.table_id == id && cell.row == 1)
        .filter_map(|cell| cell.width_raw)
        .collect();
    assert_eq!(widths, [2_848, 2_848, 2_848]);
    assert_eq!(widths.iter().sum::<u32>(), NEW_TABLE_TOTAL_WIDTH);
    assert_eq!(
        regenerate_all(&document, "new-table"),
        (vec![(1, 2), (2, 3)], 8)
    );
}

#[test]
fn planning_leaves_the_document_untouched() {
    let document = document();
    let id = table_id(&document);
    let before = document.clone();
    let plan = plan_table_edits(
        &document,
        &[TableOperation::InsertRow {
            table_id: id,
            index: 1,
        }],
    )
    .unwrap();
    assert_eq!(plan.decisions[0].status, TableEditDecisionStatus::Ready);
    assert_eq!(document.cells.len(), before.cells.len());
    assert_eq!(document.tables[0].row_count, before.tables[0].row_count);
}

#[test]
fn reports_a_missing_table_as_a_skip() {
    let mut document = document();
    let plan = apply_table_edits(
        &mut document,
        &[TableOperation::InsertRow {
            table_id: "table:404".to_string(),
            index: 0,
        }],
    )
    .unwrap();
    assert_eq!(plan.skipped, 1);
    assert_eq!(
        plan.decisions[0].reason,
        Some(TableEditSkipReason::TableNotFound)
    );
}

/// 세로 병합을 한 뒤 **다른 행**에 가로 병합을 걸면, 병합에 덮인 행의 글자가
/// 한 칸씩 오른쪽으로 밀렸다. 실물 PCT 표에서 사용자가 통특기로 확인한 결함이다.
/// `Grid::load` 가 이어짐 셀 자리를 주 셀 rowspan 과 이어짐 셀에서 두 번 세었다.
#[test]
fn a_later_merge_does_not_shift_the_text_of_a_vertically_covered_row() {
    let mut document = document();
    let id = table_id(&document);
    let plan = apply_table_edits(
        &mut document,
        &[
            TableOperation::InsertRow {
                table_id: id.clone(),
                index: 0,
            },
            TableOperation::InsertRow {
                table_id: id.clone(),
                index: 2,
            },
            TableOperation::InsertColumn {
                table_id: id.clone(),
                index: 2,
            },
            TableOperation::Merge {
                table_id: id.clone(),
                row: 0,
                column: 0,
                row_end: 1,
                column_end: 0,
            },
            TableOperation::Merge {
                table_id: id.clone(),
                row: 2,
                column: 0,
                row_end: 2,
                column_end: 2,
            },
        ],
    )
    .unwrap();
    assert!(
        plan.decisions
            .iter()
            .all(|decision| decision.status == TableEditDecisionStatus::Ready)
    );

    let carried = document
        .cells
        .iter()
        .find(|cell| cell.text == "B")
        .expect("the second cell keeps its text");
    assert_eq!(carried.row, 2);
    assert_eq!(carried.column_begin, Some(2));
}

/// KIPO XML 입력은 열 폭을 `width_raw` 가 아니라 HML 단위의 `hwidth_raw` 로만 들고 온다.
/// `Grid::load` 가 그것을 안 보면 폭이 전부 0으로 읽혀 표가 새 표 기본폭으로 갈아엎힌다.
/// 실물 PCT 표가 총폭 7110 에서 8544 로 넓어지는 것을 실측했다.
#[test]
fn keeps_the_source_widths_when_only_the_hml_unit_width_is_known() {
    let mut document = document();
    let original: u32 = document
        .cells
        .iter()
        .filter_map(|cell| cell.width_raw)
        .sum();
    for cell in &mut document.cells {
        cell.hwidth_raw = cell.width_raw.map(cell_width_to_colwidth);
        cell.width_raw = None;
    }
    let id = table_id(&document);
    apply_table_edits(
        &mut document,
        &[TableOperation::InsertColumn {
            table_id: id,
            index: 2,
        }],
    )
    .unwrap();

    let total: u32 = document
        .cells
        .iter()
        .filter(|cell| cell.row == 1)
        .filter_map(|cell| cell.width_raw)
        .sum();
    // ⛔ `original` 과 같기를 기대하지 마라. 통특기 들여오기는 내보내기의 역이 아니라서
    //    네이티브 -> colwidth -> 네이티브 왕복이 약 1% 자란다(format_rules 39쌍).
    //    여기서 지키는 것은 「폭이 살아 있다」이지 「왕복이 제자리」가 아니다.
    let expected: u32 = document
        .cells
        .iter()
        .filter(|cell| cell.row == 1)
        .filter_map(|cell| cell.hwidth_raw)
        .map(colwidth_to_cell_width)
        .sum();
    assert_eq!(original, 3_600);
    assert_eq!(total, expected);
    assert!(total > original, "폭이 새 표 기본폭으로 갈아엎히지 않았다");
}
