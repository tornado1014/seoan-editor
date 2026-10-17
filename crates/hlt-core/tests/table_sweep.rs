//! 표 편집 전수 훑기. 고정 표본의 **모든 행과 모든 열**을 하나씩 건드려 본다.
//!
//! 왜 전수인가. 옛 파이썬 구현이 결함 넷을 잡았는데 **바이트 대조로는
//! 하나도 안 나왔다.** 교재가 되는 통특기 저장본이 마침 그 경우를 안 밟는 문서였기 때문이다.
//! 넷 다 고정 표본의 모든 행과 모든 열을 훑는 테스트에서만 나왔다.
//!
//! ⛔ **연산은 반드시 둘씩 건다.** 세로병합의 이어짐 셀은 문서에 저장된 뒤에만 존재하므로
//! 한 연산만 재면 통과하고 다음 연산이 표를 다시 읽을 때 터진다(2026-08-26 사용자 육안
//! 판정으로 발견한 결함 A 가 그것이다).

mod support;

use hlt_core::{
    GenerationDialect, HmlDocument, TableOperation, WriteOptions, apply_table_edits,
    hml_document_from_hlt, save_generated_hlt_as_new,
};
use std::path::Path;

/// 첫 표를 행별 칸 글자로 편다. 세로병합에 덮인 칸은 판독기가 주 셀에 접으므로
/// 여기 나오는 것은 살아 있는 칸뿐이다.
fn grid(document: &HmlDocument) -> Vec<Vec<String>> {
    let table = &document.tables[0];
    // ⛔ `HmlCell.row` 는 **1부터**다. 0 을 첫 행으로 읽으면 빈 행이 하나 앞에 붙는다.
    let mut rows: std::collections::BTreeMap<u32, Vec<String>> = std::collections::BTreeMap::new();
    for cell in document
        .cells
        .iter()
        .filter(|cell| cell.table_id == table.id)
    {
        rows.entry(cell.row).or_default().push(cell.text.clone());
    }
    rows.into_values().collect()
}

/// 편집한 모델을 실제로 HLT 로 굽고, 그것을 **다시 역변환해서** 표를 읽는다.
/// 의미층만 보면 굽는 쪽 결함이 안 보인다.
fn bake(
    source: &Path,
    document: &HmlDocument,
    payloads: &[hlt_core::GeneratedImagePayload],
) -> HmlDocument {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("sweep.HLT");
    save_generated_hlt_as_new(
        source,
        document,
        GenerationDialect::PatentSpecificationKo,
        payloads,
        &output,
        WriteOptions::default(),
    )
    .unwrap();
    hml_document_from_hlt(&output).unwrap().0
}

#[test]
fn the_fixture_is_one_table_of_eleven_rows_and_three_columns() {
    let fixture = support::application_table_fixture();
    let (document, _) = hml_document_from_hlt(fixture.path()).unwrap();
    assert_eq!(document.tables.len(), 1);
    let rows = grid(&document);
    assert_eq!(rows.len(), 11);
    assert!(rows.iter().all(|row| row.len() == 3), "{rows:?}");
}

/// 행을 하나 지우고 **곧바로 한 행을 끼운다.** 지운 행 말고는 글자가 제자리에 있어야 한다.
#[test]
fn deleting_each_row_in_turn_leaves_the_other_rows_where_they_were() {
    let fixture = support::application_table_fixture();
    let (source, payloads) = hml_document_from_hlt(fixture.path()).unwrap();
    let before = grid(&source);

    for victim in 0..before.len() {
        let mut document = source.clone();
        let id = document.tables[0].id.clone();
        apply_table_edits(
            &mut document,
            &[
                TableOperation::DeleteRow {
                    table_id: id.clone(),
                    index: victim as u32,
                },
                TableOperation::InsertRow {
                    table_id: id,
                    index: 0,
                },
            ],
        )
        .unwrap();
        let after = grid(&bake(fixture.path(), &document, &payloads));

        let mut expected = before.clone();
        expected.remove(victim);
        expected.insert(0, vec![String::new(); before[0].len()]);
        assert_eq!(after, expected, "행 {victim} 을 지웠을 때");
    }
}

/// 열을 하나 지우고 **곧바로 그 자리를 다시 쪼갠다.** 남은 열의 글자가 밀리면 안 된다.
#[test]
fn deleting_each_column_in_turn_leaves_the_other_columns_where_they_were() {
    let fixture = support::application_table_fixture();
    let (source, payloads) = hml_document_from_hlt(fixture.path()).unwrap();
    let before = grid(&source);
    let columns = before[0].len();

    for victim in 0..columns {
        let mut document = source.clone();
        let id = document.tables[0].id.clone();
        apply_table_edits(
            &mut document,
            &[
                TableOperation::DeleteColumn {
                    table_id: id.clone(),
                    index: victim as u32,
                },
                TableOperation::InsertColumn {
                    table_id: id,
                    index: 0,
                },
            ],
        )
        .unwrap();
        let after = grid(&bake(fixture.path(), &document, &payloads));

        let expected: Vec<Vec<String>> = before
            .iter()
            .map(|row| {
                let mut kept: Vec<String> = row
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| *index != victim)
                    .map(|(_, text)| text.clone())
                    .collect();
                kept.insert(0, String::new());
                kept
            })
            .collect();
        assert_eq!(after, expected, "열 {victim} 을 지웠을 때");
    }
}

/// 세로 병합을 만든 **뒤에** 다른 행을 건드린다. 결함 A 가 여기서 나왔다.
/// 이어짐 셀은 저장된 뒤에만 존재하므로 병합한 그 연산에서는 안 드러난다.
#[test]
fn a_later_edit_does_not_shift_rows_that_a_merge_covers() {
    let fixture = support::application_table_fixture();
    let (source, payloads) = hml_document_from_hlt(fixture.path()).unwrap();
    let before = grid(&source);

    for anchor in 0..before.len() - 1 {
        let mut document = source.clone();
        let id = document.tables[0].id.clone();
        apply_table_edits(
            &mut document,
            &[
                TableOperation::Merge {
                    table_id: id.clone(),
                    row: anchor as u32,
                    column: 0,
                    row_end: anchor as u32 + 1,
                    column_end: 0,
                },
                TableOperation::InsertRow {
                    table_id: id,
                    index: before.len() as u32,
                },
            ],
        )
        .unwrap();
        let after = grid(&bake(fixture.path(), &document, &payloads));

        // 병합에 안 걸린 행은 글자가 그대로여야 한다. 걸린 두 행은 칸 수가 달라진다.
        for (row, texts) in before.iter().enumerate() {
            if row == anchor || row == anchor + 1 {
                continue;
            }
            assert_eq!(&after[row], texts, "{anchor} 행을 병합했을 때 {row} 행");
        }
    }
}

/// 세로 병합 앵커 **바로 아래**에 행을 끼운다. 위 훑기는 삽입 자리를 표 끝으로만 걸어서
/// 이 자리를 못 봤다.
///
/// 통특기는 앵커가 새 행을 삼킨다. rowspan 이 하나 늘고
/// 새 행의 그 열은 이어받는 칸이 된다. 우리 `Grid::insert_row` 도 그렇게 한다.
/// ⛔ 다만 **그 열에 새 칸을 하나 더 만들어서** 3열 표의 새 행이 4칸이 됐다.
#[test]
fn inserting_a_row_under_a_merge_anchor_keeps_the_column_count() {
    let fixture = support::application_table_fixture();
    let (source, payloads) = hml_document_from_hlt(fixture.path()).unwrap();
    let before = grid(&source);
    let columns = before[0].len();

    for anchor in 0..before.len() - 1 {
        let mut document = source.clone();
        let id = document.tables[0].id.clone();
        apply_table_edits(
            &mut document,
            &[
                TableOperation::Merge {
                    table_id: id.clone(),
                    row: anchor as u32,
                    column: 0,
                    row_end: anchor as u32 + 1,
                    column_end: 0,
                },
                TableOperation::InsertRow {
                    table_id: id,
                    index: anchor as u32 + 1,
                },
            ],
        )
        .unwrap();

        let table = document.tables[0].id.clone();
        let mut per_row: std::collections::BTreeMap<u32, usize> = Default::default();
        for cell in document.cells.iter().filter(|cell| cell.table_id == table) {
            *per_row.entry(cell.row).or_default() += cell.colspan.max(1) as usize;
        }
        let wide: Vec<(u32, usize)> = per_row
            .iter()
            .filter(|(_, count)| **count != columns)
            .map(|(row, count)| (*row, *count))
            .collect();
        assert!(
            wide.is_empty(),
            "앵커 {anchor} 아래에 끼웠더니 칸 수가 {columns} 이 아닌 행 {wide:?}"
        );

        let after = grid(&bake(fixture.path(), &document, &payloads));
        assert_eq!(after.len(), before.len() + 1, "앵커 {anchor}: 행 수");
    }
}

/// 격자 불변식을 **연산 조합 전수**로 훑는다. 굽지 않고 의미 모델만 본다.
///
/// 위 훑기들은 굽고 다시 읽어서 확실하지만 한 번에 1초 넘게 든다. 그래서 자리가 좁았고,
/// 실제로 결함 둘을 놓쳤다. 놓친 이유가 둘 다 **너비**였다.
/// 1. 삽입 자리를 표 끝으로만 걸어서 앵커 바로 아래를 못 봤다(칸이 하나 더 생기던 결함).
/// 2. 옛 파이썬 구현도 같은 함정을 겪었다. 훑기를 `merge_right`, `merge_down`, 등분 2, 3 까지
///    1,918 가지로 넓히고서야 자기 판독기 결함을 찾았다.
///
/// 그래서 굽는 훑기는 좁게 두고, **의미층 불변식만 보는 넓은 훑기**를 따로 둔다.
/// 마지막 결함(3열 표에 4칸짜리 행)은 굽지 않고도 잡히는 것이었다.
///
/// 불변식 둘.
/// - 어느 행이든 `colspan` 합이 표의 열 수와 같다.
/// - 원본 칸 글자는 지운 것 말고 하나도 안 사라진다.
#[test]
fn every_operation_pair_keeps_the_grid_rectangular() {
    let fixture = support::application_table_fixture();
    let (source, _) = hml_document_from_hlt(fixture.path()).unwrap();
    let id = source.tables[0].id.clone();
    let before = grid(&source);
    let rows = before.len() as u32;
    let columns = before[0].len() as u32;

    let mut first: Vec<(String, TableOperation)> = Vec::new();
    for row in 0..rows {
        for column in 0..columns {
            if column + 1 < columns {
                first.push((
                    format!("가로병합 {row},{column}"),
                    TableOperation::Merge {
                        table_id: id.clone(),
                        row,
                        column,
                        row_end: row,
                        column_end: column + 1,
                    },
                ));
            }
            if row + 1 < rows {
                first.push((
                    format!("세로병합 {row},{column}"),
                    TableOperation::Merge {
                        table_id: id.clone(),
                        row,
                        column,
                        row_end: row + 1,
                        column_end: column,
                    },
                ));
            }
            first.push((
                format!("나누기 {row},{column}"),
                TableOperation::Split {
                    table_id: id.clone(),
                    row,
                    column,
                },
            ));
        }
    }

    let second = |row: u32, column: u32| -> Vec<(String, TableOperation)> {
        vec![
            (
                format!("행삽입 {row}"),
                TableOperation::InsertRow {
                    table_id: id.clone(),
                    index: row,
                },
            ),
            (
                format!("행삭제 {row}"),
                TableOperation::DeleteRow {
                    table_id: id.clone(),
                    index: row,
                },
            ),
            (
                format!("열삽입 {column}"),
                TableOperation::InsertColumn {
                    table_id: id.clone(),
                    index: column,
                },
            ),
            (
                format!("열삭제 {column}"),
                TableOperation::DeleteColumn {
                    table_id: id.clone(),
                    index: column,
                },
            ),
        ]
    };

    let mut checked = 0_u32;
    for (first_label, first_op) in &first {
        for row in 0..=rows {
            for (second_label, second_op) in second(row, row.min(columns)) {
                let mut document = source.clone();
                // 첫 연산이 거절되면 볼 것이 없다. 둘째 연산의 거절은 정상 동작이다.
                if apply_table_edits(&mut document, std::slice::from_ref(first_op)).is_err() {
                    continue;
                }
                if apply_table_edits(&mut document, &[second_op]).is_err() {
                    continue;
                }
                checked += 1;

                let table = document.tables[0].id.clone();
                let mut per_row: std::collections::BTreeMap<u32, usize> = Default::default();
                for cell in document.cells.iter().filter(|cell| cell.table_id == table) {
                    *per_row.entry(cell.row).or_default() += cell.colspan.max(1) as usize;
                }
                let width = *per_row.values().next().expect("행이 하나는 있다");
                let ragged: Vec<(u32, usize)> = per_row
                    .iter()
                    .filter(|(_, count)| **count != width)
                    .map(|(row, count)| (*row, *count))
                    .collect();
                assert!(
                    ragged.is_empty(),
                    "{first_label} 뒤 {second_label}: 칸 수가 {width} 이 아닌 행 {ragged:?}"
                );
            }
        }
    }
    println!("훑은 조합 {checked} 가지");
    assert!(checked > 500, "훑은 조합이 {checked} 가지뿐이다");
}
