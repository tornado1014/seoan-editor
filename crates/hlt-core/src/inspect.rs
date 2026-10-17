use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

use encoding_rs::{DecoderResult, EUC_KR};
use flate2::read::DeflateDecoder;
use sha2::{Digest, Sha256};

use crate::{
    CellRecord, CoreError, ErrorCode, FixedBlockFingerprints, ImageOccurrence, InspectSummary,
    LineRecord, ParagraphRecord, SectionRecord, ShapeRecord, SourceRange, TableRecord,
    VerifyOptions, read_layout, verify_path,
};

const BUFFER_SIZE: usize = 64 * 1024;
const SECTION_START: u64 = 0x2c;
const SECTION_RECORD_SIZE: u64 = 67;
const SECTION_TITLE_CAPACITY: usize = 51;
pub(crate) const CHAR_MARK: &[u8] = b"~`!@#$%^&*()-+|=-TeCfMt\r\n";
const TAIL_SCAN_LIMIT: u64 = 64 * 1024 * 1024;
const DEFAULT_PARAGRAPH_TEXT_BYTES: u64 = 16 * 1024;
const DEFAULT_TEXT_INDEX_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct InspectOptions {
    pub max_inflated_bytes: u64,
    pub max_paragraph_text_bytes: u64,
    pub max_text_index_bytes: u64,
    pub include_image_identities: bool,
    /// 이미지 객체의 픽셀까지 읽는다. `.hlz` 로 내보낼 때만 켠다 (문서 하나가 수십 MB 다).
    pub include_image_payloads: bool,
}

impl Default for InspectOptions {
    fn default() -> Self {
        Self {
            max_inflated_bytes: 4 * 1024 * 1024 * 1024,
            max_paragraph_text_bytes: DEFAULT_PARAGRAPH_TEXT_BYTES,
            max_text_index_bytes: DEFAULT_TEXT_INDEX_BYTES,
            include_image_identities: false,
            include_image_payloads: false,
        }
    }
}

pub struct InspectDocument {
    _spool: tempfile::NamedTempFile,
    /// `include_image_payloads` 를 켰을 때만 채워진다. 키는 객체 코드다.
    pub image_payloads: BTreeMap<u32, HltImagePayload>,
    pub summary: InspectSummary,
    pub sections: Vec<SectionRecord>,
    pub paragraphs: Vec<ParagraphRecord>,
    pub lines: Vec<LineRecord>,
    pub shapes: Vec<ShapeRecord>,
    pub tables: Vec<TableRecord>,
    pub cells: Vec<CellRecord>,
    pub images: Vec<ImageOccurrence>,
    /// 표 칸 줄 → 칸 글자 왼쪽 여백(twip) = 행 들여쓰기(행 레코드 +32) + 칸 안 여백(칸 레코드 +44).
    /// 인쇄 배치(`print.rs`)가 쓴다. 예: 1080+108 · 0+40 · 3+15.
    pub(crate) cell_text_offsets: BTreeMap<u64, u32>,
}

struct RawParagraph {
    range: SourceRange,
    text: String,
    text_truncated: bool,
    first_line: u64,
    line_ranges: Vec<SourceRange>,
}

pub(crate) struct TailLayout {
    pub(crate) font_table: u64,
    pub(crate) register: u64,
    pub(crate) shapes: SourceRange,
    pub(crate) shape_order: SourceRange,
    pub(crate) second_shape_order: SourceRange,
    pub(crate) style_table: SourceRange,
}

struct RawCell {
    start: u32,
    width: u32,
    vertical_align: u32,
    rowspan: u32,
    continuation: bool,
    line_indices: Vec<u64>,
    source_range: SourceRange,
}

#[derive(Debug)]
struct CharSpan {
    source_range: SourceRange,
    codes: Vec<u32>,
}

#[derive(Debug)]
pub(crate) struct EditCharSpan {
    pub(crate) source_range: SourceRange,
    pub(crate) codes: Vec<u32>,
}

#[derive(Debug)]
pub(crate) struct EditLayout {
    pub(crate) body_start: u64,
    pub(crate) body_end: u64,
    pub(crate) char_base: u64,
    pub(crate) object_table: u64,
    pub(crate) line_base: u64,
    pub(crate) total_lines: u64,
    pub(crate) char_spans: Vec<EditCharSpan>,
}

#[derive(Debug, Clone, Copy)]
struct CellGeometry {
    start: u32,
    width: u32,
    continuation: bool,
    vertical_align: u32,
}

/// HLT 안의 이미지 픽셀. TIFF 무압축이라 스트립을 그대로 들고 온다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HltImagePayload {
    pub object_code: u32,
    pub width: u32,
    pub height: u32,
    pub samples: u32,
    pub bits: u32,
    /// 무압축 스트립 픽셀. 압축 TIFF(국내출원 1비트 G4)면 비어 있다.
    pub pixels: Vec<u8>,
    /// 객체가 들고 있는 TIFF 원본 바이트. 압축 여부와 무관하게 그대로 복사한 것이다.
    pub tiff: Vec<u8>,
}

struct ObjectInfo {
    object_type: u32,
    payload_sha256: Option<String>,
    display_width: u32,
    display_height: u32,
    box_width: u32,
    box_height: u32,
}

type ParsedTables = (Vec<TableRecord>, Vec<CellRecord>, BTreeMap<u64, String>);

pub fn inspect_path(path: &Path, options: InspectOptions) -> Result<InspectDocument, CoreError> {
    // 4KB 창 증명은 「통특기가 다시 읽을 수 있게 썼는가」다. 읽기에는 필요 없다 — 통특기 자신의
    // 서식 .frm 도 fixed 첫 블록이라 증명을 못 낸다. verify 는 CRC·크기를 창보다 먼저 보므로
    // WindowNotProven 이면 무결성은 이미 통과했다. 저장 출력은 save.rs 가 다시 verify 한다.
    //
    // 통특기 서식의 꼬리 뒤 줄바꿈 군더더기(SpIdea1)는 verify 를 못 지나지만 읽는다(`template::inflate_hlt` 가 그 모양만 받는다).
    // 무압축 저장(0x17 = 0 · 꼬리 없음 · PctPatE/ED · plain 저장본)은 이제 verify 가 받는다(`codec::is_stored`).
    let mut tolerated: Option<Vec<u8>> = None;
    match verify_path(
        path,
        VerifyOptions {
            max_inflated_bytes: options.max_inflated_bytes,
            ..VerifyOptions::default()
        },
    ) {
        Err(error) if error.code == ErrorCode::DeflateTrailingData => {
            tolerated = Some(crate::inflate_hlt(path).map_err(|_| error)?.1);
        }
        Err(error) if error.code != ErrorCode::WindowNotProven => return Err(error),
        _ => {}
    }
    let mut input = File::open(path).map_err(|_| inspect_error(ErrorCode::IoError, "open", 0))?;
    let layout = read_layout(
        &mut input,
        path.file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("input.HLT"),
    )?;
    // 무압축 저장은 본문이 원 바이트다 — 판정은 verify 와 같은 자리(`codec::is_stored`).
    if tolerated.is_none()
        && crate::codec::is_stored(&mut input, &layout, &VerifyOptions::default())?
    {
        let mut raw = Vec::new();
        input
            .seek(SeekFrom::Start(layout.stream_start))
            .and_then(|_| input.read_to_end(&mut raw))
            .map_err(|_| {
                inspect_error(ErrorCode::IoError, "inspect_inflate", layout.stream_start)
            })?;
        tolerated = Some(raw);
    }
    let title_end = layout.header[0x24..0x4c]
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(0x28);
    let document_title = decode_cp949_field(&layout.header[0x24..0x24 + title_end], 0x24)?;
    let document_type_raw = u32::from_le_bytes(
        layout.header[0x20..0x24]
            .try_into()
            .expect("four-byte document type field"),
    );
    let document_tree_kind = crate::DocumentTreeKind::from_raw(document_type_raw);
    input
        .seek(SeekFrom::Start(layout.stream_start))
        .map_err(|_| inspect_error(ErrorCode::IoError, "inspect_inflate", layout.stream_start))?;
    let compressed = input.take(layout.footer_offset - layout.stream_start);
    let mut decoder = DeflateDecoder::new(compressed);
    let mut spool = tempfile::NamedTempFile::new()
        .map_err(|_| inspect_error(ErrorCode::IoError, "inspect_spool", 0))?;
    let mut buffer = [0_u8; BUFFER_SIZE];
    let mut inflated = 0_u64;
    if let Some(raw) = &tolerated {
        inflated = raw.len() as u64;
        if inflated > options.max_inflated_bytes {
            return Err(inspect_error(
                ErrorCode::SizeLimitExceeded,
                "inspect_inflate",
                inflated,
            ));
        }
        spool
            .write_all(raw)
            .map_err(|_| inspect_error(ErrorCode::IoError, "inspect_spool", 0))?;
    }
    while tolerated.is_none() {
        let read = decoder
            .read(&mut buffer)
            .map_err(|_| inspect_error(ErrorCode::DeflateInvalid, "inspect_inflate", inflated))?;
        if read == 0 {
            break;
        }
        inflated = inflated.checked_add(read as u64).ok_or_else(|| {
            inspect_error(ErrorCode::SizeLimitExceeded, "inspect_inflate", inflated)
        })?;
        if inflated > options.max_inflated_bytes {
            return Err(inspect_error(
                ErrorCode::SizeLimitExceeded,
                "inspect_inflate",
                inflated,
            ));
        }
        spool
            .write_all(&buffer[..read])
            .map_err(|_| inspect_error(ErrorCode::IoError, "inspect_spool", inflated))?;
    }
    spool
        .flush()
        .map_err(|_| inspect_error(ErrorCode::IoError, "inspect_spool", inflated))?;
    if tolerated.is_none() && inflated != u64::from(layout.footer.inflated_size) {
        return Err(inspect_error(
            ErrorCode::FooterSizeMismatch,
            "inspect_inflate",
            inflated,
        ));
    }

    let cell_record = cell_record_size(&layout.header)?;
    let file = spool.as_file_mut();
    let mut sections = parse_sections(file)?;
    let table_end = SECTION_START + sections.len() as u64 * SECTION_RECORD_SIZE;
    let body_start = find_body_start(file, table_end, inflated)?;
    let body_end = find_pattern(file, body_start, inflated, CHAR_MARK)?
        .ok_or_else(|| inspect_error(ErrorCode::StructureInvalid, "body", body_start))?;
    let (mut raw_paragraphs, raw_line_count) = parse_body(
        file,
        body_start,
        body_end,
        options.max_paragraph_text_bytes,
        options.max_text_index_bytes,
    )?;
    let (line_base, total_lines) = find_line_base(file, inflated, raw_line_count)?;
    let shapes = parse_shapes(file, line_base, total_lines)?;
    let shape_by_index: BTreeMap<u32, ShapeRecord> = shapes
        .iter()
        .cloned()
        .map(|shape| (shape.index, shape))
        .collect();
    let (title_slots, wipo_slots) = parse_register(
        file,
        inflated,
        line_base,
        total_lines,
        cell_record,
        register_paired(&layout.header),
    )?;
    let fixed_layout = locate_tail(file, inflated, line_base, total_lines, cell_record)?;
    let fixed_blocks = FixedBlockFingerprints {
        shapes_sha256: hash_range(file, fixed_layout.shapes)?,
        shape_order_sha256: hash_range(file, fixed_layout.shape_order)?,
        second_shape_order_sha256: hash_range(file, fixed_layout.second_shape_order)?,
        style_table_sha256: hash_range(file, fixed_layout.style_table)?,
    };

    let mut line_to_paragraph = BTreeMap::new();
    let mut lines = Vec::new();
    for (part, raw) in raw_paragraphs.iter_mut().enumerate() {
        raw.line_ranges
            .truncate(total_lines.saturating_sub(raw.first_line) as usize);
        for (offset, range) in raw.line_ranges.iter().enumerate() {
            let index = raw.first_line + offset as u64;
            let id = format!("line:{index}");
            line_to_paragraph.insert(index, part);
            // 줄 배열 12바이트 레코드의 첫 워드가 문단모양 색인이다.
            let shape_id = if index < total_lines {
                Some(format!("shape:{}", read_u32(file, line_base + 12 * index)?))
            } else {
                None
            };
            lines.push(LineRecord {
                id,
                paragraph_id: format!("paragraph:{part}"),
                index,
                shape_id,
                opens_section_id: None,
                source_range: *range,
            });
        }
    }

    let anchors: Vec<(u64, u32)> = title_slots
        .iter()
        .enumerate()
        .filter_map(|(line, value)| (*value != 0).then_some((line as u64, *value)))
        .collect();
    for section in &mut sections {
        if let Some((line, _)) = anchors.iter().find(|(_, index)| *index == section.index) {
            section.anchor_line_id = Some(format!("line:{line}"));
            section.anchor_paragraph_id = line_to_paragraph
                .get(line)
                .map(|part| format!("paragraph:{part}"));
        }
    }
    // 절 줄지도 원본을 줄마다 실어 둔다. 절 목록의 앵커는 절마다 **첫** 슬롯만 집으므로
    // 같은 절이 두 줄에 실린 것을 못 본다. 그 중복이 곧 통특기 구조창이 뒤 절을 앞 절
    // 밑으로 말아 넣는 원인이다(2026-08-27 결함 B).
    for (line, index) in &anchors {
        if let Some(record) = lines.iter_mut().find(|record| record.index == *line) {
            record.opens_section_id = Some(format!("section:{index}"));
        }
    }

    let (tables, mut cells, line_to_cell) = parse_tables(
        file,
        &raw_paragraphs,
        line_base,
        total_lines,
        &sections,
        inflated,
        cell_record,
    )?;
    let cell_text_offsets =
        cell_text_offsets(file, inflated, line_base, total_lines, cell_record).unwrap_or_default();
    let (spans, object_table) =
        parse_char_spans(file, body_end + CHAR_MARK.len() as u64, total_lines)?;
    let object_types = parse_object_types(file, object_table, options.include_image_identities)?;
    let image_payloads = if options.include_image_payloads {
        parse_image_payloads(file, object_table)?
    } else {
        BTreeMap::new()
    };
    let images = parse_images(file, &lines, &spans, &object_types, &line_to_cell, &cells)?;
    let cell_positions: BTreeMap<String, usize> = cells
        .iter()
        .enumerate()
        .map(|(index, cell)| (cell.id.clone(), index))
        .collect();
    for image in &images {
        if let Some(cell_id) = &image.cell_id
            && let Some(index) = cell_positions.get(cell_id)
        {
            cells[*index].image_ids.push(image.id.clone());
        }
    }

    let mut paragraphs = Vec::with_capacity(raw_paragraphs.len());
    for (part, raw) in raw_paragraphs.into_iter().enumerate() {
        let first_line = raw.first_line;
        // ⛔ 문단모양은 **칸에 들지 않은 첫 줄**에서 잡는다. 표를 품은 문단은 첫 줄이
        // 표 칸이라 첫 줄에서 잡으면 표 전용 모양(줄 높이 130)을 물고 나오고, 다시 구울 때
        // 표 뒤 본문이 표 높이로 눌린다(국내 출원서 한 줄 · PCT 명세서 열일곱 줄이 이 자리에서 눌렸다).
        // 줄 배열 12바이트 레코드의 두 번째 워드가 칸 번호이고 0 이면 표 밖이다.
        let mut shape_line = None;
        for offset in 0..raw.line_ranges.len() as u64 {
            let line = first_line + offset;
            if line >= total_lines {
                break;
            }
            if read_u32(file, line_base + 12 * line + 4)? == 0 {
                shape_line = Some(line);
                break;
            }
        }
        let shape_line = shape_line.or_else(|| (first_line < total_lines).then_some(first_line));
        let shape_index = match shape_line {
            Some(line) => Some(read_u32(file, line_base + 12 * line)?),
            None => None,
        };
        let shape = shape_index.and_then(|index| shape_by_index.get(&index));
        // ⛔ 문단이 여는 절은 첫 줄로 정하지 마라. 페이지브레이크가 첫 줄을 차지한 문단은
        // 절 표제가 둘째 줄에 있다(정본 `domestic_출원서` 의 줄 832 페이지브레이크 +
        // 833 `【도면】` 이 한 문단). 첫 줄로 정하면 그 문단이 앞 절을 물고 나오고,
        // 다시 구울 때 그 절이 앵커를 잃어 구조창에서 도면이 앞 절 밑으로 말려든다.
        // 문단 안에 앵커가 있으면 그것이 이기고, 없을 때만 앞 절에 딸린다.
        let last_line = first_line + raw.line_ranges.len().saturating_sub(1) as u64;
        let section_id = anchors
            .iter()
            .find(|(line, _)| (first_line..=last_line).contains(line))
            .or_else(|| anchors.iter().rev().find(|(line, _)| *line <= first_line))
            .map(|(_, index)| format!("section:{index}"));
        let wipo_id = if first_line > 0 {
            wipo_slots
                .get(first_line as usize - 1)
                .copied()
                .filter(|value| *value != 0)
        } else {
            None
        };
        paragraphs.push(ParagraphRecord {
            id: format!("paragraph:{part}"),
            part: part as u64,
            section_id,
            text: raw.text,
            text_truncated: raw.text_truncated,
            shape_id: shape_index.map(|index| format!("shape:{index}")),
            indent_raw: shape.map_or(0, |value| value.indent_raw),
            lspace: shape.map_or(0, |value| value.lspace),
            align: shape.map_or_else(|| "unknown".to_string(), |value| value.align.clone()),
            line_ids: raw
                .line_ranges
                .iter()
                .enumerate()
                .map(|(offset, _)| format!("line:{}", first_line + offset as u64))
                .collect(),
            source_range: raw.range,
            wipo_id,
        });
    }

    let wipo_nonzero = wipo_slots.iter().filter(|value| **value != 0).count() as u64;
    let wipo_distinct = wipo_slots
        .iter()
        .filter(|value| **value != 0)
        .copied()
        .collect::<BTreeSet<_>>()
        .len() as u64;
    let text_index_bytes = paragraphs
        .iter()
        .map(|paragraph| paragraph.text.len() as u64)
        .sum();
    let text_index_truncated = paragraphs.iter().any(|paragraph| paragraph.text_truncated);
    let summary = InspectSummary {
        document_title,
        document_type_raw,
        document_tree_kind,
        document_tree_label: document_tree_kind.label().to_string(),
        fixed_blocks,
        inflated_bytes: inflated,
        section_count: sections.len() as u64,
        paragraph_count: paragraphs.len() as u64,
        line_count: total_lines,
        span_count: spans.len() as u64,
        table_count: tables.len() as u64,
        cell_count: cells.len() as u64,
        image_count: images.len() as u64,
        text_index_bytes,
        text_index_truncated,
        paragraph_wipoid_nonzero: wipo_nonzero,
        paragraph_wipoid_distinct: wipo_distinct,
    };
    Ok(InspectDocument {
        _spool: spool,
        image_payloads,
        summary,
        sections,
        paragraphs,
        lines,
        shapes,
        tables,
        cells,
        images,
        cell_text_offsets,
    })
}

/// 칸 줄마다 글자 왼쪽 여백(twip). 행 표는 칸 표(`font_table`) 바로 앞의 같은 크기 레코드 표다(생성기 순서와 같다).
fn cell_text_offsets(
    file: &mut File,
    inflated: u64,
    line_base: u64,
    total_lines: u64,
    cell_record: u64,
) -> Result<BTreeMap<u64, u32>, CoreError> {
    let tail = locate_tail(file, inflated, line_base, total_lines, cell_record)?;
    let cells = read_u32(file, tail.font_table + 1)?;
    let mut rows_start = None;
    for count in 1..=u64::from(cells) + 64 {
        let Some(start) = tail.font_table.checked_sub(5 + cell_record * count) else {
            break;
        };
        if read_byte(file, start)? == 0xbf && u64::from(read_u32(file, start + 1)?) == count {
            rows_start = Some((start, count));
            break;
        }
    }
    let Some((rows_start, rows)) = rows_start else {
        return Ok(BTreeMap::new());
    };
    let mut out = BTreeMap::new();
    for line in 0..total_lines {
        let cell = u64::from(read_u32(file, line_base + 12 * line + 4)?);
        if cell == 0 || cell >= u64::from(cells) {
            continue;
        }
        let record = tail.font_table + 5 + cell_record * cell;
        let row = u64::from(read_u32(file, record + 4)?);
        let margin = read_u32(file, record + 44)?;
        let indent = if row > 0 && row < rows {
            read_u32(file, rows_start + 5 + cell_record * row + 32)?
        } else {
            0
        };
        out.insert(line, indent.saturating_add(margin));
    }
    Ok(out)
}

/// 제자리 고쳐 쓰기용 — 줄마다 글자 바이트와 글자모양 코드가 짝이 맞아야 한다(어긋나면 거절).
pub(crate) fn inspect_edit_layout(file: &mut File, inflated: u64) -> Result<EditLayout, CoreError> {
    edit_layout(file, inflated, false)
}

/// 읽기용(서식 목록 · XML · 제출파일 · 편집기 열기) — 통특기처럼 어긋난 줄도 읽는다(`fit_codes`).
pub(crate) fn inspect_read_layout(file: &mut File, inflated: u64) -> Result<EditLayout, CoreError> {
    edit_layout(file, inflated, true)
}

fn edit_layout(file: &mut File, inflated: u64, lenient: bool) -> Result<EditLayout, CoreError> {
    let sections = parse_sections(file)?;
    let table_end = SECTION_START + sections.len() as u64 * SECTION_RECORD_SIZE;
    let body_start = find_body_start(file, table_end, inflated)?;
    let body_end = find_pattern(file, body_start, inflated, CHAR_MARK)?
        .ok_or_else(|| inspect_error(ErrorCode::StructureInvalid, "body", body_start))?;
    let (mut paragraphs, raw_line_count) = parse_body(file, body_start, body_end, 0, 0)?;
    let (line_base, total_lines) = find_line_base(file, inflated, raw_line_count)?;
    let char_base = body_end + CHAR_MARK.len() as u64;
    let (mut spans, object_table) = parse_char_spans(file, char_base, total_lines)?;
    validate_char_span_invariants(file, &mut paragraphs, total_lines, &mut spans, lenient)?;
    Ok(EditLayout {
        body_start,
        body_end,
        char_base,
        object_table,
        line_base,
        total_lines,
        char_spans: spans
            .into_iter()
            .map(|span| EditCharSpan {
                source_range: span.source_range,
                codes: span.codes,
            })
            .collect(),
    })
}

fn validate_char_span_invariants(
    file: &mut File,
    paragraphs: &mut [RawParagraph],
    total_lines: u64,
    spans: &mut [CharSpan],
    lenient: bool,
) -> Result<(), CoreError> {
    for paragraph in paragraphs {
        paragraph
            .line_ranges
            .truncate(total_lines.saturating_sub(paragraph.first_line) as usize);
        for (offset, range) in paragraph.line_ranges.iter().enumerate() {
            let line = paragraph.first_line + offset as u64;
            let span = spans.get_mut(line as usize).ok_or_else(|| {
                inspect_error(ErrorCode::StructureInvalid, "char_span_invariant", line)
            })?;
            if span.codes.len() == 1 {
                continue;
            }
            let raw = read_range(file, range.start_byte, range.end_byte - range.start_byte)?;
            let last = offset + 1 == paragraph.line_ranges.len();
            if lenient {
                fit_codes(&raw, last, &mut span.codes);
                continue;
            }
            let expected = raw.len() + usize::from(last);
            if span.codes.len() != expected {
                return Err(inspect_error(
                    ErrorCode::StructureInvalid,
                    "char_span_invariant",
                    span.source_range.start_byte,
                ));
            }
            let mut position = 0_usize;
            while position < raw.len() {
                if raw[position] >= 0x81 {
                    if position + 1 >= raw.len() || span.codes[position] != span.codes[position + 1]
                    {
                        return Err(inspect_error(
                            ErrorCode::StructureInvalid,
                            "char_span_invariant",
                            span.source_range.start_byte + 4 + 4 * position as u64,
                        ));
                    }
                    position += 2;
                } else {
                    position += 1;
                }
            }
            // 문단 마지막 줄의 종결자 몫 슬롯: 통특기가 0 이 아닌 코드를 넣기도 한다.
            // 네이티브 PCT 저장본 — 밑줄 표제 다음 문단의 종결자가 코드 2(밑줄)다.
            // 우리 생성기는 0 을 쓰지만, 판독에서 0 을 강제하면 실물을 못 읽는다.
            let _ = last;
        }
    }
    Ok(())
}

/// 한 줄의 글자모양 코드를 글자 바이트 수(+끝 줄 종결자 몫 1)에 맞춘다. 바꿨으면 true.
/// 남는 코드는 버리고, 모자라면 마지막 코드를 잇고, 2바이트 글자의 둘째 코드는 첫째 코드로 맞춘다.
/// 남는 쪽 · 서식 없는 본문은 통특기 XML 과 같다.
/// ⚠ 모자란 쪽 · 서식 있는 줄은 아직 확인 전이다(마지막 코드 = 런 연속 가정).
fn fit_codes(raw: &[u8], last: bool, codes: &mut Vec<u32>) -> bool {
    let expected = raw.len() + usize::from(last);
    let before = codes.clone();
    let fill = codes.last().copied().unwrap_or(0);
    codes.resize(expected, fill);
    let mut position = 0_usize;
    while position < raw.len() {
        if raw[position] >= 0x81 && position + 1 < raw.len() {
            codes[position + 1] = codes[position];
            position += 2;
        } else {
            position += 1;
        }
    }
    *codes != before
}

/// 절 테이블은 index 필드가 0xFFFFFFFF 인 종결 레코드로 끝나고 본문은 그 바로 뒤에서
/// 시작한다. 실물 74건 중 72건에서 이 자리가 첫 【(EUC-KR a1 bc) 위치와 정확히 일치한다.
/// 【 만 훑으면 표제가 하나도 없는 의견서에서 한글 두 바이트 사이의 a1 bc
/// (예: "에서" = bf a1 bc ad)에 걸려 홀수 경계에서 디코딩을 시작한다.
fn find_body_start(file: &mut File, table_end: u64, inflated: u64) -> Result<u64, CoreError> {
    let after_terminator = table_end + SECTION_RECORD_SIZE;
    if after_terminator <= inflated && read_u32(file, table_end + 55)? == u32::MAX {
        return Ok(after_terminator);
    }
    find_pattern(file, table_end, inflated, &[0xa1, 0xbc])?
        .ok_or_else(|| inspect_error(ErrorCode::StructureInvalid, "body", table_end))
}

fn parse_sections(file: &mut File) -> Result<Vec<SectionRecord>, CoreError> {
    let mut sections = Vec::new();
    let mut offset = SECTION_START;
    loop {
        let record = read_range(file, offset, SECTION_RECORD_SIZE)?;
        let length =
            u32::from_le_bytes(record[51..55].try_into().expect("section length")) as usize;
        let index = u32::from_le_bytes(record[55..59].try_into().expect("section index"));
        // 번호 0 은 끝이 아니다 — 0x2b 서식(DesTran·Opinion·PctPat…)은 첫 절이 0 번이다. 끝은 길이 0·종결 레코드·【 검사가 정한다.
        if length == 0 || length > SECTION_TITLE_CAPACITY || index == u32::MAX {
            break;
        }
        if record[length..SECTION_TITLE_CAPACITY]
            .iter()
            .any(|value| *value != 0)
        {
            break;
        }
        // 0x2b 서식은 절 표 자리에 힙 쓰레기(Change)나 본문(DesTran)이 바로 온다 — 풀기 전에 【 로 거른다.
        if !record.starts_with(&[0xa1, 0xbc]) {
            break;
        }
        let title = decode_cp949_field(&record[..length], offset)?;
        sections.push(SectionRecord {
            id: format!("section:{index}"),
            index,
            title,
            table_range: SourceRange {
                start_byte: offset,
                end_byte: offset + SECTION_RECORD_SIZE,
            },
            source_range: SourceRange {
                start_byte: offset,
                end_byte: offset + length as u64,
            },
            anchor_line_id: None,
            anchor_paragraph_id: None,
        });
        offset += SECTION_RECORD_SIZE;
    }
    Ok(sections)
}

fn parse_body(
    file: &mut File,
    body_start: u64,
    body_end: u64,
    max_paragraph_text_bytes: u64,
    max_text_index_bytes: u64,
) -> Result<(Vec<RawParagraph>, u64), CoreError> {
    let mut paragraphs = Vec::new();
    let mut paragraph_start = body_start;
    let mut first_line = 0_u64;
    let mut text_budget = max_text_index_bytes;
    loop {
        let paragraph_end =
            find_pattern(file, paragraph_start, body_end, &[0x15])?.unwrap_or(body_end);
        let core_start = if paragraph_end >= paragraph_start + 2
            && read_range(file, paragraph_start, 2)? == b"\r\n"
        {
            paragraph_start + 2
        } else {
            paragraph_start
        };
        let mut line_ranges = Vec::new();
        let mut line_start = core_start;
        while let Some(line_end) = find_pattern(file, line_start, paragraph_end, b"\r\n")? {
            line_ranges.push(SourceRange {
                start_byte: line_start,
                end_byte: line_end,
            });
            line_start = line_end + 2;
        }
        line_ranges.push(SourceRange {
            start_byte: line_start,
            end_byte: paragraph_end,
        });
        let capture_limit = max_paragraph_text_bytes.min(text_budget);
        let (text, text_truncated, captured_bytes) =
            read_text_preview(file, core_start, paragraph_end, capture_limit)?;
        text_budget = text_budget.saturating_sub(captured_bytes);
        let count = line_ranges.len() as u64;
        paragraphs.push(RawParagraph {
            range: SourceRange {
                start_byte: paragraph_start,
                end_byte: paragraph_end,
            },
            text,
            text_truncated,
            first_line,
            line_ranges,
        });
        first_line += count;
        if paragraph_end == body_end {
            break;
        }
        paragraph_start = paragraph_end + 1;
    }
    Ok((paragraphs, first_line))
}

fn read_text_preview(
    file: &mut File,
    start: u64,
    end: u64,
    limit: u64,
) -> Result<(String, bool, u64), CoreError> {
    file.seek(SeekFrom::Start(start))
        .map_err(|_| inspect_error(ErrorCode::IoError, "body_text", start))?;
    let mut position = start;
    let mut preview = Vec::new();
    let mut truncated = false;
    let mut buffer = [0_u8; BUFFER_SIZE];
    let mut decoder = EUC_KR.new_decoder_without_bom_handling();
    while position < end {
        let want =
            usize::try_from((end - position).min(BUFFER_SIZE as u64)).expect("bounded body read");
        let read = file
            .read(&mut buffer[..want])
            .map_err(|_| inspect_error(ErrorCode::IoError, "body_text", position))?;
        if read == 0 {
            break;
        }
        let last = position + read as u64 == end;
        let capacity = decoder
            .max_utf8_buffer_length_without_replacement(read)
            .ok_or_else(|| inspect_error(ErrorCode::ResourceTooLarge, "cp949", position))?;
        let mut decoded = String::with_capacity(capacity);
        let (result, consumed) =
            decoder.decode_to_string_without_replacement(&buffer[..read], &mut decoded, last);
        match result {
            DecoderResult::InputEmpty if consumed == read => {}
            DecoderResult::Malformed(_, _) => {
                return Err(inspect_error(
                    ErrorCode::Cp949Invalid,
                    "cp949",
                    position + consumed as u64,
                ));
            }
            DecoderResult::OutputFull | DecoderResult::InputEmpty => {
                return Err(inspect_error(
                    ErrorCode::ResourceTooLarge,
                    "cp949",
                    position + consumed as u64,
                ));
            }
        }
        for value in &buffer[..read] {
            if matches!(*value, b'\r' | b'\n' | 0) {
                continue;
            }
            if (preview.len() as u64) < limit {
                preview.push(*value);
            } else {
                truncated = true;
            }
        }
        position += read as u64;
    }
    // ⛔ 앞뒤 스페이스를 잘라내지 않는다. 통특기 제출 변환은 문단 글자를 그대로 쓴다(뒤 공백
    //    `있다. </p>` · 공백 한 칸 문단 `<p> </p>`). 잘라내면
    //    KIPO XML 이 통특기와 달라지고 공백 문단은 번호까지 사라진다.
    let preview = preview.as_slice();
    let text = match decode_cp949(preview, start) {
        Ok(text) => text,
        Err(error) if truncated && !preview.is_empty() => {
            decode_cp949(&preview[..preview.len() - 1], start).map_err(|_| error)?
        }
        Err(error) => return Err(error),
    };
    Ok((text, truncated, preview.len() as u64))
}

fn find_line_base(file: &mut File, inflated: u64, raw_lines: u64) -> Result<(u64, u64), CoreError> {
    let scan_start = inflated.saturating_sub(TAIL_SCAN_LIMIT);
    let tail = read_range(file, scan_start, inflated - scan_start)?;
    for index in (0..tail.len()).rev() {
        if tail[index] != 0xbf || index + 13 > tail.len() {
            continue;
        }
        let base = scan_start + index as u64 + 1;
        let first = u32::from_le_bytes(tail[index + 1..index + 5].try_into().expect("line first"));
        if first == 0
            || first >= 4096
            || tail[index + 5..index + 13].iter().any(|value| *value != 0)
        {
            continue;
        }
        for candidate in [
            raw_lines,
            raw_lines.saturating_sub(1),
            raw_lines.saturating_sub(2),
        ] {
            if candidate == 0 {
                continue;
            }
            let counter = base + 12 * candidate;
            if counter >= inflated {
                continue;
            }
            if read_byte(file, counter)? == 0xbf && shape_probe(file, counter + 13)? {
                return Ok((base, candidate));
            }
        }
    }
    Err(inspect_error(
        ErrorCode::StructureInvalid,
        "line_array",
        inflated,
    ))
}

fn shape_probe(file: &mut File, base: u64) -> Result<bool, CoreError> {
    let probe = match read_range(file, base, 8 * 72 + 48) {
        Ok(value) => value,
        Err(_) => return Ok(false),
    };
    for offset in (0..8 * 72).step_by(4) {
        let lspace = u32::from_le_bytes(probe[offset..offset + 4].try_into().expect("lspace"));
        if matches!(lspace, 130 | 160 | 280 | 300) {
            let align =
                u32::from_le_bytes(probe[offset + 44..offset + 48].try_into().expect("align"));
            return Ok(matches!(align, 1 | 1024 | 2048));
        }
    }
    Ok(false)
}

fn parse_shapes(
    file: &mut File,
    line_base: u64,
    total_lines: u64,
) -> Result<Vec<ShapeRecord>, CoreError> {
    let counter = line_base + 12 * total_lines;
    let count = read_u32(file, counter + 1)?;
    // 문단모양 테이블은 `shape_count` 바로 뒤(`counter + 5`)에서 72바이트씩 이어진다.
    // 줄 배열이 싣는 값이 이 테이블의 0부터 세는 색인이다.
    // ⛔ 예전에는 줄 높이가 나올 법한 자리를 4바이트씩 훑어 기준점을 잡았다. 그 훑기는
    // 실물 72건 전부 오프셋 32 에 떨어져 들여쓰기와 정렬은 맞았지만, 그 자리가 곧
    // **앞 레코드의** 줄 높이라서 줄 높이만 한 칸 앞에서 읽었다. 앞뒤 레코드의 줄 높이가
    // 같으면 안 드러나고, 다르면 표가 통특기에서 쪽을 넘겼다(2026-08-27 결함 A).
    let base = counter + 5;
    // 줄 배열이 싣는 최대 색인은 실물 74건 전부 `count - 1` 이다(2026-08-27 전수).
    let mut out = Vec::new();
    for index in 1..count {
        let offset = base + 72 * u64::from(index);
        let record = read_range(file, offset, 44)?;
        let indent = u32::from_le_bytes(record[8..12].try_into().expect("indent"));
        let align = u32::from_le_bytes(record[12..16].try_into().expect("align"));
        let lspace = u32::from_le_bytes(record[40..44].try_into().expect("lspace"));
        // ⛔ 정렬 코드로 테이블 끝을 판정하지 마라. 개수(`count`)가 정본이다.
        // 통특기가 새로 만든 빈 명세서에는 정렬 0(기본)이 있고, 실물 74건에는 상위 비트가
        // 붙은 값이 `0x8001`, `0x1800`, `0x2800`, `0x8800`, `0xC400`, `0xC800` 으로
        // 143 레코드 있다(2026-08-27 전수). 이 값들을 끝으로 보고 자르면 뒤 문단모양을
        // 통째로 잃고, 그 모양을 쓰는 줄이 다시 구울 때 기본값으로 무너진다.
        // 실물 5건이 이 자리에 걸렸다. 뜻은 `& 0xFFF` 로 읽고 상위 비트는 버리지 않는다.
        if indent > 100_000 {
            break;
        }
        out.push(ShapeRecord {
            id: format!("shape:{index}"),
            index,
            indent_raw: indent,
            lspace,
            align_raw: align,
            align: match align & 0xFFF {
                0 => "default",
                1 => "center",
                1024 => "left",
                2048 => "justified",
                _ => "unknown",
            }
            .to_string(),
            source_range: SourceRange {
                start_byte: offset,
                end_byte: offset + 72,
            },
        });
    }
    Ok(out)
}

fn parse_register(
    file: &mut File,
    inflated: u64,
    line_base: u64,
    total_lines: u64,
    cell_record: u64,
    paired: bool,
) -> Result<(Vec<u32>, Vec<u32>), CoreError> {
    let layout = locate_tail(file, inflated, line_base, total_lines, cell_record)?;
    let register = layout.register;
    if read_range(file, register, 5)? != b"\xbf\x00\x00\x00\x00" {
        return Err(inspect_error(
            ErrorCode::StructureInvalid,
            "register",
            register,
        ));
    }
    let slot_count = if paired {
        2 * total_lines - 1
    } else {
        total_lines - 1
    };
    let bytes = read_range(file, register + 5, 4 * slot_count)?;
    let slots: Vec<u32> = bytes
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes(chunk.try_into().expect("register slot")))
        .collect();
    if !paired {
        return Ok((vec![0; total_lines as usize], slots));
    }
    Ok((
        slots.iter().step_by(2).copied().collect(),
        slots.iter().skip(1).step_by(2).copied().collect(),
    ))
}

pub(crate) fn locate_tail(
    file: &mut File,
    inflated: u64,
    line_base: u64,
    total_lines: u64,
    cell_record: u64,
) -> Result<TailLayout, CoreError> {
    let counter = line_base + 12 * total_lines;
    let shapes_start = counter + 13;
    let scan_end = shapes_start.saturating_add(TAIL_SCAN_LIMIT).min(inflated);
    let mut search = shapes_start;
    let (shape_order, shape_order_end, font_table) = loop {
        let candidate = find_pattern(file, search, scan_end, b"\xbf")?.ok_or_else(|| {
            inspect_error(ErrorCode::StructureInvalid, "shape_order", shapes_start)
        })?;
        let chain = if let Some(order_end) =
            tagged_table_end(file, candidate, 184, 1, 64, inflated)?
        {
            if let Some(geometry_end) = tagged_table_end(file, order_end, 68, 1, 64, inflated)? {
                tagged_table_end(file, geometry_end, 80, 1, 99_999, inflated)?
                    .filter(|font| read_byte(file, *font).is_ok_and(|tag| tag == 0xbf))
                    .map(|font| (candidate, order_end, font))
            } else {
                None
            }
        } else {
            None
        };
        if let Some(layout) = chain {
            break layout;
        }
        search = candidate + 1;
    };
    if read_byte(file, font_table)? != 0xbf {
        return Err(inspect_error(
            ErrorCode::StructureInvalid,
            "font_table",
            font_table,
        ));
    }
    let second_search = tagged_table_end(file, font_table, cell_record, 1, 99_999, inflated)?
        .ok_or_else(|| inspect_error(ErrorCode::StructureInvalid, "font_table", font_table))?;
    let second =
        find_pattern(file, second_search, inflated, b"\xbf\x01\x00\x00\x00")?.ok_or_else(|| {
            inspect_error(
                ErrorCode::StructureInvalid,
                "second_shape_order",
                second_search,
            )
        })?;
    let style = find_pattern(file, second + 1, inflated, b"\xbf")?
        .ok_or_else(|| inspect_error(ErrorCode::StructureInvalid, "style_table", second + 1))?;
    let style_count = read_u32(file, style + 1)?;
    let register = style + 5 + 192 * u64::from(style_count);
    Ok(TailLayout {
        font_table,
        register,
        shapes: SourceRange {
            start_byte: shapes_start,
            end_byte: shape_order,
        },
        shape_order: SourceRange {
            start_byte: shape_order,
            end_byte: shape_order_end,
        },
        second_shape_order: SourceRange {
            start_byte: second,
            end_byte: style,
        },
        style_table: SourceRange {
            start_byte: style,
            end_byte: register,
        },
    })
}

/// 등록부가 줄마다 (문단 정보 번호, 절 번호) 짝인가. 판 0x31 이하(서식의 0x2b)는 문단 정보 번호만 싣는다
/// (`0x31 < 판` 일 때만 둘째 u32 가 있다 · 서식 Change +4B/줄).
pub(crate) fn register_paired(header: &[u8]) -> bool {
    header[0x15] > 0x31
}

/// 셀 표(`font_table`, TER 리더의 표 B) 레코드 크기. 헤더 0x15 = 포맷 버전, 0x16 = 부 버전.
/// 버전 0x33 은 0x50, 그 아래는 0x4c(끝 u32 없음),
/// 부 버전 10 미만은 또 다른 크기라 증명 전까지 받지 않는다. 통특기 라이터는 늘 0x33 을 쓴다.
pub(crate) fn cell_record_size(header: &[u8]) -> Result<u64, CoreError> {
    let unsupported =
        |offset| inspect_error(ErrorCode::UnsupportedFormat, "format_version", offset);
    match (header[0x15], header[0x16]) {
        (_, minor) if minor < 10 => Err(unsupported(0x16)),
        (0x33, _) => Ok(80),
        (0x32 | 0x2b, _) => Ok(76),
        _ => Err(unsupported(0x15)),
    }
}

fn tagged_table_end(
    file: &mut File,
    start: u64,
    record_size: u64,
    min_count: u32,
    max_count: u32,
    inflated: u64,
) -> Result<Option<u64>, CoreError> {
    if start.checked_add(5).is_none_or(|end| end > inflated) || read_byte(file, start)? != 0xbf {
        return Ok(None);
    }
    let count = read_u32(file, start + 1)?;
    if !(min_count..=max_count).contains(&count) {
        return Ok(None);
    }
    let end = record_size
        .checked_mul(u64::from(count))
        .and_then(|records| start.checked_add(5)?.checked_add(records));
    Ok(end.filter(|value| *value <= inflated))
}

fn hash_range(file: &mut File, range: SourceRange) -> Result<String, CoreError> {
    let bytes = read_range(file, range.start_byte, range.end_byte - range.start_byte)?;
    Ok(format!("{:X}", Sha256::digest(bytes)))
}

fn parse_font_geometry(
    file: &mut File,
    font_table: u64,
    cell_record: u64,
) -> Result<BTreeMap<u32, CellGeometry>, CoreError> {
    if read_byte(file, font_table)? != 0xbf {
        return Err(inspect_error(
            ErrorCode::StructureInvalid,
            "font_table",
            font_table,
        ));
    }
    let count = read_u32(file, font_table + 1)?;
    let mut geometry = BTreeMap::new();
    for cell in 1..count {
        let offset = font_table + 5 + cell_record * u64::from(cell);
        let record = read_range(file, offset, cell_record)?;
        let fields: Vec<u32> = record
            .chunks_exact(4)
            .map(|chunk| u32::from_le_bytes(chunk.try_into().expect("font field")))
            .collect();
        // 첫 칸 = 사용 중 표시(TER `InUse` · 추정). 0 은 지워진 칸으로 통특기도 그대로 둔다(서식 `SpRev` 65~68번). 기하 지도에서만 빼고, 줄이 그 칸을 가리키면 아래에서 거부된다.
        if fields[0] == 0 {
            continue;
        }
        if fields[0] != 1 {
            return Err(inspect_error(
                ErrorCode::GeometryInvalid,
                "font_table",
                offset,
            ));
        }
        geometry.insert(
            cell,
            CellGeometry {
                start: fields[4],
                width: fields[5],
                // 세로병합에 덮인 칸은 field[16] 의 비트 16 으로 표시된다.
                // 값 전체를 보면 안 된다. 옛 저장본은 32/56, 통특기 v6.3 이 새로
                // 만든 저장본은 0/16 을 쓴다. 56 과 16 만 비트 16 을 품는다.
                continuation: fields[16] & 0x10 != 0,
                // +76 = 세로 맞춤(칸 +0x4c). 76B 레코드(0x32)엔 없다 → 0.
                vertical_align: fields.get(19).copied().unwrap_or(0),
            },
        );
    }
    Ok(geometry)
}

fn parse_tables(
    file: &mut File,
    paragraphs: &[RawParagraph],
    line_base: u64,
    total_lines: u64,
    sections: &[SectionRecord],
    inflated: u64,
    cell_record: u64,
) -> Result<ParsedTables, CoreError> {
    let tail = locate_tail(file, inflated, line_base, total_lines, cell_record)?;
    let geometry = parse_font_geometry(file, tail.font_table, cell_record)?;
    let mut raw_tables: Vec<(usize, Vec<Vec<RawCell>>, SourceRange)> = Vec::new();
    for (part, paragraph) in paragraphs.iter().enumerate() {
        let mut rows: Vec<Vec<RawCell>> = Vec::new();
        let mut row = Vec::new();
        let mut pending: Vec<(u64, SourceRange, Vec<u8>)> = Vec::new();
        let mut table_start = None;
        let mut table_end = None;
        for (offset, range) in paragraph.line_ranges.iter().enumerate() {
            let line = paragraph.first_line + offset as u64;
            if line >= total_lines {
                break;
            }
            let raw = read_range(file, range.start_byte, range.end_byte - range.start_byte)?;
            let cell_number = read_u32(file, line_base + 12 * line + 4)?;
            if raw == b"\x12" && cell_number != 0 {
                if !pending.is_empty() || row.is_empty() {
                    return Err(inspect_error(
                        ErrorCode::GeometryInvalid,
                        "table_row",
                        range.start_byte,
                    ));
                }
                rows.push(std::mem::take(&mut row));
                table_end = Some(range.end_byte);
            } else if raw.ends_with(b"\x13") && cell_number != 0 {
                let cell_geometry = geometry.get(&cell_number).copied().ok_or_else(|| {
                    inspect_error(ErrorCode::GeometryInvalid, "table_cell", range.start_byte)
                })?;
                let mut line_indices: Vec<u64> = pending.iter().map(|value| value.0).collect();
                line_indices.push(line);
                let source_start = pending
                    .first()
                    .map_or(range.start_byte, |value| value.1.start_byte);
                row.push(RawCell {
                    start: cell_geometry.start,
                    width: cell_geometry.width,
                    vertical_align: cell_geometry.vertical_align,
                    rowspan: 1,
                    continuation: cell_geometry.continuation,
                    line_indices,
                    source_range: SourceRange {
                        start_byte: source_start,
                        end_byte: range.end_byte,
                    },
                });
                pending.clear();
                table_start.get_or_insert(source_start);
            } else if cell_number != 0 && !raw.is_empty() {
                pending.push((line, *range, raw));
            } else if !rows.is_empty() {
                break;
            }
        }
        // 마지막 행이 비었다고 버리지 않는다. 통특기 저장본의 진짜 센티널 행은
        // tail 셀 테이블에만 있고(줄 번호 0xFFFFFFFF, 높이 0, start 가 1080 만큼 이동)
        // 본문 줄을 만들지 않으므로 이 루프까지 오지 않는다(예: PCT 저장본 9행째).
        if !rows.is_empty() {
            let mut logical_rows: Vec<Vec<RawCell>> = Vec::with_capacity(rows.len());
            for raw_row in rows {
                let mut logical_row = Vec::new();
                for continuation in raw_row {
                    if continuation.continuation {
                        let parent = logical_rows
                            .iter_mut()
                            .rev()
                            .flat_map(|previous| previous.iter_mut())
                            .find(|candidate| {
                                candidate.start == continuation.start
                                    && candidate.width == continuation.width
                            })
                            .ok_or_else(|| {
                                inspect_error(
                                    ErrorCode::GeometryInvalid,
                                    "table_rowspan",
                                    continuation.source_range.start_byte,
                                )
                            })?;
                        parent.rowspan += 1;
                        parent.line_indices.extend(continuation.line_indices);
                        parent.source_range.end_byte = continuation.source_range.end_byte;
                    } else {
                        logical_row.push(continuation);
                    }
                }
                logical_rows.push(logical_row);
            }
            raw_tables.push((
                part,
                logical_rows,
                SourceRange {
                    start_byte: table_start.unwrap_or(paragraph.range.start_byte),
                    end_byte: table_end.unwrap_or(paragraph.range.end_byte),
                },
            ));
        }
    }
    let mut tables = Vec::new();
    let mut cells = Vec::new();
    let mut line_to_cell = BTreeMap::new();
    let mut used_anchor_sections = BTreeSet::new();
    for (table_index, (table_part, rows, source_range)) in raw_tables.into_iter().enumerate() {
        let row_count = rows.len() as u32;
        let mut boundaries = BTreeSet::new();
        for row in &rows {
            for cell in row {
                boundaries.insert(cell.start);
                boundaries.insert(cell.start + cell.width);
            }
        }
        let points: Vec<u32> = boundaries.into_iter().collect();
        let positions: BTreeMap<u32, u32> = points
            .iter()
            .enumerate()
            .map(|(index, value)| (*value, index as u32))
            .collect();
        let table_id = format!("table:{table_index}");
        let mut cell_ids = Vec::new();
        for (row_index, row) in rows.into_iter().enumerate() {
            for (occurrence, raw_cell) in row.into_iter().enumerate() {
                let id = format!("cell:{table_index}:{row_index}:{occurrence}");
                let colspan =
                    positions[&(raw_cell.start + raw_cell.width)] - positions[&raw_cell.start];
                for line in &raw_cell.line_indices {
                    line_to_cell.insert(*line, id.clone());
                }
                let line_ids = raw_cell
                    .line_indices
                    .iter()
                    .map(|line| format!("line:{line}"))
                    .collect();
                cell_ids.push(id.clone());
                cells.push(CellRecord {
                    id,
                    table_id: table_id.clone(),
                    row: row_index as u32,
                    occurrence: occurrence as u32,
                    start: raw_cell.start,
                    width: raw_cell.width,
                    rowspan: raw_cell.rowspan,
                    colspan,
                    vertical_align: raw_cell.vertical_align,
                    line_ids,
                    image_ids: Vec::new(),
                    source_range: raw_cell.source_range,
                });
            }
        }
        let anchor = sections
            .iter()
            .filter(|section| section.title.starts_with("【표 "))
            .filter(|section| !used_anchor_sections.contains(&section.index))
            .filter_map(|section| {
                let paragraph_id = section.anchor_paragraph_id.as_deref()?;
                let part = paragraph_id
                    .strip_prefix("paragraph:")?
                    .parse::<usize>()
                    .ok()?;
                (part < table_part).then_some((part, section))
            })
            .max_by_key(|(part, _section)| *part)
            .map(|(_part, section)| section);
        if let Some(section) = anchor {
            used_anchor_sections.insert(section.index);
        }
        tables.push(TableRecord {
            id: table_id,
            occurrence: table_index as u64,
            anchor_section_id: anchor.map(|value| value.id.clone()),
            anchor_paragraph_id: anchor.and_then(|value| value.anchor_paragraph_id.clone()),
            row_count,
            column_count: points.len().saturating_sub(1) as u32,
            cell_ids,
            source_range,
        });
    }
    Ok((tables, cells, line_to_cell))
}

fn parse_char_spans(
    file: &mut File,
    char_base: u64,
    total_lines: u64,
) -> Result<(Vec<CharSpan>, u64), CoreError> {
    let mut spans = Vec::with_capacity(total_lines as usize);
    let mut position = char_base;
    for _ in 0..total_lines {
        let start = position;
        let count = read_u32(file, position)?;
        if count == 0 || count > 100_000 {
            return Err(inspect_error(
                ErrorCode::StructureInvalid,
                "char_span",
                position,
            ));
        }
        let bytes = read_range(file, position + 4, 4 * u64::from(count))?;
        let codes = bytes
            .chunks_exact(4)
            .map(|chunk| u32::from_le_bytes(chunk.try_into().expect("char code")))
            .collect();
        position += 4 + 4 * u64::from(count);
        spans.push(CharSpan {
            source_range: SourceRange {
                start_byte: start,
                end_byte: position,
            },
            codes,
        });
    }
    if read_byte(file, position)? != 0xbf {
        return Err(inspect_error(
            ErrorCode::StructureInvalid,
            "object_table",
            position,
        ));
    }
    Ok((spans, position))
}

/// 그림 객체인가. 레코드 크기가 `384 + payload(+380)` 이고 payload 는 TIFF 다.
/// ⛔ **타입 3 만 보지 마라.** 실물 `domestic_f.HLT` 의 수학식 자리는 **타입 4**이고
/// 레이아웃이 타입 3 과 같다. 100바이트 문자모양으로 치면 그 뒤 객체 색인이 통째로 어긋난다.
pub(crate) fn is_image_object(object_type: u32) -> bool {
    matches!(object_type, 3 | 4)
}

fn parse_object_types(
    file: &mut File,
    table: u64,
    include_identities: bool,
) -> Result<BTreeMap<u32, ObjectInfo>, CoreError> {
    let count = read_u32(file, table + 1)?;
    let mut offset = table + 5;
    let mut types = BTreeMap::new();
    for code in 0..count {
        let object_type = read_u32(file, offset + 40)?;
        let size = if is_image_object(object_type) {
            384 + u64::from(read_u32(file, offset + 380)?)
        } else {
            100
        };
        let payload_sha256 = if is_image_object(object_type) && include_identities {
            // ⛔ 못 푸는 그림에서 죽지 마라. 국내출원 저장본의 1비트 G4 압축 TIFF 는
            // 기하가 읽히지 않는다. 픽셀 경로는 이미 원본 바이트만 보존하고 넘어간다.
            // 여기서 `?` 로 올리면 `inspect --kind images` 가 통째로 떨어진다.
            hash_tiff_identity(file, offset, size).ok()
        } else {
            None
        };
        // +364=높이, +368=너비 — 통특기 렌더가 실제로 쓰는 표시 필드.
        let (display_height, display_width) = if is_image_object(object_type) {
            (read_u32(file, offset + 364)?, read_u32(file, offset + 368)?)
        } else {
            (0, 0)
        };
        // +48=상자 너비, +52=상자 높이. 표시 pt 와 별개 필드라 둘 다 실어야 왕복이 같다.
        let (box_width, box_height) = if is_image_object(object_type) {
            (read_u32(file, offset + 48)?, read_u32(file, offset + 52)?)
        } else {
            (0, 0)
        };
        types.insert(
            code,
            ObjectInfo {
                object_type,
                payload_sha256,
                display_width,
                display_height,
                box_width,
                box_height,
            },
        );
        offset += size;
    }
    Ok(types)
}

/// 객체 테이블을 걸으며 이미지 객체의 TIFF 픽셀을 읽는다.
fn parse_image_payloads(
    file: &mut File,
    table: u64,
) -> Result<BTreeMap<u32, HltImagePayload>, CoreError> {
    let count = read_u32(file, table + 1)?;
    let mut offset = table + 5;
    let mut payloads = BTreeMap::new();
    for code in 0..count {
        let object_type = read_u32(file, offset + 40)?;
        let size = if is_image_object(object_type) {
            384 + u64::from(read_u32(file, offset + 380)?)
        } else {
            100
        };
        if is_image_object(object_type) {
            payloads.insert(code, read_tiff_payload(file, code, offset, size)?);
        }
        offset += size;
    }
    Ok(payloads)
}

fn read_tiff_payload(
    file: &mut File,
    code: u32,
    object_offset: u64,
    object_size: u64,
) -> Result<HltImagePayload, CoreError> {
    let tiff = object_offset + 384;
    let raw = read_range(file, tiff, object_size - 384)?;
    // 무압축 그레이/RGB 는 픽셀까지 들고 온다. 국내출원 저장본의 1비트 G4 압축 TIFF 처럼
    // 우리가 아직 못 푸는 것은 원본 바이트만 보존한다(제출 세트는 그대로 복사하면 된다).
    match tiff_geometry(file, tiff, object_size) {
        Ok(geometry) => {
            let start = usize::try_from(geometry.strip)
                .map_err(|_| inspect_error(ErrorCode::SizeLimitExceeded, "image_tiff", tiff))?;
            let length = usize::try_from(geometry.byte_count)
                .map_err(|_| inspect_error(ErrorCode::SizeLimitExceeded, "image_tiff", tiff))?;
            // 스트립이 페이로드 밖을 가리키면(다중 스트립 TIFF) 픽셀은 포기하고
            // 원본 바이트만 보존한다. 기하 읽기 실패와 같은 취급이다.
            let Some(pixels) = raw.get(start..start + length).map(<[u8]>::to_vec) else {
                return Ok(HltImagePayload {
                    object_code: code,
                    width: 0,
                    height: 0,
                    samples: 0,
                    bits: 0,
                    pixels: Vec::new(),
                    tiff: raw,
                });
            };
            Ok(HltImagePayload {
                object_code: code,
                width: geometry.width,
                height: geometry.height,
                samples: geometry.samples as u32,
                bits: geometry.bits as u32,
                pixels,
                tiff: raw,
            })
        }
        Err(_) => Ok(HltImagePayload {
            object_code: code,
            width: 0,
            height: 0,
            samples: 0,
            bits: 0,
            pixels: Vec::new(),
            tiff: raw,
        }),
    }
}

struct TiffGeometry {
    width: u32,
    height: u32,
    samples: u64,
    bits: u64,
    strip: u64,
    byte_count: u64,
}

fn tiff_geometry(file: &mut File, tiff: u64, object_size: u64) -> Result<TiffGeometry, CoreError> {
    if read_range(file, tiff, 4)? != b"II* " {
        return Err(inspect_error(
            ErrorCode::UnsupportedFormat,
            "image_tiff",
            tiff,
        ));
    }
    let ifd = tiff + u64::from(read_u32(file, tiff + 4)?);
    let entry_count = u64::from(read_u16(file, ifd)?);
    let mut values = BTreeMap::new();
    for index in 0..entry_count {
        let entry = ifd + 2 + 12 * index;
        let tag = read_u16(file, entry)?;
        let value_type = read_u16(file, entry + 2)?;
        let count = read_u32(file, entry + 4)?;
        let raw_value = read_u32(file, entry + 8)?;
        if count == 1 && matches!(value_type, 3 | 4) {
            values.insert(
                tag,
                if value_type == 3 {
                    raw_value & 0xffff
                } else {
                    raw_value
                },
            );
        }
    }
    let width = *values.get(&0x0100).unwrap_or(&0);
    let height = *values.get(&0x0101).unwrap_or(&0);
    let strip = u64::from(*values.get(&0x0111).unwrap_or(&0));
    let byte_count = u64::from(*values.get(&0x0117).unwrap_or(&0));
    let samples = u64::from(*values.get(&0x0115).unwrap_or(&1)).max(1);
    let bits = u64::from(*values.get(&0x0102).unwrap_or(&8)).max(1);
    if width == 0
        || height == 0
        || bits % 8 != 0
        || byte_count != u64::from(width) * u64::from(height) * samples * (bits / 8)
        || 384 + strip + byte_count > object_size
    {
        return Err(inspect_error(
            ErrorCode::GeometryInvalid,
            "image_tiff",
            tiff,
        ));
    }
    Ok(TiffGeometry {
        width,
        height,
        samples,
        bits,
        strip,
        byte_count,
    })
}

fn hash_tiff_identity(
    file: &mut File,
    object_offset: u64,
    object_size: u64,
) -> Result<String, CoreError> {
    let tiff = object_offset + 384;
    if read_range(file, tiff, 4)? != b"II*\x00" {
        return Err(inspect_error(
            ErrorCode::UnsupportedFormat,
            "image_tiff",
            tiff,
        ));
    }
    let ifd = tiff + u64::from(read_u32(file, tiff + 4)?);
    let entry_count = u64::from(read_u16(file, ifd)?);
    let mut values = BTreeMap::new();
    for index in 0..entry_count {
        let entry = ifd + 2 + 12 * index;
        let tag = read_u16(file, entry)?;
        let value_type = read_u16(file, entry + 2)?;
        let count = read_u32(file, entry + 4)?;
        let raw_value = read_u32(file, entry + 8)?;
        if count == 1 && matches!(value_type, 3 | 4) {
            values.insert(
                tag,
                if value_type == 3 {
                    raw_value & 0xffff
                } else {
                    raw_value
                },
            );
        }
    }
    let width = *values.get(&0x0100).unwrap_or(&0);
    let height = *values.get(&0x0101).unwrap_or(&0);
    let strip = u64::from(*values.get(&0x0111).unwrap_or(&0));
    let byte_count = u64::from(*values.get(&0x0117).unwrap_or(&0));
    // 통특기 저장본: 옛 계열은 RGB(SamplesPerPixel=3), PCT 계열은 8비트
    // 그레이스케일(=1)이다. 픽셀 시작도 방언마다 다르므로(RGB 370, 그레이 364)
    // StripOffsets 를 그대로 쓰고 크기는 가로×세로×표본×(비트/8) 로 검산한다.
    let samples = u64::from(*values.get(&0x0115).unwrap_or(&1)).max(1);
    let bits = u64::from(*values.get(&0x0102).unwrap_or(&8)).max(1);
    if width == 0
        || height == 0
        || bits % 8 != 0
        || byte_count != u64::from(width) * u64::from(height) * samples * (bits / 8)
        || 384 + strip + byte_count > object_size
    {
        return Err(inspect_error(
            ErrorCode::GeometryInvalid,
            "image_tiff",
            tiff,
        ));
    }
    let mut hasher = Sha256::new();
    hasher.update(width.to_le_bytes());
    hasher.update(height.to_le_bytes());
    file.seek(SeekFrom::Start(tiff + strip))
        .map_err(|_| inspect_error(ErrorCode::IoError, "image_tiff", tiff + strip))?;
    let mut remaining = byte_count;
    let mut buffer = [0_u8; BUFFER_SIZE];
    while remaining > 0 {
        let want = usize::try_from(remaining.min(BUFFER_SIZE as u64)).expect("bounded image read");
        file.read_exact(&mut buffer[..want])
            .map_err(|_| inspect_error(ErrorCode::IoError, "image_tiff", tiff + strip))?;
        hasher.update(&buffer[..want]);
        remaining -= want as u64;
    }
    Ok(format!("{:X}", hasher.finalize()))
}

fn parse_images(
    file: &mut File,
    lines: &[LineRecord],
    spans: &[CharSpan],
    object_types: &BTreeMap<u32, ObjectInfo>,
    line_to_cell: &BTreeMap<u64, String>,
    cells: &[CellRecord],
) -> Result<Vec<ImageOccurrence>, CoreError> {
    let cells_by_id: BTreeMap<&str, &CellRecord> =
        cells.iter().map(|cell| (cell.id.as_str(), cell)).collect();
    let mut images = Vec::new();
    for line in lines {
        let raw = read_range(
            file,
            line.source_range.start_byte,
            line.source_range.end_byte - line.source_range.start_byte,
        )?;
        let span = spans
            .get(line.index as usize)
            .ok_or_else(|| inspect_error(ErrorCode::StructureInvalid, "char_span", line.index))?;
        let mut position = 0_usize;
        while position < raw.len() {
            let width = if raw[position] >= 0x81 && position + 1 < raw.len() {
                2
            } else {
                1
            };
            if width == 1 && raw[position] == b'_' {
                let code = if span.codes.len() == 1 {
                    span.codes[0]
                } else {
                    *span.codes.get(position).ok_or_else(|| {
                        inspect_error(ErrorCode::StructureInvalid, "char_span", line.index)
                    })?
                };
                if object_types
                    .get(&code)
                    .is_some_and(|info| is_image_object(info.object_type))
                {
                    let object = &object_types[&code];
                    let payload_sha256 = object.payload_sha256.clone();
                    let (display_width, display_height) =
                        (object.display_width, object.display_height);
                    let (box_width, box_height) = (object.box_width, object.box_height);
                    let occurrence = images.len() as u64;
                    let cell_id = line_to_cell.get(&line.index).cloned();
                    let table_id = cell_id
                        .as_deref()
                        .and_then(|id| cells_by_id.get(id).map(|cell| cell.table_id.clone()));
                    images.push(ImageOccurrence {
                        id: format!("image:{occurrence}"),
                        occurrence,
                        object_code: code,
                        payload_sha256,
                        display_width,
                        display_height,
                        box_width,
                        box_height,
                        paragraph_id: line.paragraph_id.clone(),
                        line_id: line.id.clone(),
                        table_id,
                        cell_id,
                        source_range: SourceRange {
                            start_byte: line.source_range.start_byte + position as u64,
                            end_byte: line.source_range.start_byte + position as u64 + 1,
                        },
                    });
                }
            }
            position += width;
        }
    }
    Ok(images)
}

fn find_pattern(
    file: &mut File,
    start: u64,
    end: u64,
    pattern: &[u8],
) -> Result<Option<u64>, CoreError> {
    if pattern.is_empty() || start >= end {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(start))
        .map_err(|_| inspect_error(ErrorCode::IoError, "seek", start))?;
    let mut position = start;
    let mut overlap = Vec::new();
    let mut buffer = [0_u8; BUFFER_SIZE];
    while position < end {
        let want = usize::try_from((end - position).min(BUFFER_SIZE as u64)).expect("bounded read");
        let read = file
            .read(&mut buffer[..want])
            .map_err(|_| inspect_error(ErrorCode::IoError, "read", position))?;
        if read == 0 {
            break;
        }
        let mut window = overlap.clone();
        window.extend_from_slice(&buffer[..read]);
        if let Some(index) = window
            .windows(pattern.len())
            .position(|value| value == pattern)
        {
            return Ok(Some(position - overlap.len() as u64 + index as u64));
        }
        let keep = pattern.len().saturating_sub(1).min(window.len());
        overlap = window[window.len() - keep..].to_vec();
        position += read as u64;
    }
    Ok(None)
}

fn read_range(file: &mut File, offset: u64, length: u64) -> Result<Vec<u8>, CoreError> {
    let length = usize::try_from(length)
        .map_err(|_| inspect_error(ErrorCode::ResourceTooLarge, "read", offset))?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| inspect_error(ErrorCode::IoError, "seek", offset))?;
    let mut bytes = vec![0_u8; length];
    file.read_exact(&mut bytes)
        .map_err(|_| inspect_error(ErrorCode::IoError, "read", offset))?;
    Ok(bytes)
}

fn read_byte(file: &mut File, offset: u64) -> Result<u8, CoreError> {
    Ok(read_range(file, offset, 1)?[0])
}

fn read_u16(file: &mut File, offset: u64) -> Result<u16, CoreError> {
    Ok(u16::from_le_bytes(
        read_range(file, offset, 2)?.try_into().expect("u16"),
    ))
}

fn read_u32(file: &mut File, offset: u64) -> Result<u32, CoreError> {
    Ok(u32::from_le_bytes(
        read_range(file, offset, 4)?.try_into().expect("u32"),
    ))
}

/// 고정 폭 제목 칸(헤더 문서 제목 40B · 절 제목 51B). 옛 통특기는 2바이트 글자 가운데서 잘라 쓰기도 해서
/// (서식 `PctUtil` 헤더 제목 · `SpDelta` 절 21) 끝에 홀로 남은 첫 바이트 하나만 버린다. 그 밖의 깨짐은 그대로 거부.
fn decode_cp949_field(bytes: &[u8], offset: u64) -> Result<String, CoreError> {
    let mut at = 0;
    while at < bytes.len() {
        at += if bytes[at] >= 0x81 { 2 } else { 1 };
    }
    let end = if at > bytes.len() {
        bytes.len() - 1
    } else {
        bytes.len()
    };
    decode_cp949(&bytes[..end], offset)
}

fn decode_cp949(bytes: &[u8], offset: u64) -> Result<String, CoreError> {
    let (text, had_errors) = EUC_KR.decode_without_bom_handling(bytes);
    if had_errors {
        return Err(inspect_error(ErrorCode::Cp949Invalid, "cp949", offset));
    }
    Ok(text.into_owned())
}

fn inspect_error(code: ErrorCode, phase: &str, offset: u64) -> CoreError {
    let mut error = CoreError::new(code, phase, false);
    error.offset = Some(offset);
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{VerifyOptions, codec::inflate_raw_to_writer};

    fn header_with(version: u8, minor: u8) -> Vec<u8> {
        let mut header = vec![0_u8; 0x108];
        header[0x15] = version;
        header[0x16] = minor;
        header
    }

    #[test]
    fn cell_record_size_follows_the_format_version_and_refuses_unproven_ones() {
        assert_eq!(cell_record_size(&header_with(0x33, 0x0b)).unwrap(), 80);
        assert_eq!(cell_record_size(&header_with(0x32, 0x0b)).unwrap(), 76);
        // 0x2b 서식 6개가 맞는다. 그 사이 판(0x2c…0x31)은 아직 실물이 없다.
        assert_eq!(cell_record_size(&header_with(0x2b, 0x0b)).unwrap(), 76);
        for (version, minor) in [(0x2c, 0x0b), (0x34, 0x0b), (0x33, 0x09), (0x32, 0x00)] {
            let error = cell_record_size(&header_with(version, minor)).unwrap_err();
            assert_eq!(
                error.code,
                ErrorCode::UnsupportedFormat,
                "{version:#x}/{minor:#x}"
            );
        }
    }

    #[test]
    fn bounded_preview_rejects_invalid_cp949_after_the_capture_limit() {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"valid-prefix\x80").unwrap();
        let error = read_text_preview(&mut file, 0, 13, 4).unwrap_err();
        assert_eq!(error.code, ErrorCode::Cp949Invalid);
    }

    #[test]
    fn edit_layout_exposes_bounded_body_and_character_span_ranges() {
        let (mut spool, inflated) = inflated_table_fixture();

        let layout = inspect_edit_layout(&mut spool, inflated).unwrap();

        assert!(layout.body_start < layout.body_end);
        assert_eq!(layout.char_base, layout.body_end + CHAR_MARK.len() as u64);
        assert!(layout.char_base < layout.line_base);
        assert_eq!(read_byte(&mut spool, layout.object_table).unwrap(), 0xbf);
        assert_eq!(layout.total_lines, 29);
        assert_eq!(layout.char_spans.len(), 29);
        for span in &layout.char_spans {
            assert_eq!(
                span.source_range.end_byte - span.source_range.start_byte,
                4 + 4 * span.codes.len() as u64
            );
        }
        assert_eq!(layout.char_spans[3].codes, [0]);
    }

    #[test]
    fn edit_layout_rejects_a_broken_cp949_character_code_pair() {
        let (mut spool, inflated) = inflated_table_fixture();
        let layout = inspect_edit_layout(&mut spool, inflated).unwrap();
        let span = &layout.char_spans[4];
        assert!(span.codes.len() > 2);
        assert_eq!(span.codes[0], span.codes[1]);
        spool
            .seek(SeekFrom::Start(span.source_range.start_byte + 8))
            .unwrap();
        spool.write_all(&(span.codes[1] + 1).to_le_bytes()).unwrap();
        spool.flush().unwrap();

        let error = inspect_edit_layout(&mut spool, inflated).unwrap_err();

        assert_eq!(error.code, ErrorCode::StructureInvalid);
        assert_eq!(error.phase, "char_span_invariant");
    }

    /// 통특기는 글자 바이트와 글자모양 코드 수가 어긋난 줄도 연다(글만 바꿔 끼운
    /// 문서도 오류 없이 제출 변환한다 · 그 XML = 바르게 고친 판과 서식까지 같다). 읽기 경로는 그 줄의 코드를
    /// 바이트 수에 맞춘다 — 남는 코드는 버리고, 모자라면 마지막 코드를 잇고, 2바이트 글자는 앞 바이트 코드를 쓴다.
    #[test]
    fn read_layout_fits_codes_to_the_line_bytes_like_nkeditor() {
        let raw = [0xb0_u8, 0xa1, b'a', 0xb0, 0xa1];
        let mut longer = vec![4, 4, 5, 6, 6, 9, 9, 9];
        assert!(fit_codes(&raw, false, &mut longer));
        assert_eq!(longer, [4, 4, 5, 6, 6]);
        let mut shorter = vec![4, 4, 5];
        assert!(fit_codes(&raw, true, &mut shorter));
        assert_eq!(
            shorter,
            [4, 4, 5, 5, 5, 5],
            "모자란 자리는 마지막 코드 · 끝 줄은 종결자 몫 +1"
        );
        let mut split = vec![4, 7, 5, 6, 6];
        assert!(fit_codes(&raw, false, &mut split));
        assert_eq!(split, [4, 4, 5, 6, 6]);
        let mut good = vec![4, 4, 5, 6, 6];
        assert!(!fit_codes(&raw, false, &mut good));
    }

    #[test]
    fn read_layout_accepts_what_the_edit_layout_rejects() {
        let (mut spool, inflated) = inflated_table_fixture();
        let layout = inspect_edit_layout(&mut spool, inflated).unwrap();
        let span = &layout.char_spans[4];
        spool
            .seek(SeekFrom::Start(span.source_range.start_byte + 8))
            .unwrap();
        spool.write_all(&(span.codes[1] + 1).to_le_bytes()).unwrap();
        spool.flush().unwrap();

        assert!(
            inspect_edit_layout(&mut spool, inflated).is_err(),
            "고쳐 쓰는 경로는 엄격"
        );
        let read = inspect_read_layout(&mut spool, inflated).unwrap();
        assert_eq!(read.char_spans[4].codes[1], read.char_spans[4].codes[0]);
    }

    fn inflated_table_fixture() -> (File, u64) {
        let encoded = include_str!("../../../fixtures/phase2/pct_table_probe.HLT.hex");
        let digits: Vec<u8> = encoded
            .bytes()
            .filter(|byte| !byte.is_ascii_whitespace())
            .collect();
        let bytes: Vec<u8> = digits
            .chunks_exact(2)
            .map(|pair| {
                let text = std::str::from_utf8(pair).unwrap();
                u8::from_str_radix(text, 16).unwrap()
            })
            .collect();
        let mut input = tempfile::tempfile().unwrap();
        input.write_all(&bytes).unwrap();
        input.seek(SeekFrom::Start(0)).unwrap();
        let layout = read_layout(&mut input, "pct_table_probe.HLT").unwrap();
        let mut spool = tempfile::tempfile().unwrap();
        let summary =
            inflate_raw_to_writer(&mut input, &layout, &mut spool, &VerifyOptions::default())
                .unwrap();
        spool.flush().unwrap();
        (spool, summary.inflated)
    }
}
