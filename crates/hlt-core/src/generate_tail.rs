use crate::{CoreError, ErrorCode, GenerationDialect, HltBodyPlan, HmlDocument};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedRowRecord {
    pub first_cell: u32,
    pub last_cell: u32,
    pub height: u32,
    pub table: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedCellRecord {
    pub row: u32,
    pub start: u32,
    pub width: u32,
    pub line_start: u32,
    pub line_end: u32,
    pub height: u32,
    pub span_flag: u32,
    pub rowspan: u32,
    pub colspan: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedTerminalCell {
    pub row: u32,
    pub start: u32,
    pub width: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedCellLineRange {
    pub cell_id: String,
    pub line_start: u32,
    pub line_end: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedTableTailPlan {
    pub rows: Vec<GeneratedRowRecord>,
    pub cells: Vec<GeneratedCellRecord>,
    pub terminal_rows: Vec<(u32, u32, u32)>,
    pub terminal_cells: Vec<GeneratedTerminalCell>,
}

pub fn generate_section_register(
    title_indices: &[u32],
    wipo_ids: &[Option<u32>],
    builtin: bool,
) -> Result<Vec<u8>, CoreError> {
    if title_indices.len() != wipo_ids.len() {
        return Err(tail_structure("register_length_mismatch"));
    }
    let mut out = if builtin {
        vec![0xbf, 0, 0, 0, 0]
    } else {
        vec![0xbf, 0, 0, 0]
    };
    for (line, title) in title_indices.iter().enumerate() {
        out.extend_from_slice(&title.to_le_bytes());
        if line + 1 < title_indices.len() {
            out.extend_from_slice(&wipo_ids[line + 1].unwrap_or(0).to_le_bytes());
        }
    }
    Ok(out)
}

pub fn generate_line_array(
    document: &HmlDocument,
    body: &HltBodyPlan,
) -> Result<Vec<u8>, CoreError> {
    let mut cell_numbers = BTreeMap::new();
    let mut number = 1_u32;
    for table in &document.tables {
        for cell_id in &table.cell_ids {
            if cell_numbers.insert(cell_id.as_str(), number).is_some() {
                return Err(tail_structure("duplicate_cell_id"));
            }
            number = number
                .checked_add(1)
                .ok_or_else(|| tail_structure("cell_index_overflow"))?;
        }
    }
    let mut out = Vec::with_capacity(body.lines.len() * 12);
    for line in &body.lines {
        let cell = line
            .cell_id
            .as_deref()
            .map(|id| {
                cell_numbers
                    .get(id)
                    .copied()
                    .ok_or_else(|| tail_structure("unknown_line_cell"))
            })
            .transpose()?
            .unwrap_or(0);
        out.extend_from_slice(&line.shape_index.to_le_bytes());
        out.extend_from_slice(&cell.to_le_bytes());
        out.extend_from_slice(&0_u32.to_le_bytes());
    }
    Ok(out)
}

pub fn plan_table_tail(
    document: &HmlDocument,
    dialect: GenerationDialect,
    ranges: &[GeneratedCellLineRange],
) -> Result<GeneratedTableTailPlan, CoreError> {
    let range_map: BTreeMap<_, _> = ranges.iter().map(|r| (r.cell_id.as_str(), r)).collect();
    let cell_map: BTreeMap<_, _> = document.cells.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut rows = Vec::new();
    let mut cells = Vec::new();
    let mut global_row = 0_u32;
    let mut last_rows: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    let mut sentinel_rows: BTreeMap<u32, u32> = BTreeMap::new();
    for table in &document.tables {
        let mut row_groups: BTreeMap<u32, Vec<_>> = BTreeMap::new();
        for id in &table.cell_ids {
            let cell = *cell_map
                .get(id.as_str())
                .ok_or_else(|| tail_structure("missing_cell"))?;
            row_groups.entry(cell.row).or_default().push(cell);
        }
        let heights: Vec<u32> = row_groups
            .values()
            .map(|items| {
                if items.iter().any(|c| c.text.contains('\n')) {
                    207
                } else {
                    117
                }
            })
            .collect();
        for (local, (row_number, items)) in row_groups.iter().enumerate() {
            global_row += 1;
            let first = cells.len() as u32 + 1;
            let mut cursor = 0_u32;
            let mut indices = Vec::new();
            for cell in items {
                let width = cell
                    .width_raw
                    .or_else(|| cell.hwidth_raw.map(crate::colwidth_to_cell_width))
                    .ok_or_else(|| tail_structure("missing_width"))?;
                let start = cell.start_raw.unwrap_or(cursor);
                let lines = *range_map
                    .get(cell.id.as_str())
                    .ok_or_else(|| tail_structure("missing_line_range"))?;
                let edge = local == 0 || local + 1 == heights.len();
                // ⛔ 국내 값 104/101 은 **의견서(서식 111) 상수**였다. 서식 140 실물에는
                // 안 나온다(74건 전수: 140 은 96×430, 97×108, 98×24).
                // 국내 출원서도 가장자리 98, 가운데 97 이다.
                let base: u32 = match (dialect, edge) {
                    (GenerationDialect::PctPatent, true) => 101,
                    (GenerationDialect::PctPatent, false) => 99,
                    (GenerationDialect::PatentSpecificationKo, true) => 98,
                    (GenerationDialect::PatentSpecificationKo, false) => 97,
                };
                let height = if cell.merge_continuation {
                    0
                } else {
                    heights
                        .iter()
                        .skip(local + 1)
                        .take(cell.rowspan.saturating_sub(1) as usize)
                        .try_fold(base, |v, h| {
                            v.checked_add(*h)
                                .ok_or_else(|| tail_structure("height_overflow"))
                        })?
                };
                indices.push(cells.len());
                cells.push(GeneratedCellRecord {
                    row: global_row,
                    start,
                    width,
                    line_start: lines.line_start,
                    line_end: lines.line_end,
                    height,
                    span_flag: if cell.merge_continuation { 56 } else { 32 },
                    rowspan: cell.rowspan,
                    colspan: cell.colspan,
                });
                cursor = start + width;
            }
            rows.push(GeneratedRowRecord {
                first_cell: first,
                last_cell: cells.len() as u32,
                height: heights[local],
                table: table.occurrence,
            });
            if local + 1 == row_groups.len() {
                last_rows.insert(table.occurrence, indices);
            }
            let _ = row_number;
        }
        // 이 표의 센티널 줄 자리 — 다음 표의 줄은 그 뒤부터 센다(줄 레코드 배열에 센티널이 표마다 끼기 때문).
        // 안 세면 둘째 표부터 칸이 앞 표 센티널 자리를 가리켜 통특기에서 여러 줄 표가 흐트러졌다.
        global_row += 1;
        sentinel_rows.insert(table.occurrence, global_row);
    }
    // ⛔ **표 뒤에는 센티널 행이 붙는다. PCT 만이 아니다.**
    // 국내 서식에 안 달았더니 통특기가 **마지막 실제 행을 표 종결자로 먹었다**
    // (사용자 판정 2026-08-27: 12행으로 구운 표가 11행까지만 보였다).
    // 통특기가 만든 국내 파일 두 건에서 표 뒤에 높이 0, 줄 번호 `0xFFFFFFFF` 인 칸 한 줄과
    // 높이 0 인 행 레코드 하나가 확인된다.
    //
    // ⚠️ start 오프셋의 규칙은 모른다. 문서마다 갈린다:
    // 국내 출원서는 두 표 다 +3, 보정 명세서는 +1,
    // PCT 는 +1080 이다. 열 수나 행 수와도 안 맞는다. 렌더에 걸리는 것은 센티널의
    // **존재**로 보인다(PCT 가 +1080 으로 통특기 판정을 통과했다).
    let sentinel_gap = match dialect {
        GenerationDialect::PctPatent => 1080,
        GenerationDialect::PatentSpecificationKo => 3,
    };
    let mut terminal_rows = Vec::new();
    let mut terminal_cells = Vec::new();
    {
        let mut next = cells.len() as u32 + 1;
        for table in &document.tables {
            let sentinel = sentinel_rows[&table.occurrence];
            let indices = last_rows
                .get(&table.occurrence)
                .ok_or_else(|| tail_structure("missing_last_row"))?;
            let first = next;
            for index in indices {
                let cell = &cells[*index];
                terminal_cells.push(GeneratedTerminalCell {
                    row: sentinel,
                    start: sentinel_gap + cell.start,
                    width: cell.width,
                });
                next += 1;
            }
            terminal_rows.push((table.occurrence, first, next - 1));
        }
    }
    Ok(GeneratedTableTailPlan {
        rows,
        cells,
        terminal_rows,
        terminal_cells,
    })
}

pub fn generate_cell_table(
    rows: &[GeneratedRowRecord],
    terminal_rows: &[(u32, u32, u32)],
    native_pct: bool,
) -> Result<Vec<u8>, CoreError> {
    let count = 1_usize
        .checked_add(rows.len())
        .and_then(|n| n.checked_add(terminal_rows.len()))
        .ok_or_else(tail_error)?;
    let mut out = tagged(count)?;
    push(
        &mut out,
        [
            1,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            u32::MAX,
            u32::MAX,
            0,
            0,
        ],
    );
    let mut groups: BTreeMap<u32, Vec<(usize, &GeneratedRowRecord)>> = BTreeMap::new();
    for (i, row) in rows.iter().enumerate() {
        groups.entry(row.table).or_default().push((i, row));
    }
    let terminals: BTreeMap<u32, (u32, u32)> = terminal_rows
        .iter()
        .map(|(t, f, l)| (*t, (*f, *l)))
        .collect();
    // 센티널이 표 사이에 끼므로 행 번호를 배열 자리(`index`)로 세야 한다.
    // 예전엔 이것을 `pct` 라 불렀는데 국내 서식도 센티널을 달게 됐다.
    let numbered_by_slot = !terminal_rows.is_empty();
    let mut index = 1_u32;
    for (table, recs) in groups {
        for (j, (global, row)) in recs.iter().enumerate() {
            let (prev, next) = if numbered_by_slot {
                (
                    if j == 0 { u32::MAX } else { index - 1 },
                    if j + 1 == recs.len() {
                        u32::MAX
                    } else {
                        index + 1
                    },
                )
            } else {
                (
                    if j == 0 { u32::MAX } else { *global as u32 },
                    if j + 1 == recs.len() {
                        u32::MAX
                    } else {
                        *global as u32 + 2
                    },
                )
            };
            // ⛔ `f10` 은 칸의 `f11` 과 짝이다. 국내 서식 140 정본은 둘 다 15 고
            // `amend_a` 은 둘 다 10 이다. 40 은 의견서(111) 값이었다.
            push(
                &mut out,
                [
                    1,
                    0,
                    row.first_cell,
                    row.last_cell,
                    0,
                    row.height,
                    prev,
                    next,
                    0,
                    0,
                    if native_pct { 40 } else { 15 },
                    0,
                    0,
                    0,
                    0,
                    0,
                    u32::MAX,
                    u32::MAX,
                    0,
                    0,
                ],
            );
            index += 1;
        }
        if let Some((first, last)) = terminals.get(&table) {
            push(
                &mut out,
                // ⛔ 센티널 행 레코드도 그 표의 정상 행과 같은 상수를 쓴다. 방언과 무관하게
                // PCT 상수를 박아 두고 있었다(국내 파일 안에 PCT 모양 레코드가 섞였다).
                // `f8` 은 센티널 start 오프셋과 같다(PCT 1080, 국내 3. 4절 "센티널 행").
                if native_pct {
                    [
                        1,
                        0,
                        *first,
                        *last,
                        0,
                        0,
                        index - 1,
                        u32::MAX,
                        1080,
                        0,
                        108,
                        15,
                        10,
                        10,
                        10,
                        10,
                        u32::MAX,
                        u32::MAX,
                        0,
                        0,
                    ]
                } else {
                    [
                        1,
                        0,
                        *first,
                        *last,
                        0,
                        0,
                        index - 1,
                        u32::MAX,
                        3,
                        0,
                        15,
                        0,
                        0,
                        0,
                        0,
                        0,
                        u32::MAX,
                        u32::MAX,
                        0,
                        0,
                    ]
                },
            );
            index += 1;
        }
    }
    Ok(out)
}

pub fn generate_font_table(
    cells: &[GeneratedCellRecord],
    terminals: &[GeneratedTerminalCell],
    native_pct: bool,
) -> Result<Vec<u8>, CoreError> {
    let count = 1_usize
        .checked_add(cells.len())
        .and_then(|n| n.checked_add(terminals.len()))
        .ok_or_else(tail_error)?;
    let mut out = tagged(count)?;
    push(
        &mut out,
        [
            1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1703, 0, 0, 0, 0, 0,
        ],
    );
    let mut groups: BTreeMap<u32, Vec<(usize, &GeneratedCellRecord)>> = BTreeMap::new();
    for (i, c) in cells.iter().enumerate() {
        groups.entry(c.row).or_default().push((i, c));
    }
    for recs in groups.values() {
        for (j, (i, c)) in recs.iter().enumerate() {
            let prev = if j == 0 { u32::MAX } else { *i as u32 };
            let next = if j + 1 == recs.len() {
                u32::MAX
            } else {
                *i as u32 + 2
            };
            if native_pct {
                push(
                    &mut out,
                    [
                        1,
                        c.row,
                        next,
                        prev,
                        c.start,
                        c.width,
                        15,
                        10,
                        10,
                        10,
                        10,
                        108,
                        c.height,
                        c.line_start,
                        c.line_end,
                        0,
                        if c.span_flag == 32 { 0 } else { c.span_flag },
                        c.rowspan,
                        c.colspan,
                        0,
                    ],
                );
            } else {
                // ⛔ `(15,15,15,15,15,40)` 은 **의견서(서식 111) 상수**였다. 서식 140 실물에는
                // 한 번도 안 나온다(74건 전수: 140 은 `(15,2,2,2,2,10)`×500 과
                // `(15,5,5,5,5,15)`×144 뿐). 국내 출원서가 뒤쪽이라 그것을 쓴다.
                // ⛔ `f16` 도 마찬가지다. 서식 140 실물 **665개가 전부 0** 이고 32 는 하나도 없다
                // (32 는 의견서 287개 전부와 PCT 일부다). PCT 경로가 이미 하던 대로 0 을 쓴다.
                push(
                    &mut out,
                    [
                        1,
                        c.row,
                        next,
                        prev,
                        c.start,
                        c.width,
                        15,
                        5,
                        5,
                        5,
                        5,
                        15,
                        c.height,
                        c.line_start,
                        c.line_end,
                        0,
                        if c.span_flag == 32 { 0 } else { c.span_flag },
                        c.rowspan,
                        c.colspan,
                        0,
                    ],
                );
            }
        }
    }
    let mut groups: BTreeMap<u32, Vec<&GeneratedTerminalCell>> = BTreeMap::new();
    for c in terminals {
        groups.entry(c.row).or_default().push(c);
    }
    let mut index = 1_u32 + cells.len() as u32;
    for recs in groups.values() {
        for (j, c) in recs.iter().enumerate() {
            let prev = if j == 0 { u32::MAX } else { index - 1 };
            let next = if j + 1 == recs.len() {
                u32::MAX
            } else {
                index + 1
            };
            // ⛔ **센티널 칸도 그 표의 정상 칸과 같은 상수를 쓴다.** 방언과 무관하게 PCT 상수
            // (`15,10,10,10,10,108`)를 박아 두고 있었다. 국내 서식에 센티널을 달기 시작하니
            // 국내 파일 안에 PCT 모양 레코드가 섞였다.
            // 통특기 국내 출원서의 센티널 칸은 그 표의 정상 칸과
            // 똑같은 `15,5,5,5,5,15` 다.
            let (pad, style) = if native_pct {
                ([15, 10, 10, 10, 10], 108)
            } else {
                ([15, 5, 5, 5, 5], 15)
            };
            push(
                &mut out,
                [
                    1,
                    c.row,
                    next,
                    prev,
                    c.start,
                    c.width,
                    pad[0],
                    pad[1],
                    pad[2],
                    pad[3],
                    pad[4],
                    style,
                    0,
                    u32::MAX,
                    u32::MAX,
                    0,
                    0,
                    1,
                    1,
                    0,
                ],
            );
            index += 1;
        }
    }
    Ok(out)
}
fn tagged(count: usize) -> Result<Vec<u8>, CoreError> {
    let count = u32::try_from(count).map_err(|_| tail_error())?;
    let mut out = vec![0xbf];
    out.extend_from_slice(&count.to_le_bytes());
    Ok(out)
}
fn push(out: &mut Vec<u8>, words: [u32; 20]) {
    for word in words {
        out.extend_from_slice(&word.to_le_bytes());
    }
}
fn tail_error() -> CoreError {
    CoreError::new(ErrorCode::SizeLimitExceeded, "generate_tail", false)
}
fn tail_structure(reason: &str) -> CoreError {
    let mut e = CoreError::new(ErrorCode::GeometryInvalid, "generate_tail", false);
    e.details.insert("reason".into(), reason.into());
    e
}
