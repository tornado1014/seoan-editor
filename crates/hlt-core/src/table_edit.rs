//! 표 구조 편집.
//!
//! 의미 모델(`HmlDocument`)의 표를 직사각 격자로 정규화한 뒤 행/열 삽입과 삭제,
//! 병합, 분할을 적용하고 다시 셀 목록으로 되돌린다. 바이트층은 손대지 않는다.
//! 편집된 모델을 생성기에 넘기면 본문과 tail 이 통째로 재생성된다.
//!
//! 격자에서 `None` 은 병합에 덮인 칸이다. 세로 병합은 통특기/워드와 같은 모양으로,
//! 즉 rowspan 을 가진 주 셀 하나와 덮인 행마다 `merge_continuation` 셀 하나로 되돌린다.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{CoreError, ErrorCode, HmlCell, HmlDocument};

/// 통특기가 빈 명세서에서 새 표를 만들 때 쓰는 총폭.
/// 열 폭은 이 값을 열 수로 나누고 나머지를 마지막 열에 준다.
pub const NEW_TABLE_TOTAL_WIDTH: u32 = 8_544;

/// 표 하나에 허용하는 상한. 통특기 자체 상한과 무관한 방어값이다.
const MAX_ROWS: usize = 200;
const MAX_COLUMNS: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TableOperation {
    NewTable {
        paragraph_id: String,
        rows: u32,
        columns: u32,
    },
    InsertRow {
        table_id: String,
        index: u32,
    },
    DeleteRow {
        table_id: String,
        index: u32,
    },
    InsertColumn {
        table_id: String,
        index: u32,
    },
    DeleteColumn {
        table_id: String,
        index: u32,
    },
    Merge {
        table_id: String,
        row: u32,
        column: u32,
        row_end: u32,
        column_end: u32,
    },
    Split {
        table_id: String,
        row: u32,
        column: u32,
    },
}

impl TableOperation {
    fn table_id(&self) -> Option<&str> {
        match self {
            TableOperation::NewTable { .. } => None,
            TableOperation::InsertRow { table_id, .. }
            | TableOperation::DeleteRow { table_id, .. }
            | TableOperation::InsertColumn { table_id, .. }
            | TableOperation::DeleteColumn { table_id, .. }
            | TableOperation::Merge { table_id, .. }
            | TableOperation::Split { table_id, .. } => Some(table_id),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableEditDecisionStatus {
    Ready,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableEditSkipReason {
    TableNotFound,
    ParagraphNotFound,
    ParagraphNotEmpty,
    IndexOutOfRange,
    LastRow,
    LastColumn,
    CellContainsImage,
    NotMerged,
    MergeCrossesBoundary,
    SizeLimit,
    NoChange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableEditDecision {
    pub operation_index: u32,
    pub table_id: Option<String>,
    pub status: TableEditDecisionStatus,
    pub reason: Option<TableEditSkipReason>,
    pub row_count: u32,
    pub column_count: u32,
    /// 이 연산으로 사라지는 셀 가운데 글이 들어 있던 칸 수.
    pub discarded_text_cells: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableEditPlan {
    pub source: String,
    pub operation_count: u32,
    pub ready: u32,
    pub skipped: u32,
    pub decisions: Vec<TableEditDecision>,
}

/// 요청 파일이 실어 나르는 연산 목록.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableEditRequest {
    pub operations: Vec<TableOperation>,
}

/// 편집한 모델을 새 HLT 로 구운 결과. 저장은 언제나 create-new 다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableEditSaveResult {
    #[serde(flatten)]
    pub generated: crate::GeneratedHltSaveResult,
    pub edit_plan: TableEditPlan,
}

/// 문서를 복제해 편집을 예행하고 결과만 돌려준다. 원본은 바뀌지 않는다.
pub fn plan_table_edits(
    document: &HmlDocument,
    operations: &[TableOperation],
) -> Result<TableEditPlan, CoreError> {
    let mut draft = document.clone();
    apply_table_edits(&mut draft, operations)
}

/// 편집을 실제로 적용한다. 건너뛴 연산은 모델을 건드리지 않는다.
pub fn apply_table_edits(
    document: &mut HmlDocument,
    operations: &[TableOperation],
) -> Result<TableEditPlan, CoreError> {
    if operations.len() > 500 {
        return Err(edit_error("operation_limit"));
    }
    let mut decisions = Vec::with_capacity(operations.len());
    for (index, operation) in operations.iter().enumerate() {
        let index = u32::try_from(index).map_err(|_| edit_error("operation_index_overflow"))?;
        decisions.push(apply_one(document, index, operation)?);
    }
    let ready = decisions
        .iter()
        .filter(|value| value.status == TableEditDecisionStatus::Ready)
        .count() as u32;
    Ok(TableEditPlan {
        source: document.source.clone(),
        operation_count: operations.len() as u32,
        ready,
        skipped: decisions.len() as u32 - ready,
        decisions,
    })
}

fn apply_one(
    document: &mut HmlDocument,
    operation_index: u32,
    operation: &TableOperation,
) -> Result<TableEditDecision, CoreError> {
    if let TableOperation::NewTable {
        paragraph_id,
        rows,
        columns,
    } = operation
    {
        return new_table(document, operation_index, paragraph_id, *rows, *columns);
    }

    let table_id = operation.table_id().expect("checked above").to_string();
    let Some(mut grid) = Grid::load(document, &table_id)? else {
        return Ok(skip(operation_index, Some(table_id), None, 0, 0));
    };

    let outcome = match operation {
        TableOperation::InsertRow { index, .. } => grid.insert_row(*index as usize),
        TableOperation::DeleteRow { index, .. } => grid.delete_row(*index as usize, document),
        TableOperation::InsertColumn { index, .. } => grid.insert_column(*index as usize),
        TableOperation::DeleteColumn { index, .. } => grid.delete_column(*index as usize, document),
        TableOperation::Merge {
            row,
            column,
            row_end,
            column_end,
            ..
        } => grid.merge(
            *row as usize,
            *column as usize,
            *row_end as usize,
            *column_end as usize,
        ),
        TableOperation::Split { row, column, .. } => grid.split(*row as usize, *column as usize),
        TableOperation::NewTable { .. } => unreachable!("handled above"),
    };

    match outcome {
        Err(reason) => Ok(skip(
            operation_index,
            Some(table_id),
            Some(reason),
            grid.rows.len() as u32,
            grid.widths.len() as u32,
        )),
        Ok(discarded) => {
            grid.store(document, &table_id)?;
            Ok(TableEditDecision {
                operation_index,
                table_id: Some(table_id),
                status: TableEditDecisionStatus::Ready,
                reason: None,
                row_count: grid.rows.len() as u32,
                column_count: grid.widths.len() as u32,
                discarded_text_cells: discarded,
            })
        }
    }
}

fn skip(
    operation_index: u32,
    table_id: Option<String>,
    reason: Option<TableEditSkipReason>,
    row_count: u32,
    column_count: u32,
) -> TableEditDecision {
    TableEditDecision {
        operation_index,
        table_id,
        status: TableEditDecisionStatus::Skipped,
        reason: Some(reason.unwrap_or(TableEditSkipReason::TableNotFound)),
        row_count,
        column_count,
        discarded_text_cells: 0,
    }
}

fn new_table(
    document: &mut HmlDocument,
    operation_index: u32,
    paragraph_id: &str,
    rows: u32,
    columns: u32,
) -> Result<TableEditDecision, CoreError> {
    let rows_n = rows as usize;
    let columns_n = columns as usize;
    if rows_n == 0 || columns_n == 0 || rows_n > MAX_ROWS || columns_n > MAX_COLUMNS {
        return Ok(skip(
            operation_index,
            None,
            Some(TableEditSkipReason::SizeLimit),
            rows,
            columns,
        ));
    }
    let Some(position) = document
        .paragraphs
        .iter()
        .position(|value| value.id == paragraph_id)
    else {
        return Ok(skip(
            operation_index,
            None,
            Some(TableEditSkipReason::ParagraphNotFound),
            0,
            0,
        ));
    };
    if document.paragraphs[position].table_id.is_some()
        || !document.paragraphs[position].text.trim().is_empty()
        || !document.paragraphs[position].image_ids.is_empty()
    {
        return Ok(skip(
            operation_index,
            None,
            Some(TableEditSkipReason::ParagraphNotEmpty),
            0,
            0,
        ));
    }

    let occurrence = document
        .tables
        .iter()
        .map(|table| table.occurrence + 1)
        .max()
        .unwrap_or(0);
    let table_id = unique_table_id(document, occurrence);
    let mut grid = Grid {
        rows: vec![vec![None; columns_n]; rows_n],
        cells: Vec::new(),
        widths: even_widths(NEW_TABLE_TOTAL_WIDTH, columns_n),
        template: CellTemplate::default(),
        uses_start: true,
        next_id: 0,
    };
    for row in 0..rows_n {
        for column in 0..columns_n {
            grid.cells.push(GridCell::blank());
            grid.rows[row][column] = Some(grid.cells.len() - 1);
        }
    }

    // 생성기는 꼬리(줄 · 칸 레코드)를 표 목록 차례로 쓰고 본문은 문단 차례로 쓴다 — 둘이 같아야 한다.
    // 새 표를 문서 차례 자리에 끼우고 차례 번호를 다시 매긴다(뒤에 붙이면 앞에 넣은 표가 어긋났다).
    let paragraph_at = |id: &str| {
        document
            .paragraphs
            .iter()
            .position(|value| value.id == id)
            .unwrap_or(usize::MAX)
    };
    let slot = document
        .tables
        .iter()
        .filter(|table| paragraph_at(&table.paragraph_id) < position)
        .count();
    document.tables.insert(
        slot,
        crate::HmlTable {
            id: table_id.clone(),
            occurrence,
            paragraph_id: paragraph_id.to_string(),
            max_columns: columns,
            row_count: rows,
            cell_ids: Vec::new(),
        },
    );
    for (index, table) in document.tables.iter_mut().enumerate() {
        table.occurrence = index as u32;
    }
    document.paragraphs[position].table_id = Some(table_id.clone());
    grid.store(document, &table_id)?;

    Ok(TableEditDecision {
        operation_index,
        table_id: Some(table_id),
        status: TableEditDecisionStatus::Ready,
        reason: None,
        row_count: rows,
        column_count: columns,
        discarded_text_cells: 0,
    })
}

/// 칸 하나가 표 격자에서 차지하는 자리(0 부터) — 편집기가 「이 칸 위/아래에 줄 넣기」를 정할 때 쓴다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellPosition {
    pub table_id: String,
    pub row: u32,
    pub column: u32,
    pub rowspan: u32,
    pub colspan: u32,
    /// 표 전체 줄 · 칸 수.
    pub rows: u32,
    pub columns: u32,
}

/// `cell_id` 의 격자 자리. 병합에 덮인 칸(이어짐 칸)은 `None`.
pub fn cell_position(
    document: &HmlDocument,
    cell_id: &str,
) -> Result<Option<CellPosition>, CoreError> {
    let Some(table_id) = document
        .cells
        .iter()
        .find(|cell| cell.id == cell_id)
        .map(|cell| cell.table_id.clone())
    else {
        return Ok(None);
    };
    let Some(grid) = Grid::load(document, &table_id)? else {
        return Ok(None);
    };
    for (row, slots) in grid.rows.iter().enumerate() {
        for (column, slot) in slots.iter().enumerate() {
            let Some(index) = slot else { continue };
            let cell = &grid.cells[*index];
            if cell.id.as_deref() == Some(cell_id) {
                return Ok(Some(CellPosition {
                    table_id,
                    row: row as u32,
                    column: column as u32,
                    rowspan: cell.rowspan as u32,
                    colspan: cell.colspan as u32,
                    rows: grid.rows.len() as u32,
                    columns: grid.widths.len() as u32,
                }));
            }
        }
    }
    Ok(None)
}

/// 총폭을 열 수로 나누고 나머지는 마지막 열에 준다(통특기 기본값).
fn even_widths(total: u32, columns: usize) -> Vec<u32> {
    let columns_u32 = columns as u32;
    let base = total / columns_u32;
    let mut widths = vec![base; columns];
    if let Some(last) = widths.last_mut() {
        *last = total - base * (columns_u32 - 1);
    }
    widths
}

#[derive(Debug, Clone, Default)]
struct CellTemplate {
    align: String,
    indent_raw: i32,
    lspace: u32,
}

#[derive(Debug, Clone)]
struct GridCell {
    /// 기존 셀이면 원본 id. 새로 만든 칸은 `None`.
    id: Option<String>,
    rowspan: usize,
    colspan: usize,
}

impl GridCell {
    fn blank() -> Self {
        GridCell {
            id: None,
            rowspan: 1,
            colspan: 1,
        }
    }
}

struct Grid {
    /// 격자 슬롯. 주 셀의 왼쪽 위 칸에만 색인이 들어가고 덮인 칸은 `None`.
    rows: Vec<Vec<Option<usize>>>,
    cells: Vec<GridCell>,
    widths: Vec<u32>,
    template: CellTemplate,
    /// 원본이 `start_raw` 를 쓰던 문서인지. 워드에서 온 표는 쓰지 않는다.
    uses_start: bool,
    next_id: u32,
}

type GridResult = Result<u32, TableEditSkipReason>;

impl Grid {
    fn load(document: &HmlDocument, table_id: &str) -> Result<Option<Grid>, CoreError> {
        let Some(table) = document.tables.iter().find(|value| value.id == table_id) else {
            return Ok(None);
        };
        let by_id: BTreeMap<&str, &HmlCell> =
            document.cells.iter().map(|c| (c.id.as_str(), c)).collect();
        let mut ordered: BTreeMap<u32, Vec<&HmlCell>> = BTreeMap::new();
        for id in &table.cell_ids {
            let cell = *by_id
                .get(id.as_str())
                .ok_or_else(|| edit_error("missing_cell"))?;
            ordered.entry(cell.row).or_default().push(cell);
        }

        let column_count = ordered
            .values()
            .map(|row| {
                row.iter()
                    .map(|cell| cell.colspan.max(1) as usize)
                    .sum::<usize>()
            })
            .max()
            .unwrap_or(0)
            .max(1);
        let row_count = ordered.len();
        if row_count == 0 {
            return Ok(None);
        }

        let mut grid = Grid {
            rows: vec![vec![None; column_count]; row_count],
            cells: Vec::new(),
            widths: vec![0; column_count],
            template: CellTemplate::default(),
            uses_start: false,
            next_id: 0,
        };
        // 열 폭은 colspan 1 인 셀에서 곧바로 읽고, 남는 열만 넓은 셀에서 균등 분배한다.
        let mut spans: Vec<(usize, usize, u32)> = Vec::new();
        let mut taken = vec![vec![false; column_count]; row_count];
        for (row_index, cells) in ordered.values().enumerate() {
            let mut column = 0_usize;
            for cell in cells {
                while column < column_count && taken[row_index][column] {
                    column += 1;
                }
                if column >= column_count {
                    break;
                }
                if cell.merge_continuation {
                    // 세로 병합에 덮인 칸이다. 그 자리는 주 셀의 rowspan 이 이미 taken 에
                    // 찍어 뒀다. 여기서 또 찍고 column 을 밀면 그 행의 뒤 칸이 통째로
                    // 한 칸씩 오른쪽으로 밀린다(실물 PCT 표에서 통특기로 확인한 결함).
                    continue;
                }
                let rowspan = (cell.rowspan.max(1) as usize).min(row_count - row_index);
                let colspan = (cell.colspan.max(1) as usize).min(column_count - column);
                if grid.template.align.is_empty() {
                    grid.template = CellTemplate {
                        align: cell.align.clone(),
                        indent_raw: cell.indent_raw,
                        lspace: cell.lspace,
                    };
                }
                grid.uses_start |= cell.start_raw.is_some();
                // KIPO XML 입력은 `width_raw` 를 안 채우고 `colwidth` 를 `hwidth_raw` 로만 준다.
                let source_width = cell
                    .width_raw
                    .or_else(|| cell.hwidth_raw.map(crate::colwidth_to_cell_width));
                if let Some(width) = source_width {
                    if colspan == 1 {
                        grid.widths[column] = width;
                    } else {
                        spans.push((column, colspan, width));
                    }
                }
                grid.cells.push(GridCell {
                    id: Some(cell.id.clone()),
                    rowspan,
                    colspan,
                });
                grid.rows[row_index][column] = Some(grid.cells.len() - 1);
                for row_offset in 0..rowspan {
                    for column_offset in 0..colspan {
                        taken[row_index + row_offset][column + column_offset] = true;
                    }
                }
                column += colspan;
            }
        }
        grid.resolve_widths(&spans);
        Ok(Some(grid))
    }

    fn resolve_widths(&mut self, spans: &[(usize, usize, u32)]) {
        for (column, colspan, width) in spans {
            let unknown: Vec<usize> = (*column..column + colspan)
                .filter(|index| self.widths[*index] == 0)
                .collect();
            if unknown.is_empty() {
                continue;
            }
            let known: u32 = (*column..column + colspan)
                .filter(|index| self.widths[*index] != 0)
                .map(|index| self.widths[index])
                .sum();
            let share = even_widths(width.saturating_sub(known).max(1), unknown.len());
            for (slot, value) in unknown.into_iter().zip(share) {
                self.widths[slot] = value;
            }
        }
        let missing: Vec<usize> = (0..self.widths.len())
            .filter(|index| self.widths[*index] == 0)
            .collect();
        if !missing.is_empty() {
            let share = even_widths(NEW_TABLE_TOTAL_WIDTH, self.widths.len());
            for slot in missing {
                self.widths[slot] = share[slot];
            }
        }
    }

    /// (row, column) 을 덮고 있는 주 셀의 왼쪽 위 좌표.
    fn covering(&self, row: usize, column: usize) -> Option<(usize, usize, usize)> {
        for (r, slots) in self.rows.iter().enumerate() {
            for (c, slot) in slots.iter().enumerate() {
                let Some(index) = slot else { continue };
                let cell = &self.cells[*index];
                if r <= row && row < r + cell.rowspan && c <= column && column < c + cell.colspan {
                    return Some((r, c, *index));
                }
            }
        }
        None
    }

    fn insert_row(&mut self, index: usize) -> GridResult {
        if index > self.rows.len() {
            return Err(TableEditSkipReason::IndexOutOfRange);
        }
        if self.rows.len() + 1 > MAX_ROWS {
            return Err(TableEditSkipReason::SizeLimit);
        }
        // 이 자리를 세로 병합이 가로지르면 병합을 한 행 늘린다.
        // ⛔ 그 열은 새 행에서도 **그 병합 칸의 자리**다. 새 칸을 만들면 열이 하나 늘어난다
        // (3열 표에 4칸짜리 행이 생겼다). 앵커 바로 아래에 끼울 때만 드러나므로 전수 훑기가
        // 삽입 자리를 표 끝으로만 걸던 동안 안 보였다.
        let mut covered: Vec<Option<usize>> = vec![None; self.widths.len()];
        for (row, slots) in self.rows.iter().enumerate() {
            for (column, slot) in slots.iter().enumerate() {
                let Some(cell) = slot else { continue };
                if row < index && index < row + self.cells[*cell].rowspan {
                    covered[column] = Some(*cell);
                }
            }
        }
        for cell in covered.iter().flatten() {
            self.cells[*cell].rowspan += 1;
        }
        let mut fresh = Vec::with_capacity(self.widths.len());
        for slot in &covered {
            if let Some(cell) = slot {
                fresh.push(Some(*cell));
                continue;
            }
            self.cells.push(GridCell::blank());
            fresh.push(Some(self.cells.len() - 1));
        }
        self.rows.insert(index, fresh);
        self.reseat();
        Ok(0)
    }

    fn delete_row(&mut self, index: usize, document: &HmlDocument) -> GridResult {
        if index >= self.rows.len() {
            return Err(TableEditSkipReason::IndexOutOfRange);
        }
        if self.rows.len() == 1 {
            return Err(TableEditSkipReason::LastRow);
        }
        let doomed = self.anchors_confined_to_row(index);
        self.guard_images(&doomed, document)?;
        let discarded = self.count_text(&doomed, document);

        // 지우는 행에서 시작하지만 아래로 이어지는 병합은 다음 행으로 내려 준다.
        let mut moved: Vec<(usize, usize)> = Vec::new();
        for (column, slot) in self.rows[index].iter().enumerate() {
            if let Some(cell) = slot
                && self.cells[*cell].rowspan > 1
            {
                moved.push((column, *cell));
            }
        }
        for (column, cell) in moved {
            self.cells[cell].rowspan -= 1;
            self.rows[index + 1][column] = Some(cell);
        }
        for (row, slots) in self.rows.iter().enumerate() {
            if row == index {
                continue;
            }
            for slot in slots.iter().flatten() {
                if row < index && index < row + self.cells[*slot].rowspan {
                    self.cells[*slot].rowspan -= 1;
                }
            }
        }
        self.rows.remove(index);
        self.reseat();
        Ok(discarded)
    }

    fn insert_column(&mut self, index: usize) -> GridResult {
        let columns = self.widths.len();
        if index > columns {
            return Err(TableEditSkipReason::IndexOutOfRange);
        }
        if columns + 1 > MAX_COLUMNS {
            return Err(TableEditSkipReason::SizeLimit);
        }
        for row in 0..self.rows.len() {
            match self.covering(row, index.min(columns.saturating_sub(1))) {
                // 가로 병합 한가운데를 가르면 병합을 한 칸 넓힌다.
                Some((anchor_row, anchor_column, cell))
                    if index < columns && anchor_column < index =>
                {
                    if anchor_row == row {
                        self.cells[cell].colspan += 1;
                    }
                    self.rows[row].insert(index, None);
                }
                _ => {
                    self.cells.push(GridCell::blank());
                    let fresh = self.cells.len() - 1;
                    self.rows[row].insert(index, Some(fresh));
                }
            }
        }
        // 총폭을 지키려고 이웃 열의 폭을 반으로 나눈다.
        let neighbour = index.min(columns - 1);
        let width = self.widths[neighbour];
        let taken = width / 2;
        self.widths[neighbour] = width - taken;
        self.widths.insert(index, taken.max(1));
        self.reseat();
        Ok(0)
    }

    fn delete_column(&mut self, index: usize, document: &HmlDocument) -> GridResult {
        let columns = self.widths.len();
        if index >= columns {
            return Err(TableEditSkipReason::IndexOutOfRange);
        }
        if columns == 1 {
            return Err(TableEditSkipReason::LastColumn);
        }
        let doomed = self.anchors_confined_to_column(index);
        self.guard_images(&doomed, document)?;
        let discarded = self.count_text(&doomed, document);

        for row in 0..self.rows.len() {
            match self.rows[row][index] {
                Some(cell) if self.cells[cell].colspan > 1 => {
                    // 병합의 왼쪽 끝을 지운다. 병합을 한 칸 줄여 오른쪽으로 옮긴다.
                    self.cells[cell].colspan -= 1;
                    self.rows[row][index + 1] = Some(cell);
                }
                Some(_) => {}
                None => {
                    if let Some((anchor_row, anchor_column, cell)) = self.covering(row, index)
                        && anchor_column < index
                        && anchor_row == row
                    {
                        self.cells[cell].colspan -= 1;
                    }
                }
            }
            self.rows[row].remove(index);
        }
        // 폭은 왼쪽 이웃(없으면 오른쪽)이 물려받아 총폭을 지킨다.
        let width = self.widths.remove(index);
        let heir = if index == 0 { 0 } else { index - 1 };
        self.widths[heir] += width;
        self.reseat();
        Ok(discarded)
    }

    fn merge(
        &mut self,
        row: usize,
        column: usize,
        row_end: usize,
        column_end: usize,
    ) -> GridResult {
        let (row0, row1) = (row.min(row_end), row.max(row_end));
        let (column0, column1) = (column.min(column_end), column.max(column_end));
        if row1 >= self.rows.len() || column1 >= self.widths.len() {
            return Err(TableEditSkipReason::IndexOutOfRange);
        }
        if row0 == row1 && column0 == column1 {
            return Err(TableEditSkipReason::NoChange);
        }
        // 선택 밖으로 삐져나가는 병합이 걸리면 거절한다.
        for r in row0..=row1 {
            for c in column0..=column1 {
                let Some((anchor_row, anchor_column, cell)) = self.covering(r, c) else {
                    continue;
                };
                let cell = &self.cells[cell];
                if anchor_row < row0
                    || anchor_column < column0
                    || anchor_row + cell.rowspan - 1 > row1
                    || anchor_column + cell.colspan - 1 > column1
                {
                    return Err(TableEditSkipReason::MergeCrossesBoundary);
                }
            }
        }
        // 왼쪽 위 칸이 주 셀이 되고 나머지 칸의 글은 사라진다.
        let master = match self.rows[row0][column0] {
            Some(index) => index,
            None => {
                self.cells.push(GridCell::blank());
                self.cells.len() - 1
            }
        };
        let mut discarded = 0_u32;
        for r in row0..=row1 {
            for c in column0..=column1 {
                if let Some(index) = self.rows[r][c]
                    && index != master
                    && self.cells[index].id.is_some()
                {
                    discarded += 1;
                }
                self.rows[r][c] = None;
            }
        }
        self.cells[master].rowspan = row1 - row0 + 1;
        self.cells[master].colspan = column1 - column0 + 1;
        self.rows[row0][column0] = Some(master);
        self.reseat();
        Ok(discarded)
    }

    fn split(&mut self, row: usize, column: usize) -> GridResult {
        if row >= self.rows.len() || column >= self.widths.len() {
            return Err(TableEditSkipReason::IndexOutOfRange);
        }
        let Some((anchor_row, anchor_column, master)) = self.covering(row, column) else {
            return Err(TableEditSkipReason::NotMerged);
        };
        let (rowspan, colspan) = (self.cells[master].rowspan, self.cells[master].colspan);
        if rowspan == 1 && colspan == 1 {
            return Err(TableEditSkipReason::NotMerged);
        }
        self.cells[master].rowspan = 1;
        self.cells[master].colspan = 1;
        for row_offset in 0..rowspan {
            for column_offset in 0..colspan {
                if row_offset == 0 && column_offset == 0 {
                    continue;
                }
                self.cells.push(GridCell::blank());
                self.rows[anchor_row + row_offset][anchor_column + column_offset] =
                    Some(self.cells.len() - 1);
            }
        }
        self.reseat();
        Ok(0)
    }

    /// 격자를 다시 훑어 병합에 덮인 칸을 `None` 으로 맞춘다.
    // 격자는 행과 열 두 축을 함께 색인하므로 범위 반복이 읽기 쉽다.
    #[allow(clippy::needless_range_loop)]
    fn reseat(&mut self) {
        let rows = self.rows.len();
        let columns = self.widths.len();
        let mut anchors: Vec<(usize, usize, usize)> = Vec::new();
        for (row, slots) in self.rows.iter().enumerate() {
            for (column, slot) in slots.iter().enumerate() {
                if let Some(index) = slot {
                    anchors.push((row, column, *index));
                }
            }
        }
        // 같은 셀이 두 자리에 남아 있으면 첫 자리만 인정한다.
        let mut seen = BTreeSet::new();
        anchors.retain(|(_, _, index)| seen.insert(*index));
        self.rows = vec![vec![None; columns]; rows];
        for (row, column, index) in anchors {
            if row >= rows || column >= columns {
                continue;
            }
            let cell = &mut self.cells[index];
            cell.rowspan = cell.rowspan.clamp(1, rows - row);
            cell.colspan = cell.colspan.clamp(1, columns - column);
            self.rows[row][column] = Some(index);
        }
        // 병합 뒤에 남은 빈 칸을 새 셀로 메운다.
        let mut covered = vec![vec![false; columns]; rows];
        for row in 0..rows {
            for column in 0..columns {
                let Some(index) = self.rows[row][column] else {
                    continue;
                };
                let (rowspan, colspan) = (self.cells[index].rowspan, self.cells[index].colspan);
                for row_offset in 0..rowspan {
                    for column_offset in 0..colspan {
                        covered[row + row_offset][column + column_offset] = true;
                    }
                }
            }
        }
        for row in 0..rows {
            for column in 0..columns {
                if !covered[row][column] {
                    self.cells.push(GridCell::blank());
                    self.rows[row][column] = Some(self.cells.len() - 1);
                    covered[row][column] = true;
                }
            }
        }
    }

    fn anchors_confined_to_row(&self, row: usize) -> Vec<String> {
        self.rows[row]
            .iter()
            .flatten()
            .filter(|index| self.cells[**index].rowspan == 1)
            .filter_map(|index| self.cells[*index].id.clone())
            .collect()
    }

    fn anchors_confined_to_column(&self, column: usize) -> Vec<String> {
        (0..self.rows.len())
            .filter_map(|row| self.rows[row][column])
            .filter(|index| self.cells[*index].colspan == 1)
            .filter_map(|index| self.cells[index].id.clone())
            .collect()
    }

    /// 그림이 든 칸은 지우지 않는다. 도면을 조용히 잃는 편집을 만들지 않기 위해서다.
    fn guard_images(
        &self,
        ids: &[String],
        document: &HmlDocument,
    ) -> Result<(), TableEditSkipReason> {
        let carries = document
            .cells
            .iter()
            .any(|cell| ids.contains(&cell.id) && !cell.image_ids.is_empty());
        if carries {
            return Err(TableEditSkipReason::CellContainsImage);
        }
        Ok(())
    }

    fn count_text(&self, ids: &[String], document: &HmlDocument) -> u32 {
        document
            .cells
            .iter()
            .filter(|cell| ids.contains(&cell.id) && !cell.text.trim().is_empty())
            .count() as u32
    }

    // 격자는 행과 열 두 축을 함께 색인하므로 범위 반복이 읽기 쉽다.
    #[allow(clippy::needless_range_loop)]
    fn store(&mut self, document: &mut HmlDocument, table_id: &str) -> Result<(), CoreError> {
        let previous: BTreeMap<String, HmlCell> = document
            .cells
            .iter()
            .filter(|cell| cell.table_id == table_id)
            .map(|cell| (cell.id.clone(), cell.clone()))
            .collect();
        let starts: Vec<u32> = self
            .widths
            .iter()
            .scan(0_u32, |cursor, width| {
                let start = *cursor;
                *cursor += width;
                Some(start)
            })
            .collect();

        let mut fresh: Vec<HmlCell> = Vec::new();
        for row in 0..self.rows.len() {
            let mut occurrence = 0_u32;
            for column in 0..self.widths.len() {
                let Some(index) = self.rows[row][column] else {
                    continue;
                };
                let cell = self.cells[index].clone();
                let width: u32 = self.widths[column..column + cell.colspan].iter().sum();
                occurrence += 1;
                let mut record = self.materialize(
                    document,
                    table_id,
                    &previous,
                    cell.id.as_deref(),
                    row,
                    occurrence,
                );
                record.rowspan = cell.rowspan as u32;
                record.colspan = cell.colspan as u32;
                record.merge_continuation = false;
                record.width_raw = Some(width);
                record.start_raw = self.uses_start.then_some(starts[column]);
                record.column_begin = Some(column as u32 + 1);
                record.row_begin = Some(row as u32 + 1);
                fresh.push(record);

                // 세로 병합에 덮인 행마다 이어짐 셀을 하나씩 되돌린다.
                for row_offset in 1..cell.rowspan {
                    let mut continuation =
                        self.materialize(document, table_id, &previous, None, row + row_offset, 0);
                    continuation.rowspan = 1;
                    continuation.colspan = cell.colspan as u32;
                    continuation.merge_continuation = true;
                    continuation.width_raw = Some(width);
                    continuation.start_raw = self.uses_start.then_some(starts[column]);
                    continuation.column_begin = Some(column as u32 + 1);
                    continuation.row_begin = Some(row as u32 + 1);
                    fresh.push(continuation);
                }
            }
        }
        // 이어짐 셀이 제 행에 자리하도록 행 우선으로 다시 세운다.
        fresh.sort_by_key(|cell| (cell.row, cell.column_begin.unwrap_or(0)));
        for (row, group) in &mut fresh.chunk_by_mut(|a, b| a.row == b.row).enumerate() {
            for (position, cell) in group.iter_mut().enumerate() {
                cell.row = row as u32 + 1;
                cell.occurrence = position as u32 + 1;
            }
        }

        let kept: BTreeSet<&str> = fresh.iter().map(|cell| cell.id.as_str()).collect();
        let dropped: Vec<String> = previous
            .keys()
            .filter(|id| !kept.contains(id.as_str()))
            .cloned()
            .collect();
        if !dropped.is_empty() {
            let doomed: BTreeSet<&str> = dropped.iter().map(String::as_str).collect();
            document.images.retain(|image| {
                !image
                    .cell_id
                    .as_deref()
                    .is_some_and(|id| doomed.contains(id))
            });
        }

        document.cells.retain(|cell| cell.table_id != table_id);
        let table = document
            .tables
            .iter_mut()
            .find(|value| value.id == table_id)
            .ok_or_else(|| edit_error("missing_table"))?;
        table.cell_ids = fresh.iter().map(|cell| cell.id.clone()).collect();
        table.row_count = self.rows.len() as u32;
        table.max_columns = self.widths.len() as u32;
        document.cells.extend(fresh);
        refresh_summary(document);
        Ok(())
    }

    fn materialize(
        &mut self,
        document: &HmlDocument,
        table_id: &str,
        previous: &BTreeMap<String, HmlCell>,
        id: Option<&str>,
        row: usize,
        occurrence: u32,
    ) -> HmlCell {
        if let Some(existing) = id.and_then(|value| previous.get(value)) {
            let mut cell = existing.clone();
            cell.row = row as u32 + 1;
            cell.occurrence = occurrence;
            return cell;
        }
        let id = self.mint_id(document, previous, table_id);
        HmlCell {
            id,
            table_id: table_id.to_string(),
            row: row as u32 + 1,
            occurrence,
            text: String::new(),
            start_raw: None,
            width_raw: None,
            hwidth_raw: None,
            column_begin: None,
            row_begin: None,
            rowspan: 1,
            colspan: 1,
            merge_continuation: false,
            align: if self.template.align.is_empty() {
                "justified".to_string()
            } else {
                self.template.align.clone()
            },
            indent_raw: self.template.indent_raw,
            lspace: if self.template.lspace == 0 {
                280
            } else {
                self.template.lspace
            },
            image_ids: Vec::new(),
            inlines: Vec::new(),
        }
    }

    fn mint_id(
        &mut self,
        document: &HmlDocument,
        previous: &BTreeMap<String, HmlCell>,
        table_id: &str,
    ) -> String {
        let suffix = table_id.rsplit(':').next().unwrap_or("0").to_string();
        loop {
            self.next_id += 1;
            let candidate = format!("cell:{suffix}:new:{}", self.next_id);
            let clash = previous.contains_key(&candidate)
                || document.cells.iter().any(|cell| cell.id == candidate);
            if !clash {
                return candidate;
            }
        }
    }
}

fn unique_table_id(document: &HmlDocument, occurrence: u32) -> String {
    let mut suffix = occurrence;
    loop {
        let candidate = format!("table:{suffix}");
        if !document.tables.iter().any(|table| table.id == candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

fn refresh_summary(document: &mut HmlDocument) {
    document.summary.table_count = document.tables.len() as u64;
    document.summary.row_count = document
        .tables
        .iter()
        .map(|table| u64::from(table.row_count))
        .sum();
    document.summary.cell_count = document.cells.len() as u64;
    document.summary.image_count = document.images.len() as u64;
}

fn edit_error(detail: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::StructureInvalid, "table_edit", false);
    error.details.insert("detail".into(), detail.into());
    error
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_the_total_width_and_gives_the_remainder_to_the_last_column() {
        assert_eq!(even_widths(NEW_TABLE_TOTAL_WIDTH, 2), [4272, 4272]);
        assert_eq!(
            even_widths(NEW_TABLE_TOTAL_WIDTH, 5),
            [1708, 1708, 1708, 1708, 1712]
        );
        assert_eq!(
            even_widths(NEW_TABLE_TOTAL_WIDTH, 5).iter().sum::<u32>(),
            NEW_TABLE_TOTAL_WIDTH
        );
    }
}
