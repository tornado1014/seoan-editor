//! 편집기 세션의 표 그리기 · 표 편집 — 합성 문서(표 1개 · 1×2 「A」「B」)를 굽고, 고치고, 저장해 다시 연다.

use std::path::PathBuf;

use hlt_core::{
    EditorSession, GenerationDialect, TableOperation, WriteOptions, inspect_hml_path,
    save_generated_hlt_as_new,
};

fn session_with_table(dir: &tempfile::TempDir) -> EditorSession {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/generate_pct.hml");
    let document = inspect_hml_path(&fixture).unwrap();
    let path = dir.path().join("표.HLT");
    save_generated_hlt_as_new(
        &fixture,
        &document,
        GenerationDialect::PctPatent,
        &[],
        &path,
        WriteOptions::default(),
    )
    .unwrap();
    EditorSession::open(&path).unwrap()
}

/// 저장해 다시 연 문서의 표마다 (줄, 칸, 칸 글들).
fn reopen(
    session: &mut EditorSession,
    dir: &tempfile::TempDir,
    name: &str,
) -> Vec<(u32, u32, Vec<String>)> {
    let out = dir.path().join(format!("{name}.HLT"));
    session.save_as(&out).unwrap();
    let saved = EditorSession::open(&out).unwrap();
    saved
        .document
        .tables
        .iter()
        .map(|table| {
            let texts = table
                .cell_ids
                .iter()
                .map(|id| saved.cell_text(id).unwrap())
                .collect();
            (table.row_count, table.column_count, texts)
        })
        .collect()
}

fn paragraph_id(session: &EditorSession, text: &str) -> String {
    session
        .paragraphs()
        .into_iter()
        .find(|paragraph| paragraph.text.trim() == text)
        .unwrap()
        .id
}

/// 기존 표 **앞**에 새 표를 넣어도 두 표의 칸 글이 제 표에 남는다
/// (새 표가 표 목록 끝에 붙으면 꼬리 레코드 차례가 문서 차례와 어긋난다).
#[test]
fn inserts_a_table_before_an_existing_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = session_with_table(&dir);
    let heading = paragraph_id(&session, "【표 1】");
    let first = session.insert_table(&heading, 2, 3).unwrap();
    session.replace_cell_text(&first, "새 칸").unwrap();
    let tables = reopen(&mut session, &dir, "새표");
    assert_eq!(tables.len(), 2, "{tables:?}");
    assert_eq!((tables[0].0, tables[0].1), (2, 3), "{tables:?}");
    assert_eq!(tables[0].2[0], "새 칸", "{tables:?}");
    assert_eq!((tables[1].0, tables[1].1), (1, 2), "{tables:?}");
    assert_eq!(tables[1].2, ["A", "B"], "{tables:?}");
}

/// 칸 자리를 알고(화면의 「위/아래에 줄 넣기」), 줄 · 칸을 넣고 지우고, 합치고 나눈다.
#[test]
fn edits_rows_columns_and_merges() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = session_with_table(&dir);
    let table = session.document.tables[0].id.clone();
    let b = session.document.tables[0].cell_ids[1].clone();
    let position = session.cell_position(&b).unwrap();
    assert_eq!((position.row, position.column), (0, 1));
    assert_eq!((position.rows, position.columns), (1, 2));
    assert_eq!(position.table_id, table);

    let mut op = |operation| session_op(&mut session, operation);
    op(TableOperation::InsertRow {
        table_id: table.clone(),
        index: 1,
    });
    op(TableOperation::InsertColumn {
        table_id: table.clone(),
        index: 2,
    });
    let tables = reopen(&mut session, &dir, "줄칸");
    assert_eq!((tables[0].0, tables[0].1), (2, 3), "{tables:?}");
    assert_eq!(&tables[0].2[..2], ["A", "B"], "{tables:?}");

    let table = session.document.tables[0].id.clone();
    session_op(
        &mut session,
        TableOperation::Merge {
            table_id: table.clone(),
            row: 1,
            column: 0,
            row_end: 1,
            column_end: 1,
        },
    );
    session_op(
        &mut session,
        TableOperation::DeleteColumn {
            table_id: table.clone(),
            index: 2,
        },
    );
    let tables = reopen(&mut session, &dir, "합침");
    assert_eq!((tables[0].0, tables[0].1), (2, 2), "{tables:?}");
    assert_eq!(tables[0].2.len(), 3, "합친 아래 줄은 한 칸: {tables:?}");

    // 마지막 줄 하나 남기면 더 못 지운다 — 고치지 않고 이유를 낸다.
    let table = session.document.tables[0].id.clone();
    session_op(
        &mut session,
        TableOperation::DeleteRow {
            table_id: table.clone(),
            index: 1,
        },
    );
    let error = session
        .table_edit(TableOperation::DeleteRow {
            table_id: table,
            index: 0,
        })
        .unwrap_err();
    assert_eq!(
        error.details.get("reason").map(String::as_str),
        Some("table_skipped:last_row")
    );
}

/// 표 지우기: 표가 없어지고 그 자리는 빈 문단으로 남는다(뒤 글은 그대로).
#[test]
fn deletes_a_table() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = session_with_table(&dir);
    let table = session.document.tables[0].id.clone();
    session.delete_table(&table).unwrap();
    assert!(reopen(&mut session, &dir, "지움").is_empty());
    assert!(
        session
            .paragraphs()
            .iter()
            .any(|p| p.text.contains("claim 1."))
    );
}

fn session_op(session: &mut EditorSession, operation: TableOperation) {
    session.table_edit(operation).unwrap();
}

/// 표가 둘 이상일 때 꼬리 레코드: 표는 문서 차례로, 표마다 「실제 줄들 → 센티널 줄」이 줄 자리를 차례로 차지하고
/// 칸 레코드의 줄 번호도 그 자리로 센다. (센티널 자리를 안 세면 둘째 표부터 칸이 앞 표 센티널 자리를 가리켜
/// 통특기에서 여러 줄 표의 첫 줄이 오른쪽으로 밀리고 줄 사이가 벌어졌다.)
#[test]
fn table_rows_and_sentinels_take_slots_in_document_order() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/generate_pct.hml");
    let mut document = inspect_hml_path(&fixture).unwrap();
    // 기존 표(1×2) **앞**에 3×2 새 표.
    let mut blank = document.paragraphs[0].clone();
    blank.id = "paragraph:new".into();
    blank.text.clear();
    blank.inlines.clear();
    blank.marker = None;
    blank.source_line_bytes.clear();
    document.paragraphs.insert(1, blank);
    for section in &mut document.sections {
        section.paragraph_ids = document
            .paragraphs
            .iter()
            .filter(|p| p.section_id.as_deref() == Some(section.id.as_str()))
            .map(|p| p.id.clone())
            .collect();
    }
    let plan = hlt_core::apply_table_edits(
        &mut document,
        &[TableOperation::NewTable {
            paragraph_id: "paragraph:new".into(),
            rows: 3,
            columns: 2,
        }],
    )
    .unwrap();
    assert_eq!(plan.ready, 1);
    assert_eq!(
        document.tables[0].paragraph_id, "paragraph:new",
        "표 목록 = 문서 차례"
    );
    let body = hlt_core::plan_hlt_body(&document, GenerationDialect::PctPatent).unwrap();
    let tail =
        hlt_core::plan_table_tail(&document, GenerationDialect::PctPatent, &body.cell_ranges)
            .unwrap();
    let rows: Vec<u32> = tail.cells.iter().map(|cell| cell.row).collect();
    assert_eq!(rows, [1, 1, 2, 2, 3, 3, 5, 5], "실제 칸 줄 자리");
    let sentinels: Vec<u32> = tail.terminal_cells.iter().map(|cell| cell.row).collect();
    assert_eq!(sentinels, [4, 4, 6, 6], "센티널 칸 줄 자리");
}

/// 표 테두리: 쪽 계획에 칸마다 상자(빈 칸 포함) — 새 빈 표가 화면 · PDF 에서 보이지 않던 것.
/// 상자 왼쪽 · 폭은 칸 시작 · 폭(twip/20)을 따르고, 같은 행 칸들은 붙어 있다.
#[test]
fn page_plan_draws_a_box_for_every_cell() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = session_with_table(&dir);
    let heading = paragraph_id(&session, "【표 1】");
    let first = session.insert_table(&heading, 2, 3).unwrap();
    let boxes: Vec<_> = session
        .plan
        .pages
        .iter()
        .flat_map(|page| page.cells.iter())
        .collect();
    assert_eq!(boxes.len(), 2 * 3 + 2, "새 표 6칸 + 기존 표 2칸: {boxes:?}");
    let row: Vec<_> = boxes.iter().filter(|b| b.top == boxes[0].top).collect();
    assert_eq!(row.len(), 3, "새 표 첫 행 3칸");
    assert_eq!(row[0].cell, first);
    for pair in row.windows(2) {
        assert!(
            (pair[0].x + pair[0].width - pair[1].x).abs() < 0.01,
            "칸이 붙어 있다: {pair:?}"
        );
    }
    assert!((row[0].width - 2848.0 / 20.0).abs() < 0.01);
    assert!(boxes.iter().all(|b| b.height > 10.0));
}

/// 화면이 누른 칸 id(쪽 계획의 칸 상자)는 표를 고친 뒤에도 모델에서 찾아진다 — 「표」 메뉴 · 칸 글 고치기.
#[test]
fn clicked_cell_ids_stay_valid_after_table_edits() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = session_with_table(&dir);
    let heading = paragraph_id(&session, "【표 1】");
    session.insert_table(&heading, 2, 2).unwrap();
    let table = session
        .cell_position(&session.plan.pages[0].cells[0].cell)
        .unwrap()
        .table_id;
    session
        .table_edit(TableOperation::InsertRow {
            table_id: table,
            index: 1,
        })
        .unwrap();
    for cell in session.plan.pages.iter().flat_map(|page| &page.cells) {
        assert!(
            session.cell_position(&cell.cell).is_some(),
            "칸 자리: {}",
            cell.cell
        );
        assert!(
            session.cell_text(&cell.cell).is_some(),
            "칸 글: {}",
            cell.cell
        );
    }
    let last = session.plan.pages[0].cells[4].cell.clone();
    session.replace_cell_text(&last, "셋째 줄").unwrap();
    assert_eq!(session.cell_text(&last).as_deref(), Some("셋째 줄"));
}

/// 쪽 위 칸 편집: 칸마다(빈 칸도) 커서 줄이 있고, 칸 줄들의 자리(`start` · `stops`)를 이으면 칸 글과 글자 수가 같다
/// (줄바꿈된 긴 칸 · 영문 · 공백 포함 — 어긋나면 친 글이 엉뚱한 자리에 들어간다).
#[test]
fn cell_lines_carry_caret_stops_that_add_up_to_the_cell_text() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = session_with_table(&dir);
    let heading = paragraph_id(&session, "【표 1】");
    let first = session.insert_table(&heading, 2, 2).unwrap();
    session
        .replace_cell_text(
            &first,
            "가나다라마바사아자차카타파하 abc def 가나다라마바사",
        )
        .unwrap();
    let cells: Vec<String> = session
        .plan
        .pages
        .iter()
        .flat_map(|p| &p.cells)
        .map(|c| c.cell.clone())
        .collect();
    assert!(!cells.is_empty());
    for cell in &cells {
        let mut lines: Vec<_> = session
            .plan
            .pages
            .iter()
            .flat_map(|p| &p.lines)
            .filter(|line| line.cell.as_deref() == Some(cell.as_str()))
            .collect();
        assert!(!lines.is_empty(), "칸 {cell} 에 커서 줄이 없다");
        lines.sort_by_key(|line| line.start.unwrap());
        let mut next = 0;
        for line in &lines {
            assert_eq!(line.start, Some(next), "칸 {cell} 줄 이음");
            assert!(!line.stops.is_empty());
            next += line.stops.len() as u32 - 1;
        }
        let text = session.cell_text(cell).unwrap();
        assert_eq!(next as usize, text.chars().count(), "칸 {cell} 글 {text:?}");
    }
    let wrapped = session
        .plan
        .pages
        .iter()
        .flat_map(|p| &p.lines)
        .filter(|l| l.cell.as_deref() == Some(first.as_str()))
        .count();
    assert!(wrapped >= 2, "긴 칸 글은 여러 줄: {wrapped}");
}
