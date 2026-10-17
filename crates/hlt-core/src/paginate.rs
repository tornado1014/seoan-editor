//! 쪽 매기기 — 통특기가 문서를 열 때 조판하는 쪽 수(꼬리 #14 개수)를 파일의 줄로 다시 센다.
//!
//! 이 1단계는 **저장 줄 = 인쇄 줄**로 본다(인쇄 재흘림 `reflow` 는 첨자·인라인 그림이 많은 문서에서만 다르다).

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    InspectDocument,
    inspect::CHAR_MARK,
    reflow::{self, Item},
};

/// 쪽 본문 용량. 1u = 0.11871pt(글자 줄 피치 33.24pt = 280u).
const PAGE_CAPACITY: i64 = 5662;
const UNITS_PER_PT: f64 = 1.0 / 0.11871;
const MM_PER_PT: f64 = 25.4 / 72.0;
/// 표 칸 글자 줄 하나 = 인쇄 행 피치 14.04pt.
const ROW_UNIT: f64 = 118.27;
/// 표 위 여백(첫 행에 얹음) · 아래 여백(마지막 행을 놓을 때만 셈).
const TABLE_PAD_TOP: f64 = 4.0;
const TABLE_PAD_BOTTOM: i64 = 21;
/// 그림이 든 칸의 행 = 그림 높이 + 11.
const ROW_IMAGE_PAD: f64 = 11.0;
/// 줄 높이 = 오름 + 내림 + 170. 글자 오름 80 · 내림 30.
pub(crate) const TEXT_ASCENT: f64 = 80.0;
const TEXT_DESCENT: f64 = 30.0;
const LINE_GAP: f64 = 170.0;
/// 그림 앞에 글자가 있는 줄의 내림(그림 뒤 글자는 안 센다).
const DESCENT_TEXT_BEFORE: f64 = 28.0;
/// 그려진 그림 높이 = 표시 pt(+364) − 이 값.
const DRAW_INSET_PT: f64 = 0.72;
const DEFAULT_LINE: i64 = 280;
const FORM_FEED: u8 = 0x0c;
const CELL_END: u8 = 0x13;
const ROW_END: u8 = 0x12;

/// 줄 높이 자리의 「이 줄 뒤에서 새 쪽」(폼피드 줄).
pub(crate) const BREAK: i64 = -1;

/// 진단용: 줄 높이 · 붙임 줄 · 줄별 쪽(1 기준).
#[doc(hidden)]
pub fn page_layout(
    document: &InspectDocument,
    inflated: &[u8],
) -> (Vec<i64>, Vec<usize>, Vec<usize>) {
    let (heights, keep, pad, _) = line_heights(document, inflated);
    let pages = paginate(&heights, &keep, &pad);
    (heights, keep.into_iter().collect(), pages)
}

/// 쪽 수. `inflated` = 해제본(줄 원문을 `LineRecord::source_range` 로 읽는다).
pub fn page_count(document: &InspectDocument, inflated: &[u8]) -> usize {
    let (heights, keep, pad, _) = line_heights(document, inflated);
    paginate(&heights, &keep, &pad).last().copied().unwrap_or(1)
}

fn index_of(id: &str) -> Option<usize> {
    id.rsplit(':').next()?.parse().ok()
}

fn image_units(display_height: u32, box_height: u32) -> f64 {
    if display_height > 0 && display_height < 4000 {
        // 그려진 높이를 mm 소수 둘째 자리로 반올림한 뒤 단위로 바꾼다(1u 차이가 쪽 경계를 가른다).
        let mm = ((f64::from(display_height) - DRAW_INSET_PT) * MM_PER_PT * 100.0).round() / 100.0;
        mm / MM_PER_PT * UNITS_PER_PT
    } else {
        // 표시 pt 가 없으면 상자 높이(96dpi 픽셀 관례).
        let mm = (f64::from(box_height) * 25.4 / 96.0 * 100.0).round() / 100.0;
        mm / MM_PER_PT * UNITS_PER_PT
    }
}

fn image_line_height(image: f64, images: usize, text_before: bool) -> i64 {
    let descent = if images >= 2 {
        TEXT_DESCENT
    } else if text_before {
        DESCENT_TEXT_BEFORE
    } else {
        0.0
    };
    (image.max(TEXT_ASCENT) + descent + LINE_GAP).round() as i64
}

type Heights = (
    Vec<i64>,
    BTreeSet<usize>,
    BTreeMap<usize, i64>,
    Vec<PrintedLine>,
);

/// 인쇄 줄 하나(그리기용 · `print.rs`). 높이는 `line`(저장 줄)에 실린 몫이다.
#[derive(Debug, Clone)]
pub(crate) struct PrintedLine {
    /// 높이가 실린 저장 줄.
    pub(crate) line: usize,
    /// (글자, 칸, 글자모양 비트). 그림은 `'\u{FFFC}'`.
    pub(crate) items: Vec<(char, Item, u32)>,
    /// 첫 줄 들여쓰기(px600) — 문단 첫 인쇄 줄만.
    pub(crate) indent: f64,
    /// 문단(칸 밖 묶음)의 마지막 인쇄 줄 — 양쪽 맞춤을 안 편다.
    pub(crate) last: bool,
    pub(crate) height: i64,
    /// 줄의 가장 큰 오름(u) — 글자 윗변 = 줄 위 + 이 값 − 글자 오름.
    pub(crate) ascent: f64,
    /// (칸 차례, `document.images` 차례, 그려진 높이 u).
    pub(crate) images: Vec<(usize, usize, f64)>,
    /// 첫 칸의 문단 글 안 자리(글자 수) — 문단 글(`ParagraphRecord::text`)과 칸이 하나씩 맞는다(둘 다 CR·LF 만 뺀 바이트).
    /// 표가 낀 문단의 둘째 묶음부터는 모른다(`None`).
    pub(crate) start: Option<usize>,
}

pub(crate) fn line_heights(document: &InspectDocument, inflated: &[u8]) -> Heights {
    let n = document.lines.len();
    let mut out = vec![0_i64; n];
    let raw = |line: usize| -> &[u8] {
        let range = &document.lines[line].source_range;
        inflated
            .get(range.start_byte as usize..range.end_byte as usize)
            .unwrap_or(&[])
    };
    let lspace: BTreeMap<&str, i64> = document
        .shapes
        .iter()
        .map(|shape| (shape.id.as_str(), i64::from(shape.lspace)))
        .collect();
    let indent: BTreeMap<&str, i64> = document
        .shapes
        .iter()
        .map(|shape| (shape.id.as_str(), i64::from(shape.indent_raw as i32)))
        .collect();
    let line_indent = |line: usize| {
        document.lines[line]
            .shape_id
            .as_deref()
            .and_then(|id| indent.get(id).copied())
            .unwrap_or(0)
    };
    let line_lspace = |line: usize| {
        document.lines[line]
            .shape_id
            .as_deref()
            .and_then(|id| lspace.get(id).copied())
            .unwrap_or(DEFAULT_LINE)
    };

    // 줄마다 (원문 안 자리, 그려진 높이u, 폭 px600, `document.images` 차례).
    let mut images: BTreeMap<usize, Vec<(usize, f64, f64, usize)>> = BTreeMap::new();
    // 줄마다 마지막 그림의 dpi 태그 유무(그림 본문을 실어 읽었을 때만 — 없으면 「없음」으로 본다).
    let mut line_dpi: BTreeMap<usize, bool> = BTreeMap::new();
    for (image_index, image) in document.images.iter().enumerate() {
        let height = image_units(image.display_height, image.box_height);
        if let Some(line) = index_of(&image.line_id).filter(|line| *line < n) {
            let at = (image.source_range.start_byte - document.lines[line].source_range.start_byte)
                as usize;
            let width = f64::from(image.display_width) * reflow::PT;
            images
                .entry(line)
                .or_default()
                .push((at, height, width, image_index));
            let dpi = document
                .image_payloads
                .get(&image.object_code)
                .is_some_and(|payload| tiff_has_dpi(&payload.tiff));
            line_dpi.insert(line, dpi);
        }
    }

    // 표: 줄 배열의 칸 번호와 표지(0x13 칸 끝 · 0x12 행 끝)로 행을 읽는다. 세로 병합에 덮인 칸도 제 행의 칸이다
    // (판독 모델 `CellRecord` 는 덮인 칸을 위 칸에 합친다 — 쪽 매기기에는 안 맞는다).
    // 행 높이 = 칸마다 max(줄 수 × 118.27, 그림 + 11) 의 최대 · 첫 행에 위 여백 · 행의 첫 줄에 싣고 나머지는 0.
    let codes = char_codes(inflated, n);
    let in_table = |line: usize| codes.line_cell.get(line).is_some_and(|cell| *cell != 0);
    let line_image = |line: usize| {
        images
            .get(&line)
            .map(|marks| marks.iter().map(|(_, h, _, _)| *h).fold(0.0, f64::max))
            .unwrap_or(0.0)
    };
    let mut pad = BTreeMap::new();
    let mut row_first: Vec<(usize, usize, usize)> = Vec::new(); // (표, 행, 첫 줄)
    let mut tables = 0_usize;
    for paragraph in &document.paragraphs {
        let mut rows: Vec<(Vec<usize>, f64)> = Vec::new();
        let (mut row_lines, mut row_height, mut pending) = (Vec::new(), 0.0_f64, Vec::new());
        let lines = paragraph
            .line_ids
            .iter()
            .filter_map(|id| index_of(id))
            .filter(|l| *l < n);
        for line in lines.map(Some).chain([None]) {
            if let Some(line) = line.filter(|line| in_table(*line)) {
                let text = raw(line);
                row_lines.push(line);
                if text == [ROW_END] {
                    rows.push((std::mem::take(&mut row_lines), row_height));
                    row_height = 0.0;
                } else {
                    pending.push(line);
                    if text.ends_with(&[CELL_END]) {
                        let image = pending.iter().map(|l| line_image(*l)).fold(0.0, f64::max);
                        let height = (ROW_UNIT * pending.len() as f64).max(image + ROW_IMAGE_PAD);
                        row_height = row_height.max(height);
                        pending.clear();
                    }
                }
                continue;
            }
            // 행에 못 든 칸 줄(끝나지 않은 칸)은 글자 줄 하나 높이.
            for line in row_lines.drain(..).chain(pending.drain(..)) {
                out[line] = ROW_UNIT.round() as i64;
            }
            row_height = 0.0;
            if rows.is_empty() {
                continue;
            }
            let last = rows.len() - 1;
            for (index, (lines, height)) in rows.drain(..).enumerate() {
                let height = if index == 0 {
                    height + TABLE_PAD_TOP
                } else {
                    height
                };
                out[lines[0]] = height.round() as i64;
                if index == last {
                    pad.insert(lines[0], TABLE_PAD_BOTTOM);
                }
                row_first.push((tables, index, lines[0]));
            }
            tables += 1;
        }
    }

    // 문단의 연속된 칸 밖 줄 묶음. 줄마다 (첫 줄, 줄 수, 글자 있음, 그림만, 그림 수).
    let figure_caption = |line: usize| {
        let text = raw(line);
        let start = text.iter().position(|b| *b != b' ').unwrap_or(text.len());
        text[start..].starts_with(&[0xa1, 0xbc, 0xb5, 0xb5]) // 【도
    };
    let mut paragraphs: Vec<(usize, usize, bool, bool, usize)> = Vec::new();
    let mut printed = Vec::new();
    let context = Context {
        raw: &raw,
        images: &images,
        line_dpi: &line_dpi,
        lspace: &line_lspace,
        indent: &line_indent,
        sub: &|line: usize, at: usize| codes.sub(line, at),
        attr: &|line: usize, at: usize| codes.attr(line, at),
        caption: &figure_caption,
    };
    for paragraph in &document.paragraphs {
        let lines: Vec<usize> = paragraph
            .line_ids
            .iter()
            .filter_map(|id| index_of(id))
            .filter(|line| *line < n)
            .collect();
        let mut group: Vec<usize> = Vec::new();
        for line in lines.iter().copied().map(Some).chain([None]) {
            if let Some(line) = line.filter(|line| !in_table(*line)) {
                group.push(line);
                continue;
            }
            if let Some(&first) = group.first() {
                reflow_group(
                    &group,
                    first == lines[0],
                    &mut Group {
                        out: &mut out,
                        paragraphs: &mut paragraphs,
                        printed: &mut printed,
                    },
                    &context,
                );
            }
            group.clear();
        }
    }

    let mut keep = BTreeSet::new();
    // 한 줄 글자 문단 바로 뒤 그림 1장뿐인 문단은 같은 쪽(【도 N】 표제).
    for pair in paragraphs.windows(2) {
        let ((l1, n1, text1, _, _), (l2, n2, _, only2, images2)) = (pair[0], pair[1]);
        if n1 == 1 && text1 && n2 == 1 && only2 && images2 == 1 && l2 == l1 + 1 {
            keep.insert(l1);
        }
    }
    // 표 행은 다음 행과 같은 쪽(마지막 행 제외).
    for pair in row_first.windows(2) {
        if pair[0].0 == pair[1].0 && pair[0].1 + 1 == pair[1].1 {
            keep.insert(pair[0].2);
        }
    }
    for line in 0..n {
        if raw(line).contains(&FORM_FEED) {
            out[line] = BREAK;
        }
    }
    (out, keep, pad, printed)
}

struct Context<'a> {
    raw: &'a dyn Fn(usize) -> &'a [u8],
    images: &'a BTreeMap<usize, Vec<(usize, f64, f64, usize)>>,
    line_dpi: &'a BTreeMap<usize, bool>,
    lspace: &'a dyn Fn(usize) -> i64,
    indent: &'a dyn Fn(usize) -> i64,
    sub: &'a dyn Fn(usize, usize) -> bool,
    /// 글자모양 비트(밑줄 1 · 굵게 2 · 기울임 4 · 위첨자 16 · 아래첨자 32).
    attr: &'a dyn Fn(usize, usize) -> u32,
    caption: &'a dyn Fn(usize) -> bool,
}

struct Group<'a> {
    out: &'a mut Vec<i64>,
    paragraphs: &'a mut Vec<(usize, usize, bool, bool, usize)>,
    printed: &'a mut Vec<PrintedLine>,
}

/// 칸 밖 저장 줄 묶음 하나 → 인쇄 줄(`reflow`) → 줄 높이. 인쇄 줄의 높이는 시작이 가장 가까운 저장 줄에 싣는다
/// (합은 정확 · 저장 줄 하나가 두 인쇄 줄로 갈라지면 둘째 줄의 쪽 경계는 그 저장 줄 안에 숨는다).
fn reflow_group(lines: &[usize], paragraph_start: bool, group: &mut Group<'_>, cx: &Context<'_>) {
    let lsp = (cx.lspace)(lines[0]);
    let indent = if paragraph_start {
        (cx.indent)(lines[0]) as f64 * reflow::TWIP
    } else {
        0.0
    };
    let mut items: Vec<Item> = Vec::new();
    let mut glyphs: Vec<char> = Vec::new();
    let mut attrs: Vec<u32> = Vec::new();
    let mut image_of: BTreeMap<usize, usize> = BTreeMap::new();
    let mut bounds = Vec::with_capacity(lines.len());
    let mut heights: BTreeMap<usize, f64> = BTreeMap::new();
    for &line in lines {
        let text = (cx.raw)(line);
        let marks = cx.images.get(&line).map(Vec::as_slice).unwrap_or(&[]);
        let mut at = 0;
        while at < text.len() {
            let sub = (cx.sub)(line, at);
            let attr = (cx.attr)(line, at);
            if let Some((_, height, width, image)) =
                marks.iter().find(|(mark, _, _, _)| *mark == at)
            {
                heights.insert(items.len(), *height);
                image_of.insert(items.len(), *image);
                items.push(Item::Image(*width));
                glyphs.push('\u{FFFC}');
                attrs.push(attr);
                at += 1;
            } else if text[at] >= 0x81 && at + 1 < text.len() {
                items.push(Item::Wide(sub));
                let (decoded, _, _) = encoding_rs::EUC_KR.decode(&text[at..at + 2]);
                glyphs.push(decoded.chars().next().unwrap_or('\u{FFFD}'));
                attrs.push(attr);
                at += 2;
            } else {
                if !matches!(text[at], b'\r' | b'\n') {
                    items.push(Item::Ascii(text[at], sub));
                    glyphs.push(char::from(text[at]));
                    attrs.push(attr);
                }
                at += 1;
            }
        }
        bounds.push(items.len());
    }
    let printed = reflow::reflow(&items, &bounds, indent);
    if printed.is_empty() {
        for &line in lines {
            group.out[line] += lsp;
            group.printed.push(PrintedLine {
                line,
                items: Vec::new(),
                indent: 0.0,
                last: true,
                height: lsp,
                ascent: TEXT_ASCENT,
                images: Vec::new(),
                start: (paragraph_start && line == lines[0]).then_some(0),
            });
        }
        return;
    }
    let starts: Vec<usize> = std::iter::once(0)
        .chain(bounds[..bounds.len() - 1].iter().copied())
        .collect();
    let mut first = None;
    // 들여쓰기는 글자가 있는 첫 인쇄 줄에 — 쪽나눔(폼피드)만 있는 줄이 먼저 오는 표제(【도면】 · 【도 N】).
    let indented = printed
        .iter()
        .position(|&(a, b)| items[a..b].iter().any(|item| item.has_ink()))
        .unwrap_or(0);
    for (index, &(a, b)) in printed.iter().enumerate() {
        let (mut has_text, mut count, mut text_before, mut tallest) = (false, 0, false, 0.0_f64);
        for (q, item) in items.iter().enumerate().take(b).skip(a) {
            if let Item::Image(_) = item {
                if count == 0 {
                    text_before = has_text;
                }
                count += 1;
                tallest = tallest.max(heights.get(&q).copied().unwrap_or(0.0));
            } else if item.has_ink() {
                has_text = true;
            }
        }
        let j = (0..starts.len())
            .min_by_key(|j| (starts[*j].abs_diff(a), *j))
            .unwrap();
        let line = lines[j];
        let height = if count > 0 {
            let mut image = image_line_height(tallest, count, text_before);
            // 표제와 합쳐 쪽을 넘는 도면: dpi 태그가 없으면 줄여 그려 한 쪽에 넣고(pct 도면 7장),
            // 있으면 줄이지 않고 다음 쪽으로 흘린다(domestic 【도 12】).
            if line > 0
                && (cx.caption)(line - 1)
                && image + 280 > PAGE_CAPACITY
                && !cx.line_dpi.get(&line).copied().unwrap_or(false)
            {
                image = PAGE_CAPACITY - 280;
            }
            if has_text { image.max(lsp) } else { image }
        } else {
            lsp
        };
        group.out[line] += height;
        group.printed.push(PrintedLine {
            line,
            items: (a..b).map(|q| (glyphs[q], items[q], attrs[q])).collect(),
            indent: if index == indented { indent } else { 0.0 },
            last: index + 1 == printed.len(),
            height,
            ascent: if count > 0 {
                tallest.max(TEXT_ASCENT)
            } else {
                TEXT_ASCENT
            },
            images: (a..b)
                .filter_map(|q| Some((q - a, *image_of.get(&q)?, heights.get(&q).copied()?)))
                .collect(),
            start: paragraph_start.then_some(a),
        });
        if first.is_none() {
            first = Some((line, has_text, count));
        }
    }
    let (line, has_text, count) = first.unwrap();
    group
        .paragraphs
        .push((line, printed.len(), has_text, !has_text, count));
}

/// 줄마다 글자 → 글자모양 번호(글자 표) · 번호 → 첨자인가(객체 표 글자모양 +0 비트 16·32).
struct CharCodes {
    lines: Vec<Vec<u32>>,
    /// 줄 배열(객체 표 뒤 `bf` + 줄마다 모양·칸·0)의 칸 번호. 0 = 칸 밖.
    line_cell: Vec<u32>,
    sub: BTreeMap<u32, bool>,
    attr: BTreeMap<u32, u32>,
}

impl CharCodes {
    fn attr(&self, line: usize, at: usize) -> u32 {
        let Some(codes) = self.lines.get(line) else {
            return 0;
        };
        let code = if codes.len() == 1 {
            codes[0]
        } else {
            codes.get(at).copied().unwrap_or(0)
        };
        self.attr.get(&code).copied().unwrap_or(0)
    }

    fn sub(&self, line: usize, at: usize) -> bool {
        let Some(codes) = self.lines.get(line) else {
            return false;
        };
        let code = if codes.len() == 1 {
            codes[0]
        } else {
            codes.get(at).copied().unwrap_or(0)
        };
        self.sub.get(&code).copied().unwrap_or(false)
    }
}

fn char_codes(inflated: &[u8], total_lines: usize) -> CharCodes {
    let read = |at: usize| {
        inflated
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let mut codes = CharCodes {
        lines: Vec::new(),
        line_cell: Vec::new(),
        sub: BTreeMap::new(),
        attr: BTreeMap::new(),
    };
    let Some(mark) = inflated
        .windows(CHAR_MARK.len())
        .position(|w| w == CHAR_MARK)
    else {
        return codes;
    };
    let mut at = mark + CHAR_MARK.len();
    for _ in 0..total_lines {
        let Some(count) = read(at).map(|v| v as usize) else {
            return codes;
        };
        codes
            .lines
            .push((0..count).filter_map(|i| read(at + 4 + 4 * i)).collect());
        at += 4 + 4 * count;
    }
    if inflated.get(at) != Some(&0xbf) {
        return codes;
    }
    let count = read(at + 1).unwrap_or(0);
    let mut record = at + 5;
    for code in 0..count {
        let (Some(flags), Some(kind)) = (read(record), read(record + 40)) else {
            break;
        };
        if matches!(kind, 3 | 4) {
            record += 384 + read(record + 380).unwrap_or(0) as usize;
        } else {
            codes.sub.insert(code, flags & (16 | 32) != 0);
            codes.attr.insert(code, flags);
            record += 100;
        }
    }
    if inflated.get(record) == Some(&0xbf) {
        codes.line_cell = (0..total_lines)
            .map(|line| read(record + 1 + 12 * line + 4).unwrap_or(0))
            .collect();
    }
    codes
}

/// TIFF 에 해상도가 적혔는가 — XResolution(282)이 0 이 아니고 단위(296)가 「없음(1)」이 아니다
/// (Pillow 의 `info["dpi"]` 판정과 같다 · 단위 태그가 없으면 인치로 본다).
fn tiff_has_dpi(tiff: &[u8]) -> bool {
    let little = match tiff.get(..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return false,
    };
    let u16_at = |at: usize| {
        tiff.get(at..at + 2).map(|b| {
            if little {
                u16::from_le_bytes([b[0], b[1]])
            } else {
                u16::from_be_bytes([b[0], b[1]])
            }
        })
    };
    let u32_at = |at: usize| {
        tiff.get(at..at + 4).map(|b| {
            let b = [b[0], b[1], b[2], b[3]];
            if little {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            }
        })
    };
    let Some(ifd) = u32_at(4).map(|v| v as usize) else {
        return false;
    };
    let Some(count) = u16_at(ifd) else {
        return false;
    };
    let (mut resolution, mut unit) = (None, None);
    for entry in 0..usize::from(count) {
        let at = ifd + 2 + 12 * entry;
        match u16_at(at) {
            Some(282) => resolution = u32_at(at + 8).and_then(|offset| u32_at(offset as usize)),
            Some(296) => unit = u16_at(at + 8),
            _ => {}
        }
    }
    resolution.is_some_and(|value| value != 0) && unit != Some(1)
}

/// 줄 → 쪽(1 기준). 쌓다 넘치면 다음 쪽. `keep` 줄은 다음 「높이 있는」 줄과 합쳐 판단하되,
/// 합쳐서 빈 쪽에도 안 들어가면 데려가지 않는다.
pub(crate) fn paginate(
    heights: &[i64],
    keep: &BTreeSet<usize>,
    pad: &BTreeMap<usize, i64>,
) -> Vec<usize> {
    let mut pages = Vec::with_capacity(heights.len());
    let (mut page, mut used) = (1_usize, 0_i64);
    for (line, &height) in heights.iter().enumerate() {
        if height == BREAK {
            page += 1;
            used = 0;
            pages.push(page);
            continue;
        }
        let height = height + pad.get(&line).copied().unwrap_or(0);
        let mut need = height;
        if keep.contains(&line) {
            let next = heights[line + 1..].iter().find(|h| **h != 0).copied();
            if let Some(next) = next.filter(|h| *h != BREAK && height + h <= PAGE_CAPACITY) {
                need = height + next;
            }
        }
        if used > 0 && used + need > PAGE_CAPACITY {
            page += 1 + (0.max(used - 1) / PAGE_CAPACITY) as usize;
            used = 0;
        }
        pages.push(page);
        used += height;
    }
    pages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stacks_lines_and_breaks_on_overflow_and_form_feed() {
        let none = (BTreeSet::new(), BTreeMap::new());
        // 20 줄 × 280 = 5,600 ≤ 5,662 → 한 쪽, 21 번째 줄이 넘친다.
        assert_eq!(*paginate(&[280; 20], &none.0, &none.1).last().unwrap(), 1);
        assert_eq!(*paginate(&[280; 21], &none.0, &none.1).last().unwrap(), 2);
        assert_eq!(
            paginate(&[280, BREAK, 280], &none.0, &none.1),
            vec![1, 2, 2]
        );
        // keep: 19 줄 뒤 표제(280)+그림(400)은 함께 다음 쪽으로.
        let mut heights = vec![280; 19];
        heights.extend([280, 400]);
        let keep = BTreeSet::from([19]);
        assert_eq!(&paginate(&heights, &keep, &none.1)[19..], &[2, 2]);
        assert_eq!(&paginate(&heights, &none.0, &none.1)[19..], &[1, 2]);
    }
}
