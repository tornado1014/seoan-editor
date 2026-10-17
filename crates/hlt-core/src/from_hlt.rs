//! HLT → 의미 모델(`HmlDocument`) 역변환.
//!
//! 표 구조 편집이 `.docx/.hml/.xml` 입력만 받던 것을 풀기 위한 경로다. 이미 받은 HLT 를
//! 의미 모델로 되돌리면 같은 편집기를 그대로 쓸 수 있다.
//!
//! ⛔ **바이트 동일은 목표가 아니다.** 통특기 저장본조차 원본의 최소 변경이 아니다(저장할 때
//! 문서를 정규화한다). 받아들일 조건은 다시 구운 파일을 판독한 값이 원본 판독값과 맞는 것이다.
//!
//! 확인한 것:
//! - 글자 크기는 `FormatParagraph.sizes` 가 빈틈없이 덮는다. 서식 스팬만 쓰면 표 셀 10pt 를 잃는다.
//! - 셀의 정렬, 들여쓰기, 줄간격은 그 셀 **첫 줄의 문단모양**에서 온다. 문단 첫 줄만으로는 모자란다.
//! - 세로 병합에 덮인 칸은 판독기가 주 셀의 rowspan 으로 접으므로, 생성기가 기대하는
//!   **이어짐 셀을 여기서 되살려야** 한다.

use std::collections::BTreeMap;
use std::path::Path;

use crate::{
    CellFormatRecord, CellRecord, CharacterFormat, CoreError, FormatParagraph,
    GeneratedImagePayload, HmlCell, HmlDocument, HmlImage, HmlInline, HmlParagraph, HmlSection,
    HmlSummary, HmlTable, ImageOccurrence, InspectDocument, InspectOptions, ShapeRecord,
    inspect_format_inventories, inspect_path,
};

const ROW_END: char = '\u{12}';
const CELL_END: char = '\u{13}';
const PAGE_BREAK: char = '\u{c}';
const DEFAULT_CELL_LSPACE: u32 = 280;

/// HLT 하나를 의미 모델과 그림 페이로드로 되돌린다.
/// 페이로드는 `save_generated_hlt_as_new` 에 그대로 넘길 수 있다.
pub fn hml_document_from_hlt(
    path: &Path,
) -> Result<(HmlDocument, Vec<GeneratedImagePayload>), CoreError> {
    let document = inspect_path(
        path,
        InspectOptions {
            include_image_payloads: true,
            // ⛔ 그림 지문은 켜지 않는다. `hash_tiff_identity` 가 기하를 못 읽으면 통째로
            // 죽고, 국내출원의 1비트 G4 압축 TIFF 가 그 경우다. 역변환에는 필요 없다.
            include_image_identities: false,
            ..InspectOptions::default()
        },
    )?;
    let (formats, cell_formats) = inspect_format_inventories(path)?;

    let shapes: BTreeMap<&str, &ShapeRecord> = document
        .shapes
        .iter()
        .map(|shape| (shape.id.as_str(), shape))
        .collect();
    let line_shape: BTreeMap<&str, &ShapeRecord> = document
        .lines
        .iter()
        .filter_map(|line| {
            line.shape_id
                .as_deref()
                .and_then(|id| shapes.get(id))
                .map(|shape| (line.id.as_str(), *shape))
        })
        .collect();
    let paragraph_formats: BTreeMap<&str, &FormatParagraph> = formats
        .paragraphs
        .iter()
        .map(|value| (value.paragraph_id.as_str(), value))
        .collect();
    let cell_records: BTreeMap<&str, &CellFormatRecord> = cell_formats
        .cells
        .iter()
        .map(|value| (value.cell_id.as_str(), value))
        .collect();

    // 표는 본문에 행 종결자를 담은 문단이 순서대로 물고 있다. 판독기의 표 앵커는
    // 【표 N】 표제 문단이라 본문 문단과 다르므로 여기서 문서 순서로 짝짓는다.
    let table_paragraphs: Vec<&str> = document
        .paragraphs
        .iter()
        .filter(|paragraph| paragraph.text.contains(ROW_END))
        .map(|paragraph| paragraph.id.as_str())
        .collect();
    let mut paragraph_table: BTreeMap<&str, &str> = BTreeMap::new();
    for (table, paragraph) in document.tables.iter().zip(&table_paragraphs) {
        paragraph_table.insert(paragraph, table.id.as_str());
    }

    let (images, payloads) = rebuild_images(&document);
    let images_by_paragraph =
        anchor_map(&document.images, |image| Some(image.paragraph_id.as_str()));
    let images_by_cell = anchor_map(&document.images, |image| image.cell_id.as_deref());

    // 원본 줄나눔. 줄마다 본문 바이트 범위를 들고 있으므로 길이만 뽑아 문단별로 모은다.
    let mut source_lines: BTreeMap<&str, Vec<u32>> = BTreeMap::new();
    for line in &document.lines {
        let bytes = line
            .source_range
            .end_byte
            .saturating_sub(line.source_range.start_byte);
        source_lines
            .entry(line.paragraph_id.as_str())
            .or_default()
            .push(u32::try_from(bytes).unwrap_or(u32::MAX));
    }

    let mut paragraphs = Vec::with_capacity(document.paragraphs.len());
    for record in &document.paragraphs {
        let table_id = paragraph_table
            .get(record.id.as_str())
            .map(|value| (*value).to_string());
        let raw = record.text.as_str();
        // 표 문단의 의미 글자는 표 뒤 꼬리뿐이다. 칸 글자는 셀이 따로 들고 있다.
        let body = if table_id.is_some() {
            raw.rsplit(ROW_END).next().unwrap_or_default()
        } else {
            raw
        };
        // ⛔ 여기서 `trim()` 하지 마라. 앞머리 탭을 먹는다(실물 PCT 문단 하나가 그렇다).
        // 판독기가 이미 앞뒤 ASCII 공백을 잘라 뒀으므로 페이지브레이크만 떼면 된다.
        let mut text = body.trim_matches(PAGE_BREAK).to_string();
        let lead = body
            .chars()
            .position(|value| value != PAGE_BREAK)
            .unwrap_or(0);
        let head = raw.chars().count() - body.chars().count() + lead;
        // 서식 스팬 좌표는 칸, 행 종결자를 뺀 글자열 기준이다.
        let terminators = raw
            .chars()
            .take(head)
            .filter(|value| *value == CELL_END || *value == ROW_END)
            .count();
        let anchored = images_by_paragraph
            .get(record.id.as_str())
            .map(Vec::as_slice)
            .unwrap_or_default();
        // 판독기는 문단 글자의 꼬리 ASCII 공백을 잘라내지만 서식 목록의 글자는 안 자른다.
        // 밑줄이 그 공백까지 걸친 실물이 있으므로 꼬리 공백만 그쪽에서 되찾는다. 탭이나
        // 페이지브레이크가 섞인 문단은 두 글자열이 그것 말고도 달라서 이 규칙이 안 걸린다.
        if table_id.is_none()
            && let Some(full) = paragraph_formats.get(record.id.as_str())
            && full.text.trim_end_matches(' ') == text
        {
            text = full.text.clone();
        }
        let table_id_present = table_id.is_some();
        let inlines = match paragraph_formats.get(record.id.as_str()) {
            Some(formats) => inline_runs(&text, formats, head - terminators, anchored),
            None => plain(&text),
        };
        // 생성기는 `text` 가 런의 글자와 정확히 같기를 요구한다. 그림 자리표는 런이 되므로
        // 글자에서 빠져야 한다. 그래서 원문이 아니라 런에서 도출한다.
        let text = visible_text(&inlines);
        paragraphs.push(HmlParagraph {
            id: record.id.clone(),
            section_id: record.section_id.clone(),
            marker: marker_of(&text),
            tail_text: if table_id.is_some() {
                text.clone()
            } else {
                String::new()
            },
            text,
            align: record.align.clone(),
            indent_raw: i32::try_from(record.indent_raw).unwrap_or(0),
            lmargin_raw: 0,
            rmargin_raw: 0,
            lspace: record.lspace,
            page_break: raw.contains(PAGE_BREAK),
            table_id,
            image_ids: anchored.iter().map(|id| (*id).to_string()).collect(),
            inlines,
            // 표 문단은 `emit_table_body` 가 칸 단위로 흘리므로 줄나눔을 옮기지 않는다.
            // 페이지브레이크 줄은 생성기가 따로 내므로 앞에서 그만큼 떼어 낸다.
            source_line_bytes: if table_id_present {
                Vec::new()
            } else {
                source_lines
                    .get(record.id.as_str())
                    .map(|value| value[lead.min(value.len())..].to_vec())
                    .unwrap_or_default()
            },
            number: None,
        });
    }

    // ⛔ `parse_body` 는 본문이 `0x15` 줄바꿈으로 끝나면 **언제나** 빈 문단 하나를 뒤에 만든다.
    // 문서의 내용이 아니라 파싱 부산물이다. 그것까지 의미 모델에 담아 다시 구우면 종결자가
    // 하나 늘어나고 왕복마다 빈 문단이 쌓인다(예: 562 → 563).
    if paragraphs.last().is_some_and(|last| {
        last.text.is_empty()
            && last.table_id.is_none()
            && last.marker.is_none()
            && last.image_ids.is_empty()
    }) {
        paragraphs.pop();
    }

    let mut paragraphs_by_section: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for paragraph in &paragraphs {
        if let Some(section) = paragraph.section_id.as_deref() {
            paragraphs_by_section
                .entry(section)
                .or_default()
                .push(paragraph.id.clone());
        }
    }
    let sections: Vec<HmlSection> = document
        .sections
        .iter()
        .map(|section| HmlSection {
            id: section.id.clone(),
            index: section.index,
            title: section.title.clone(),
            kind: crate::hml::section_kind(&section.title).to_string(),
            level: crate::hml::section_level(&section.title, &[]),
            paragraph_ids: paragraphs_by_section
                .get(section.id.as_str())
                .cloned()
                .unwrap_or_default(),
        })
        .collect();

    let (tables, cells) = rebuild_tables(
        &document,
        &table_paragraphs,
        &line_shape,
        &cell_records,
        &images_by_cell,
    );

    let summary = HmlSummary {
        section_count: sections.len() as u64,
        top_level_paragraph_count: paragraphs.len() as u64,
        content_paragraph_count: paragraphs
            .iter()
            .filter(|paragraph| paragraph.marker.is_none())
            .count() as u64,
        table_count: tables.len() as u64,
        row_count: tables.iter().map(|table| u64::from(table.row_count)).sum(),
        cell_count: cells.len() as u64,
        image_count: images.len() as u64,
    };

    Ok((
        HmlDocument {
            source: path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("input.HLT")
                .to_string(),
            source_sha256: formats.source_sha256.clone(),
            encoding: "cp949".to_string(),
            title: document.summary.document_title.clone(),
            // 생성기는 글꼴 목록을 쓰지 않는다(`generate_font_table` 은 셀 레코드로 표를 만든다).
            fonts: Vec::new(),
            summary,
            sections,
            paragraphs,
            tables,
            cells,
            images,
        },
        payloads,
    ))
}

/// 그림 자리와 TIFF 원본을 옮긴다. 페이로드는 객체 코드마다 한 번만 싣는다.
fn rebuild_images(document: &InspectDocument) -> (Vec<HmlImage>, Vec<GeneratedImagePayload>) {
    let mut images = Vec::with_capacity(document.images.len());
    let mut payloads = Vec::new();
    let mut seen: BTreeMap<u32, ()> = BTreeMap::new();
    for image in &document.images {
        images.push(HmlImage {
            id: image.id.clone(),
            occurrence: u32::try_from(image.occurrence).unwrap_or(u32::MAX),
            name: format!("{}.tif", image.id.replace(':', "_")),
            // 상자(+48/+52)와 표시 pt(+368/+364)를 각각 제자리로. 옛 판은 pt 를 상자에
            // 넣고 pt 는 (px+2)/5 로 다시 계산해 PCT 왕복본 69장이 정본의 28% 가 됐다
            // (결함 D). 상자가 0 인 옛 JSON 은 pt 로 물러선다.
            width: Some(if image.box_width > 0 {
                image.box_width
            } else {
                image.display_width
            }),
            height: Some(if image.box_height > 0 {
                image.box_height
            } else {
                image.display_height
            }),
            secondary_width: Some(image.display_width).filter(|value| *value > 0),
            secondary_height: Some(image.display_height).filter(|value| *value > 0),
            paragraph_id: Some(image.paragraph_id.clone()),
            cell_id: image.cell_id.clone(),
        });
        if seen.insert(image.object_code, ()).is_none()
            && let Some(payload) = document.image_payloads.get(&image.object_code)
        {
            payloads.push(GeneratedImagePayload {
                image_id: image.id.clone(),
                bytes: payload.tiff.clone(),
            });
        }
    }
    (images, payloads)
}

/// 판독한 표와 셀을 의미 모델로 옮기고, 세로 병합에 덮인 **이어짐 셀을 되살린다.**
fn rebuild_tables(
    document: &InspectDocument,
    table_paragraphs: &[&str],
    line_shape: &BTreeMap<&str, &ShapeRecord>,
    cell_records: &BTreeMap<&str, &CellFormatRecord>,
    images_by_cell: &BTreeMap<&str, Vec<&str>>,
) -> (Vec<HmlTable>, Vec<HmlCell>) {
    let mut tables = Vec::with_capacity(document.tables.len());
    let mut cells = Vec::new();
    for (index, table) in document.tables.iter().enumerate() {
        let table_cells: Vec<&CellRecord> = document
            .cells
            .iter()
            .filter(|cell| cell.table_id == table.id)
            .collect();

        // 격자 자리를 채워 가며 각 셀의 열 번호를 정하고, 세로 병합이 덮은 자리를 적어 둔다.
        let mut filled: BTreeMap<(u32, u32), ()> = BTreeMap::new();
        let mut column_of: BTreeMap<&str, u32> = BTreeMap::new();
        let mut covered: Vec<(u32, u32, &CellRecord)> = Vec::new();
        for cell in &table_cells {
            let mut column = 0_u32;
            while filled.contains_key(&(cell.row, column)) {
                column += 1;
            }
            column_of.insert(cell.id.as_str(), column);
            for row in cell.row..cell.row + cell.rowspan.max(1) {
                for offset in 0..cell.colspan.max(1) {
                    filled.insert((row, column + offset), ());
                }
                if row != cell.row {
                    covered.push((row, column, cell));
                }
            }
        }

        let mut rows: BTreeMap<u32, Vec<HmlCell>> = BTreeMap::new();
        for cell in &table_cells {
            let column = column_of[cell.id.as_str()];
            let shape = cell
                .line_ids
                .first()
                .and_then(|line| line_shape.get(line.as_str()))
                .copied();
            let record = cell_records.get(cell.id.as_str());
            let text = record.map(|value| value.text.clone()).unwrap_or_default();
            let inlines = match record {
                Some(value) => cell_inline_runs(value),
                None => plain(&text),
            };
            let cell_images = images_by_cell
                .get(cell.id.as_str())
                .map(Vec::as_slice)
                .unwrap_or_default();
            let inlines = split_image_marks(inlines, cell_images);
            let text = visible_text(&inlines);
            rows.entry(cell.row).or_default().push(HmlCell {
                id: cell.id.clone(),
                table_id: table.id.clone(),
                row: cell.row + 1,
                occurrence: 0,
                text,
                start_raw: Some(cell.start),
                width_raw: Some(cell.width),
                hwidth_raw: None,
                column_begin: Some(column + 1),
                row_begin: Some(cell.row + 1),
                rowspan: cell.rowspan.max(1),
                colspan: cell.colspan.max(1),
                merge_continuation: false,
                align: shape
                    .map(|value| value.align.clone())
                    .unwrap_or_else(|| "left".to_string()),
                indent_raw: shape
                    .map(|value| i32::try_from(value.indent_raw).unwrap_or(0))
                    .unwrap_or(0),
                lspace: shape
                    .map(|value| value.lspace)
                    .unwrap_or(DEFAULT_CELL_LSPACE),
                image_ids: images_by_cell
                    .get(cell.id.as_str())
                    .map(|ids| ids.iter().map(|id| (*id).to_string()).collect())
                    .unwrap_or_default(),
                inlines,
            });
        }
        // 덮인 자리마다 이어짐 셀 하나. 주 셀의 자리와 폭을 그대로 물려받는다.
        for (row, column, master) in covered {
            rows.entry(row).or_default().push(HmlCell {
                id: format!("{}:cont:{row}", master.id),
                table_id: table.id.clone(),
                row: row + 1,
                occurrence: 0,
                text: String::new(),
                start_raw: Some(master.start),
                width_raw: Some(master.width),
                hwidth_raw: None,
                column_begin: Some(column + 1),
                row_begin: Some(master.row + 1),
                rowspan: 1,
                colspan: master.colspan.max(1),
                merge_continuation: true,
                align: "left".to_string(),
                indent_raw: 0,
                lspace: DEFAULT_CELL_LSPACE,
                image_ids: Vec::new(),
                inlines: Vec::new(),
            });
        }

        let mut ids = Vec::new();
        let mut flat = Vec::new();
        for row in rows.values_mut() {
            row.sort_by_key(|cell| cell.column_begin.unwrap_or(0));
            for (position, cell) in row.iter_mut().enumerate() {
                cell.occurrence = position as u32 + 1;
            }
            for cell in row.iter() {
                ids.push(cell.id.clone());
                flat.push(cell.clone());
            }
        }
        cells.extend(flat);
        tables.push(HmlTable {
            id: table.id.clone(),
            occurrence: u32::try_from(index + 1).unwrap_or(1),
            paragraph_id: table_paragraphs
                .get(index)
                .map(|value| (*value).to_string())
                .unwrap_or_default(),
            max_columns: table.column_count,
            row_count: table.row_count,
            cell_ids: ids,
        });
    }
    (tables, cells)
}

fn anchor_map<'a, F>(images: &'a [ImageOccurrence], key: F) -> BTreeMap<&'a str, Vec<&'a str>>
where
    F: Fn(&'a ImageOccurrence) -> Option<&'a str>,
{
    let mut map: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for image in images {
        if let Some(anchor) = key(image) {
            map.entry(anchor).or_default().push(image.id.as_str());
        }
    }
    map
}

/// 런이 실제로 내는 글자. 그림 런은 자리표를 남기지 않는다.
fn visible_text(inlines: &[HmlInline]) -> String {
    inlines
        .iter()
        .filter_map(|inline| match inline {
            HmlInline::Text { text, .. } => Some(text.as_str()),
            HmlInline::Image { .. } => None,
        })
        .collect()
}

fn marker_of(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (trimmed.starts_with('【') && trimmed.ends_with('】')).then(|| trimmed.to_string())
}

fn plain(text: &str) -> Vec<HmlInline> {
    if text.is_empty() {
        return Vec::new();
    }
    vec![HmlInline::Text {
        text: text.to_string(),
        formats: Vec::new(),
        size: None,
    }]
}

/// 서식 스팬과 크기 구간을 한 줄기의 런으로 합친다. `offset` 은 이 글자열의 첫 글자가
/// 서식 좌표계에서 갖는 위치다(표 문단은 칸, 행 종결자를 뺀 좌표를 쓴다).
fn inline_runs(
    text: &str,
    formats: &FormatParagraph,
    offset: usize,
    image_ids: &[&str],
) -> Vec<HmlInline> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return image_ids
            .iter()
            .map(|id| HmlInline::Image {
                image_id: (*id).to_string(),
            })
            .collect();
    }
    let shift = |value: u32| (value as usize).saturating_sub(offset).min(chars.len());
    let mut boundaries = vec![0_usize, chars.len()];
    for span in &formats.spans {
        boundaries.push(shift(span.start));
        boundaries.push(shift(span.end));
    }
    for run in &formats.sizes {
        boundaries.push(shift(run.start));
        boundaries.push(shift(run.end));
    }
    // `_` 수가 그림 수와 같으면 `_` 하나하나가 그림이다 — 한 런 안에 그림 여럿(「R6는 _, _, _」 pct_a · pct_b)을 가른다.
    // 같지 않으면(글자 밑줄이 섞임) 런 통째로 `_` 인 것만 그림으로 본다.
    if chars.iter().filter(|value| **value == '_').count() == image_ids.len() {
        for (at, _) in chars.iter().enumerate().filter(|(_, value)| **value == '_') {
            boundaries.push(at);
            boundaries.push(at + 1);
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut images = image_ids.iter();
    let mut out: Vec<HmlInline> = Vec::new();
    for window in boundaries.windows(2) {
        let (start, end) = (window[0], window[1]);
        if start >= end {
            continue;
        }
        let slice: String = chars[start..end].iter().collect();
        // 그림 자리표는 `_` 한 글자다.
        if slice.chars().all(|value| value == '_')
            && let Some(id) = images.next()
        {
            out.push(HmlInline::Image {
                image_id: (*id).to_string(),
            });
            continue;
        }
        let here = formats
            .spans
            .iter()
            .find(|span| shift(span.start) <= start && end <= shift(span.end))
            .map(|span| span.formats.clone())
            .unwrap_or_default();
        let size = formats
            .sizes
            .iter()
            .find(|run| shift(run.start) <= start && end <= shift(run.end))
            .map(|run| run.size);
        push_run(&mut out, slice, here, size);
    }
    out
}

/// 칸 글자의 서식과 **크기**를 한 줄기의 런으로 합친다.
/// ⛔ 크기를 빼먹으면 표 셀 10pt 가 왕복에서 12pt 로 눌린다(PCT 표 211자).
fn cell_inline_runs(record: &CellFormatRecord) -> Vec<HmlInline> {
    let chars: Vec<char> = record.text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let mut boundaries = vec![0_usize, chars.len()];
    for span in &record.spans {
        boundaries.push((span.start as usize).min(chars.len()));
        boundaries.push((span.end as usize).min(chars.len()));
    }
    for run in &record.sizes {
        boundaries.push((run.start as usize).min(chars.len()));
        boundaries.push((run.end as usize).min(chars.len()));
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut out = Vec::new();
    for window in boundaries.windows(2) {
        let (start, end) = (window[0], window[1]);
        if start >= end {
            continue;
        }
        let formats = record
            .spans
            .iter()
            .find(|span| span.start as usize <= start && end <= span.end as usize)
            .map(|span| span.formats.clone())
            .unwrap_or_default();
        let size = record
            .sizes
            .iter()
            .find(|run| run.start as usize <= start && end <= run.end as usize)
            .map(|run| run.size);
        push_run(&mut out, chars[start..end].iter().collect(), formats, size);
    }
    out
}

/// 칸 글자의 `_` 수가 그 칸 그림 수와 같으면 `_` 하나하나를 그림 런으로 바꾼다(pct_a 【표 2】 칸 그림 16장).
fn split_image_marks(inlines: Vec<HmlInline>, image_ids: &[&str]) -> Vec<HmlInline> {
    let marks: usize = inlines
        .iter()
        .map(|inline| match inline {
            HmlInline::Text { text, .. } => text.matches('_').count(),
            HmlInline::Image { .. } => 0,
        })
        .sum();
    if image_ids.is_empty() || marks != image_ids.len() {
        return inlines;
    }
    let mut ids = image_ids.iter();
    let mut out = Vec::new();
    for inline in inlines {
        let HmlInline::Text {
            text,
            formats,
            size,
        } = inline
        else {
            out.push(inline);
            continue;
        };
        for (index, piece) in text.split('_').enumerate() {
            if index > 0
                && let Some(id) = ids.next()
            {
                out.push(HmlInline::Image {
                    image_id: (*id).to_string(),
                });
            }
            push_run(&mut out, piece.to_string(), formats.clone(), size);
        }
    }
    out
}

/// 같은 서식, 같은 크기가 이어지면 한 런으로 붙인다.
fn push_run(
    out: &mut Vec<HmlInline>,
    text: String,
    formats: Vec<CharacterFormat>,
    size: Option<u32>,
) {
    if text.is_empty() {
        return;
    }
    if let Some(HmlInline::Text {
        text: previous,
        formats: previous_formats,
        size: previous_size,
    }) = out.last_mut()
        && *previous_formats == formats
        && *previous_size == size
    {
        previous.push_str(&text);
        return;
    }
    out.push(HmlInline::Text {
        text,
        formats,
        size,
    });
}
