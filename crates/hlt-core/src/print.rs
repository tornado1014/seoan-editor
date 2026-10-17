//! 인쇄 쪽 배치(P4-4) — 쪽 모델(`paginate`)과 인쇄 줄나눔(`reflow`) 위에 글자 자리를 놓는다.
//! 화면(편집기)과 PDF 가 같은 배치를 그린다. 판정 = 통특기 인쇄 PDF(`page-truth/*.pdf`)의 글자 줄 상자.
//!
//! 기준(Microsoft Print To PDF · A4 595.32×841.92pt):
//! - 본문 왼쪽 49.80pt · 폭 467.5pt(px600 3,896) · 글자 바탕체 12pt 고정폭(한글 12 · ASCII 6).
//! - 쪽마다 첫 글자 줄 윗변 115.58pt · 줄 단위 u = 0.11871pt.
//! - 양쪽 맞춤 = 남는 폭을 낱말 사이 공백에 고루 나눈다(문단 끝 줄은 안 편다). 가운데 = 들여쓰기 뒤 남는 폭의 가운데.
//! - 쪽번호 「전체-쪽」 10pt · 윗변 798.93pt · 가운데 283.3pt.

use std::collections::BTreeMap;

use crate::{
    InspectDocument,
    paginate::{BREAK, PrintedLine, TEXT_ASCENT, line_heights, paginate},
    reflow::{self, Item},
};

pub const PAGE_WIDTH: f64 = 595.32;
pub const PAGE_HEIGHT: f64 = 841.92;
const LEFT: f64 = 49.80;
const FIRST_TOP: f64 = 115.58;
const PT_PER_UNIT: f64 = 0.11871;
const PT_PER_PX600: f64 = 72.0 / 600.0;
const FONT_SIZE: f64 = 12.0;
/// 첨자 글자 크기(ASCII 폭 31/50).
const SUB_SIZE: f64 = FONT_SIZE * 31.0 / 50.0;
/// 표: 칸 글자 10pt · 행 안 줄 피치 14.04pt(118.27u) · 칸 글자 왼쪽 = 본문 왼쪽 + 행 들여쓰기 + 칸 안 여백 + 칸 시작 + 칸 줄 첫 줄 들여쓰기
/// (pct 109.2 · pct_a 51.84 · domestic 90.48 — 마지막은 0.22pt 남는다).
const CELL_SIZE: f64 = 10.0;
const CELL_PITCH: f64 = 14.04;
/// 행 윗변 → 첫 칸 글자 윗변(u). domestic 【표 1】 첫 행 548.61pt 로 맞춘 값.
const CELL_TEXT_OFFSET: f64 = 0.0;
const ROW_UNIT: f64 = 118.27;
const TABLE_PAD_TOP: f64 = 4.0;
const FOOTER_TOP: f64 = 798.93;
const DRAW_INSET_WIDTH: f64 = 0.76;
/// 칸 그림 윗변 − 칸 글자 윗변(pt). pct_a 표 그림으로 맞춘다.
const CELL_IMAGE_DROP: f64 = 0.0;
const DRAW_INSET_HEIGHT: f64 = 0.68;
const IMAGE_TRIM: f64 = 0.084 / PT_PER_UNIT;
const IMAGE_TRIM_DESCENT: f64 = 0.119 / PT_PER_UNIT;
const IMAGE_LINE_GAP: f64 = 170.0;
const IMAGE_RAISE: f64 = 0.02;
const FOOTER_CENTER: f64 = 283.30;
const FOOTER_SIZE: f64 = 10.0;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PagePlan {
    pub width: f64,
    pub height: f64,
    pub pages: Vec<PlannedPage>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PlannedPage {
    pub number: usize,
    pub lines: Vec<PlannedLine>,
    pub images: Vec<PlannedImage>,
}

/// 그림 하나. `object_code` = `InspectDocument::image_payloads` 의 키. 크기 = 표시 크기 − (0.76, 0.68)pt.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PlannedImage {
    pub object_code: u32,
    pub x: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

/// 글자 한 줄. `top` = 글자 상자 윗변(pt · 쪽 위 기준).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PlannedLine {
    pub top: f64,
    pub glyphs: Vec<PlannedGlyph>,
    /// 이 줄의 문단(`ParagraphRecord::id`) — 편집기가 클릭을 문단으로 바꾼다. 쪽번호 줄은 `None`.
    pub paragraph: Option<String>,
    /// 표 칸 줄이면 그 칸(`CellRecord::id`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
    /// 편집 커서용: 이 줄 첫 글자의 문단 글 안 자리(글자 수). 모르면(표 칸 · 표 뒤 묶음 · 쪽번호) `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<u32>,
    /// 편집 커서용: 글자 경계 x(pt) — `stops[i]` = 줄의 i 번째 글자(공백 · 그림 포함) 앞, 마지막 = 줄 끝. 글자 수 + 1 개.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stops: Vec<f64>,
    /// 밑줄 구간 `[x 시작, x 끝]`(pt) — 밑줄 글자와 그 사이 **공백까지** 잇는다(양쪽 정렬로 넓어진 공백 폭 포함).
    /// 글자마다 그리면 공백(잉크 없는 칸이라 글리프가 없다)에서 밑줄이 끊겼다(2026-10-16 사용자).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub underlines: Vec<[f64; 2]>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PlannedGlyph {
    pub ch: char,
    pub x: f64,
    pub size: f64,
    /// 글자모양 비트: 밑줄 1 · 굵게 2 · 기울임 4 · 위첨자 16 · 아래첨자 32.
    pub style: u32,
    /// 줄 윗변에서 이 글자 상자 윗변까지(pt). 아래첨자 +3.91 · 위첨자 −0.04.
    pub drop: f64,
}

pub const UNDERLINE: u32 = 1;
pub const BOLD: u32 = 2;
pub const ITALIC: u32 = 4;
pub const SUPERSCRIPT: u32 = 16;
pub const SUBSCRIPT: u32 = 32;
const STYLE_BITS: u32 = UNDERLINE | BOLD | ITALIC | SUPERSCRIPT | SUBSCRIPT;
pub(crate) const SUBSCRIPT_DROP: f64 = 3.91;
const SUPERSCRIPT_DROP: f64 = -0.04;

/// 인쇄 선택 사항. 통특기 인쇄 대화상자의 「쪽 번호」 — 끄면 꼬리 쪽번호가 없고 본문이 44.52pt 위로 간다(pct.pdf).
#[derive(Debug, Clone, Copy)]
pub struct PrintOptions {
    pub page_numbers: bool,
}

impl Default for PrintOptions {
    fn default() -> Self {
        Self { page_numbers: true }
    }
}

/// 쪽번호를 끈 인쇄의 본문 첫 줄 윗변(pct.pdf 71.06pt).
const FIRST_TOP_WITHOUT_NUMBERS: f64 = 71.06;

pub fn page_plan(document: &InspectDocument, inflated: &[u8], options: PrintOptions) -> PagePlan {
    let first_top = if options.page_numbers {
        FIRST_TOP
    } else {
        FIRST_TOP_WITHOUT_NUMBERS
    };
    let (heights, keep, pad, printed) = line_heights(document, inflated);
    let pages = paginate(&heights, &keep, &pad);
    let total = pages.last().copied().unwrap_or(1);
    let mut by_line: BTreeMap<usize, Vec<&PrintedLine>> = BTreeMap::new();
    for line in &printed {
        by_line.entry(line.line).or_default().push(line);
    }
    let align: BTreeMap<&str, &str> = document
        .shapes
        .iter()
        .map(|shape| (shape.id.as_str(), shape.align.as_str()))
        .collect();
    let mut plan: Vec<PlannedPage> = (1..=total)
        .map(|number| PlannedPage {
            number,
            lines: Vec::new(),
            images: Vec::new(),
        })
        .collect();
    let rows = table_rows(document);
    // 쪽 나누기는 정수 u(paginate)로, 그리기는 표 행의 실제 높이(118.27u × 줄)로 쌓는다 — 반올림이 행마다 0.03pt 씩 밀린다.
    let (mut current, mut used) = (0_usize, 0.0_f64);
    for (line, &height) in heights.iter().enumerate() {
        let page = pages[line];
        if page != current {
            current = page;
            used = 0.0;
        }
        if height == BREAK {
            continue;
        }
        match by_line.get(&line) {
            Some(lines) => {
                let shape_align = document.lines[line]
                    .shape_id
                    .as_deref()
                    .and_then(|id| align.get(id).copied())
                    .unwrap_or("justified");
                for printed in lines {
                    let top = first_top + (used + printed.ascent - TEXT_ASCENT) * PT_PER_UNIT;
                    let line_top = first_top + used * PT_PER_UNIT;
                    let (mut planned, images) = place_line(printed, shape_align, top);
                    if let Some(planned) = planned.as_mut() {
                        planned.paragraph = Some(document.lines[line].paragraph_id.clone());
                    }
                    plan[page - 1].lines.extend(planned);
                    // 그림 아랫변 = 기준선(줄 위 + 오름). 그림 윗변 = 아랫변 − 그려진 높이 · 0.02pt 위(domestic 182.04 대 글자 182.06).
                    for (x, index, height) in images {
                        let image = &document.images[index];
                        plan[page - 1].images.push(PlannedImage {
                            object_code: image.object_code,
                            x,
                            top: line_top + (printed.ascent - height) * PT_PER_UNIT - IMAGE_RAISE,
                            width: (f64::from(image.display_width) - DRAW_INSET_WIDTH).max(0.0),
                            height: if image.display_height > 0 {
                                f64::from(image.display_height) - DRAW_INSET_HEIGHT
                            } else {
                                height * PT_PER_UNIT
                            },
                        });
                    }
                    // 그림 줄은 쪽 모델(그림 + 내림 + 170u)보다 조금 낮게 그린다 — 그림 줄 1,823개의 평균
                    // 차이가 내림 없음 0.084pt · 내림 있음 0.119pt(줄마다 ±0.18 은 프린터 칸 반올림이라 못 맞춘다).
                    // 쪽 나누기는 쪽 모델 그대로다.
                    let descent = printed.height as f64 - printed.ascent - IMAGE_LINE_GAP > 0.5;
                    let drawn = if printed.images.is_empty() {
                        printed.height as f64
                    } else if descent {
                        printed.height as f64 - IMAGE_TRIM_DESCENT
                    } else {
                        printed.height as f64 - IMAGE_TRIM
                    };
                    used += drawn;
                }
            }
            None => {
                if let Some(row) = rows.get(&line) {
                    let pad = if row.first { TABLE_PAD_TOP } else { 0.0 };
                    let top = first_top + (used + pad + CELL_TEXT_OFFSET) * PT_PER_UNIT;
                    let (lines, images) = place_row(row, top, inflated, document);
                    plan[page - 1].lines.extend(lines);
                    plan[page - 1].images.extend(images);
                    used += pad + ROW_UNIT * row.lines as f64;
                } else {
                    used += height as f64;
                }
            }
        }
        used += pad.get(&line).copied().unwrap_or(0) as f64;
    }
    for page in plan.iter_mut().filter(|_| options.page_numbers) {
        let label = format!("{total}-{}", page.number);
        let width = label.len() as f64 * FOOTER_SIZE / 2.0;
        let start = FOOTER_CENTER - width / 2.0;
        page.lines.push(PlannedLine {
            top: FOOTER_TOP,
            glyphs: label
                .chars()
                .enumerate()
                .map(|(index, ch)| PlannedGlyph {
                    ch,
                    x: start + index as f64 * FOOTER_SIZE / 2.0,
                    size: FOOTER_SIZE,
                    style: 0,
                    drop: 0.0,
                })
                .collect(),
            paragraph: None,
            cell: None,
            start: None,
            stops: Vec::new(),
            underlines: Vec::new(),
        });
    }
    PagePlan {
        width: PAGE_WIDTH,
        height: PAGE_HEIGHT,
        pages: plan,
    }
}

/// 글자 줄과 그림들(x pt, `document.images` 차례, 그려진 높이 u).
fn place_line(
    line: &PrintedLine,
    align: &str,
    top: f64,
) -> (Option<PlannedLine>, Vec<(f64, usize, f64)>) {
    let items = &line.items;
    // 글자 없는 줄(빈 문단)도 커서가 설 줄은 낸다 — 글리프가 없어 인쇄엔 안 보인다.
    if !items.iter().any(|(_, item, _)| item.has_ink()) {
        let line = line.start.map(|start| {
            let x = LEFT + line.indent * PT_PER_PX600;
            PlannedLine {
                top,
                glyphs: Vec::new(),
                paragraph: None,
                cell: None,
                start: Some(start as u32),
                stops: vec![x; items.len() + 1],
                underlines: Vec::new(),
            }
        });
        return (line, Vec::new());
    }
    // 자연 폭(끝 공백 제외)과 칸마다 시작 자리(px600).
    let mut starts = Vec::with_capacity(items.len());
    let (mut width, mut natural) = (line.indent, line.indent);
    for (_, item, _) in items {
        starts.push(width);
        if matches!(item, Item::Ascii(b'\t', _)) {
            width = reflow::next_tab(width);
            continue;
        }
        width += item.width();
        if item.has_ink() {
            natural = width;
        }
    }
    let first_ink = items
        .iter()
        .position(|(_, item, _)| item.has_ink())
        .unwrap_or(0);
    let last_ink = items
        .iter()
        .rposition(|(_, item, _)| item.has_ink())
        .unwrap_or(0);
    let spare = (reflow::WIDTH - natural).max(0.0);
    let (offset, per_space) = match align {
        // 들여쓰기는 칸 시작에 이미 들었다 — 남는 폭의 반만 민다(【발명의 설명】 258.7pt).
        "center" => (spare / 2.0, 0.0),
        "right" => (spare, 0.0),
        "justified" if !line.last => {
            let spaces = items[first_ink..last_ink]
                .iter()
                .filter(|(_, item, _)| matches!(item, Item::Ascii(b' ', _)))
                .count();
            (
                0.0,
                if spaces > 0 {
                    spare / spaces as f64
                } else {
                    0.0
                },
            )
        }
        _ => (0.0, 0.0),
    };
    let mut glyphs = Vec::new();
    let mut stops = Vec::with_capacity(items.len() + 1);
    let mut extra = 0.0;
    for (index, (ch, item, attr)) in items.iter().enumerate() {
        if index > first_ink
            && index <= last_ink
            && matches!(items[index - 1].1, Item::Ascii(b' ', _))
        {
            extra += per_space;
        }
        stops.push(LEFT + (starts[index] + offset + extra) * PT_PER_PX600);
        if !item.has_ink() || matches!(item, Item::Image(_)) {
            continue;
        }
        let sub = matches!(item, Item::Ascii(_, true) | Item::Wide(true));
        glyphs.push(PlannedGlyph {
            ch: *ch,
            x: LEFT + (starts[index] + offset + extra) * PT_PER_PX600,
            size: if sub { SUB_SIZE } else { FONT_SIZE },
            style: *attr & STYLE_BITS,
            drop: if attr & SUBSCRIPT != 0 {
                SUBSCRIPT_DROP
            } else if attr & SUPERSCRIPT != 0 {
                SUPERSCRIPT_DROP
            } else {
                0.0
            },
        });
    }
    let images = line
        .images
        .iter()
        .map(|&(at, index, height)| {
            let extra_at = if per_space > 0.0 {
                items[first_ink.min(at)..at]
                    .iter()
                    .filter(|(_, item, _)| matches!(item, Item::Ascii(b' ', _)))
                    .count() as f64
                    * per_space
            } else {
                0.0
            };
            (
                LEFT + (starts[at] + offset + extra_at) * PT_PER_PX600,
                index,
                height,
            )
        })
        .collect();
    stops.push(LEFT + (width + offset + extra) * PT_PER_PX600);
    let mut underlines: Vec<[f64; 2]> = Vec::new();
    for (index, (_, item, attr)) in items.iter().enumerate() {
        if attr & UNDERLINE == 0 || matches!(item, Item::Image(_)) {
            continue;
        }
        let (from, to) = (stops[index], stops[index + 1]);
        match underlines.last_mut() {
            Some(last) if (last[1] - from).abs() < 1e-6 => last[1] = to,
            _ => underlines.push([from, to]),
        }
    }
    let start = line.start;
    // 그림만 있는 줄(도면)도 커서 자리는 낸다.
    let line = (!glyphs.is_empty() || start.is_some()).then_some(PlannedLine {
        top,
        glyphs,
        paragraph: None,
        cell: None,
        start: start.map(|start| start as u32),
        stops: if start.is_some() { stops } else { Vec::new() },
        underlines,
    });
    (line, images)
}

/// 표 행 하나: 칸들(칸 시작 pt · 줄 차례들 · 세로 가운데인가)과 행 줄 수. 행 첫 줄(높이가 실린 줄)로 찾는다.
struct TableRow<'a> {
    left: f64,
    cells: Vec<(&'a crate::CellRecord, Vec<usize>)>,
    lines: usize,
    first: bool,
}

fn table_rows(document: &InspectDocument) -> BTreeMap<usize, TableRow<'_>> {
    let index = |id: &str| id.rsplit(':').next().and_then(|n| n.parse::<usize>().ok());
    let mut grouped: BTreeMap<(&str, u32), Vec<&crate::CellRecord>> = BTreeMap::new();
    for cell in &document.cells {
        grouped
            .entry((cell.table_id.as_str(), cell.row))
            .or_default()
            .push(cell);
    }
    let mut rows = BTreeMap::new();
    for cells in grouped.values() {
        let cells: Vec<_> = cells
            .iter()
            .map(|cell| {
                (
                    *cell,
                    cell.line_ids
                        .iter()
                        .filter_map(|id| index(id))
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        let Some(first) = cells
            .iter()
            .flat_map(|(_, lines)| lines.iter().copied())
            .min()
        else {
            continue;
        };
        let lines = cells
            .iter()
            .map(|(_, lines)| lines.len())
            .max()
            .unwrap_or(1);
        rows.insert(
            first,
            TableRow {
                left: LEFT
                    + f64::from(
                        document
                            .cell_text_offsets
                            .get(&(first as u64))
                            .copied()
                            .unwrap_or(0),
                    ) / 20.0,
                first: cells.iter().all(|(cell, _)| cell.row == 0),
                cells,
                lines,
            },
        );
    }
    rows
}

fn place_row(
    row: &TableRow<'_>,
    top: f64,
    inflated: &[u8],
    document: &InspectDocument,
) -> (Vec<PlannedLine>, Vec<PlannedImage>) {
    let mut out = Vec::new();
    let mut images = Vec::new();
    for (cell, lines) in &row.cells {
        // 세로 가운데(1)면 남는 줄의 반만큼 내린다.
        let shift = if cell.vertical_align == 1 {
            (row.lines - lines.len()) as f64 * CELL_PITCH / 2.0
        } else {
            0.0
        };
        for (k, &line) in lines.iter().enumerate() {
            let range = &document.lines[line].source_range;
            let raw = inflated
                .get(range.start_byte as usize..range.end_byte as usize)
                .unwrap_or(&[]);
            let (text, _, _) = encoding_rs::EUC_KR.decode(raw);
            let line_id = &document.lines[line].id;
            let line_start = range.start_byte;
            // 칸 줄의 첫 줄 들여쓰기(문단모양)도 칸 안에서 먹는다(domestic 표 = 800).
            let indent = if k == 0 {
                document.lines[line]
                    .shape_id
                    .as_deref()
                    .and_then(|id| document.shapes.iter().find(|shape| shape.id == id))
                    .map_or(0.0, |shape| f64::from(shape.indent_raw as i32) / 20.0)
            } else {
                0.0
            };
            let mut x = row.left + f64::from(cell.start) / 20.0 + indent;
            let mut glyphs = Vec::new();
            // 칸 안 그림: 그림 자리표(원문 1바이트)를 만나면 그 자리에 놓는다. 윗변 = 칸 글자 윗변 + CELL_IMAGE_DROP.
            let mut marks: Vec<&crate::ImageOccurrence> = document
                .images
                .iter()
                .filter(|image| &image.line_id == line_id)
                .collect();
            marks.sort_by_key(|image| image.source_range.start_byte);
            let mut marks = marks.into_iter().peekable();
            let mut byte = line_start;
            for ch in text.chars() {
                let len = if ch.is_ascii() { 1 } else { 2 };
                if let Some(image) = marks.next_if(|image| image.source_range.start_byte == byte) {
                    let width = (f64::from(image.display_width) - DRAW_INSET_WIDTH).max(0.0);
                    images.push(PlannedImage {
                        object_code: image.object_code,
                        x,
                        top: top + shift + k as f64 * CELL_PITCH + CELL_IMAGE_DROP,
                        width,
                        height: (f64::from(image.display_height) - DRAW_INSET_HEIGHT).max(0.0),
                    });
                    x += f64::from(image.display_width);
                    byte += 1;
                    continue;
                }
                byte += len;
                if ch < ' ' {
                    continue;
                }
                let width = if ch.is_ascii() {
                    CELL_SIZE / 2.0
                } else {
                    CELL_SIZE
                };
                if ch != ' ' {
                    glyphs.push(PlannedGlyph {
                        ch,
                        x,
                        size: CELL_SIZE,
                        style: 0,
                        drop: 0.0,
                    });
                }
                x += width;
            }
            if !glyphs.is_empty() {
                out.push(PlannedLine {
                    top: top + shift + k as f64 * CELL_PITCH,
                    glyphs,
                    paragraph: Some(document.lines[line].paragraph_id.clone()),
                    cell: Some(cell.id.clone()),
                    start: None,
                    stops: Vec::new(),
                    underlines: Vec::new(),
                });
            }
        }
    }
    (out, images)
}

/// 그림 본문 → RGBA. 무압축 화소(8비트 회색 · RGB)는 그대로, 비었으면 TIFF(1비트 · G4)를 푼다.
pub(crate) fn payload_rgba(payload: &crate::HltImagePayload) -> Option<image::RgbaImage> {
    if payload.pixels.is_empty() {
        return Some(
            image::load_from_memory_with_format(&payload.tiff, image::ImageFormat::Tiff)
                .ok()?
                .to_rgba8(),
        );
    }
    let (width, height) = (payload.width, payload.height);
    let image = match (payload.samples, payload.bits) {
        (1, 8) => image::DynamicImage::ImageLuma8(image::GrayImage::from_raw(
            width,
            height,
            payload.pixels.clone(),
        )?),
        (3, 8) => image::DynamicImage::ImageRgb8(image::RgbImage::from_raw(
            width,
            height,
            payload.pixels.clone(),
        )?),
        _ => return None,
    };
    Some(image.to_rgba8())
}
