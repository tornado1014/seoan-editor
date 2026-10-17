use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use encoding_rs::EUC_KR;

use crate::{
    CellCharacterFormatSpan, CellFormatInventory, CellFormatRecord, CellRecord, CharacterFormat,
    CharacterFormatSpan, CharacterSizeRun, CoreError, ErrorCode, FormatInventory, FormatParagraph,
    LineRecord, UnresolvedCellFormat, UnresolvedFormat, VerifyOptions, error::safe_source,
    inspect::inspect_read_layout, save::open_inflated_source,
};

/// 통특기가 크기 미지정 런에 기록하는 기본 글자 크기(포인트).
const DEFAULT_POINT_SIZE: u32 = 12;

pub const KNOWN_CHARACTER_FORMAT_MASK: u32 = 1 | 2 | 4 | 16 | 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeObjectKind {
    CharacterShape,
    Image,
}

#[derive(Debug, Clone, Copy)]
struct RuntimeObject {
    kind: RuntimeObjectKind,
    attr: Option<u32>,
    /// 문자모양 레코드 `+4`. 포인트 단위 글자 크기다.
    size: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
struct CharacterAtom {
    index: u32,
    code: Option<u32>,
    attr: Option<u32>,
    size: Option<u32>,
    image: bool,
    verified: bool,
}

pub fn inspect_format_inventory(path: &Path) -> Result<FormatInventory, CoreError> {
    let mut source = open_inflated_source(path, &VerifyOptions::default())?;
    Ok(inspect_format_state_from_source(&mut source)?.inventory)
}

pub fn inspect_cell_format_inventory(path: &Path) -> Result<CellFormatInventory, CoreError> {
    Ok(inspect_format_inventories(path)?.1)
}

/// 문단 서식과 셀 서식을 한 번의 해제, 한 번의 판독으로 같이 낸다.
/// 내보내기는 둘 다 쓰는데 따로 부르면 16MB 짜리 실물을 두 번 푼다.
pub fn inspect_format_inventories(
    path: &Path,
) -> Result<(FormatInventory, CellFormatInventory), CoreError> {
    let mut source = open_inflated_source(path, &VerifyOptions::default())?;
    let cells = source.document.cells.clone();
    let state = inspect_format_state_from_source(&mut source)?;
    let cell_inventory = build_cell_format_inventory(&cells, &state);
    Ok((state.inventory, cell_inventory))
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeFormatState {
    pub(crate) inventory: FormatInventory,
    pub(crate) characters: BTreeMap<String, Vec<RuntimeCharacter>>,
    pub(crate) line_text: BTreeMap<String, String>,
    pub(crate) line_characters: BTreeMap<String, Vec<RuntimeCharacter>>,
    pub(crate) style_codes: BTreeMap<u32, u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimeCharacter {
    pub(crate) code: Option<u32>,
    pub(crate) attr: Option<u32>,
    pub(crate) size: Option<u32>,
    pub(crate) image: bool,
    pub(crate) verified: bool,
}

pub(crate) fn inspect_format_state_from_source(
    source: &mut crate::save::InflatedSource,
) -> Result<RuntimeFormatState, CoreError> {
    let source_name = source.source_name.clone();
    let source_sha256 = source.verification.input_sha256.clone();
    let inflated = source.document.summary.inflated_bytes;
    let paragraphs = source.document.paragraphs.clone();
    let lines: BTreeMap<String, LineRecord> = source
        .document
        .lines
        .iter()
        .cloned()
        .map(|line| (line.id.clone(), line))
        .collect();
    // 읽기 경로 — 통특기처럼 글자/코드 수가 어긋난 줄도 읽는다(제출파일 · XML · 편집기 열기).
    let layout = inspect_read_layout(&mut source.spool, inflated)?;
    let objects = parse_runtime_objects(
        &mut source.spool,
        layout.object_table,
        inflated,
        &source_name,
    )?;
    let mut style_codes = BTreeMap::new();
    for (code, object) in objects.iter().enumerate() {
        if object.kind == RuntimeObjectKind::CharacterShape {
            let attr = object.attr.unwrap_or(0);
            if attr & !KNOWN_CHARACTER_FORMAT_MASK == 0 {
                style_codes.entry(attr).or_insert(code as u32);
            }
        }
    }

    let mut output_paragraphs = Vec::with_capacity(paragraphs.len());
    let mut unresolved = Vec::new();
    let mut characters = BTreeMap::new();
    let mut line_text = BTreeMap::new();
    let mut line_characters = BTreeMap::new();
    for paragraph in paragraphs {
        let mut text = Vec::new();
        let mut atoms = Vec::new();
        for line_id in &paragraph.line_ids {
            let line_text_start = text.len();
            let line_atom_start = atoms.len();
            let line = lines.get(line_id).ok_or_else(|| {
                format_error(
                    ErrorCode::StructureInvalid,
                    "format_line",
                    &source_name,
                    None,
                )
            })?;
            let raw = read_range(
                &mut source.spool,
                line.source_range.start_byte,
                line.source_range.end_byte - line.source_range.start_byte,
                &source_name,
            )?;
            let span = layout.char_spans.get(line.index as usize).ok_or_else(|| {
                format_error(
                    ErrorCode::StructureInvalid,
                    "format_char_span",
                    &source_name,
                    Some(line.index),
                )
            })?;
            let mut byte_index = 0_usize;
            while byte_index < raw.len() {
                let width = if raw[byte_index] >= 0x81 { 2 } else { 1 };
                if byte_index + width > raw.len() {
                    return Err(format_error(
                        ErrorCode::Cp949Invalid,
                        "format_cp949",
                        &source_name,
                        Some(line.source_range.start_byte + byte_index as u64),
                    ));
                }
                let decoded = EUC_KR
                    .decode_without_bom_handling_and_without_replacement(
                        &raw[byte_index..byte_index + width],
                    )
                    .ok_or_else(|| {
                        format_error(
                            ErrorCode::Cp949Invalid,
                            "format_cp949",
                            &source_name,
                            Some(line.source_range.start_byte + byte_index as u64),
                        )
                    })?;
                let mut decoded_chars = decoded.chars();
                let character = decoded_chars.next().ok_or_else(|| {
                    format_error(
                        ErrorCode::Cp949Invalid,
                        "format_cp949",
                        &source_name,
                        Some(line.source_range.start_byte + byte_index as u64),
                    )
                })?;
                if decoded_chars.next().is_some() {
                    return Err(format_error(
                        ErrorCode::Cp949Invalid,
                        "format_cp949",
                        &source_name,
                        Some(line.source_range.start_byte + byte_index as u64),
                    ));
                }

                let raw_code = if span.codes.len() == 1 {
                    Some(span.codes[0])
                } else {
                    span.codes.get(byte_index).copied()
                };
                let pair_matches = width == 1
                    || span.codes.len() == 1
                    || span.codes.get(byte_index + 1) == raw_code.as_ref();

                // 탭은 문단 글자(`paragraph.text`)에 남으므로 좌표에도 센다 — 빼면 탭 뒤 서식이 탭 수만큼
                // 앞으로 밀린다(예: 약어 표 「BINAP\t\t(<i>S</i>)」).
                if character >= '\u{20}' || character == '\t' {
                    let character_index = u32::try_from(text.len()).map_err(|_| {
                        format_error(
                            ErrorCode::ResourceTooLarge,
                            "format_character_index",
                            &source_name,
                            Some(line.source_range.start_byte + byte_index as u64),
                        )
                    })?;
                    text.push(character);
                    let Some(code) = raw_code else {
                        unresolved.push(unresolved_item(
                            &paragraph.id,
                            line_id,
                            Some(character_index),
                            None,
                            None,
                            "character_code_absent",
                        ));
                        atoms.push(CharacterAtom {
                            index: character_index,
                            code: None,
                            attr: None,
                            size: None,
                            image: false,
                            verified: false,
                        });
                        byte_index += width;
                        continue;
                    };
                    if !pair_matches {
                        unresolved.push(unresolved_item(
                            &paragraph.id,
                            line_id,
                            Some(character_index),
                            Some(code),
                            None,
                            "cp949_character_code_pair_mismatch",
                        ));
                        atoms.push(CharacterAtom {
                            index: character_index,
                            code: Some(code),
                            attr: None,
                            size: None,
                            image: false,
                            verified: false,
                        });
                        byte_index += width;
                        continue;
                    }
                    match objects.get(code as usize) {
                        None => {
                            unresolved.push(unresolved_item(
                                &paragraph.id,
                                line_id,
                                Some(character_index),
                                Some(code),
                                None,
                                "character_shape_slot_absent",
                            ));
                            atoms.push(CharacterAtom {
                                index: character_index,
                                code: Some(code),
                                attr: None,
                                size: None,
                                image: false,
                                verified: false,
                            });
                        }
                        Some(object) if object.kind == RuntimeObjectKind::Image => {
                            if character != '_' {
                                unresolved.push(unresolved_item(
                                    &paragraph.id,
                                    line_id,
                                    Some(character_index),
                                    Some(code),
                                    None,
                                    "text_uses_image_slot",
                                ));
                            }
                            atoms.push(CharacterAtom {
                                index: character_index,
                                code: Some(code),
                                attr: None,
                                size: None,
                                image: true,
                                verified: character == '_',
                            });
                        }
                        Some(object) => {
                            let attr = object.attr.unwrap_or(0);
                            let unknown = attr & !KNOWN_CHARACTER_FORMAT_MASK;
                            if unknown != 0 {
                                unresolved.push(unresolved_item(
                                    &paragraph.id,
                                    line_id,
                                    Some(character_index),
                                    Some(code),
                                    Some(attr),
                                    "unknown_character_attribute_bits",
                                ));
                            }
                            atoms.push(CharacterAtom {
                                index: character_index,
                                code: Some(code),
                                attr: Some(attr),
                                size: object.size,
                                image: false,
                                verified: unknown == 0,
                            });
                        }
                    }
                }
                byte_index += width;
            }
            line_text.insert(line_id.clone(), text[line_text_start..].iter().collect());
            line_characters.insert(
                line_id.clone(),
                atoms[line_atom_start..]
                    .iter()
                    .map(runtime_character)
                    .collect(),
            );
        }

        let spans = build_spans(&paragraph.id, &text, &atoms);
        characters.insert(
            paragraph.id.clone(),
            atoms.iter().map(runtime_character).collect(),
        );
        let mut per_char = vec![None; text.len()];
        for atom in &atoms {
            if let Some(slot) = per_char.get_mut(atom.index as usize) {
                *slot = atom.size;
            }
        }
        let sizes = build_size_runs(&text, &per_char);
        output_paragraphs.push(FormatParagraph {
            paragraph_id: paragraph.id,
            text: text.into_iter().collect(),
            spans,
            sizes,
        });
    }

    Ok(RuntimeFormatState {
        inventory: FormatInventory {
            source: source_name,
            source_sha256,
            paragraphs: output_paragraphs,
            unresolved,
        },
        characters,
        line_text,
        line_characters,
        style_codes,
    })
}

fn runtime_character(atom: &CharacterAtom) -> RuntimeCharacter {
    RuntimeCharacter {
        code: atom.code,
        attr: atom.attr,
        size: atom.size,
        image: atom.image,
        verified: atom.verified,
    }
}

pub(crate) fn build_cell_format_inventory(
    cells: &[CellRecord],
    state: &RuntimeFormatState,
) -> CellFormatInventory {
    let mut line_owners: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for cell in cells {
        for line_id in &cell.line_ids {
            line_owners
                .entry(line_id.as_str())
                .or_default()
                .push(cell.id.as_str());
        }
    }
    let mut unresolved = Vec::new();
    for item in &state.inventory.unresolved {
        if let Some(owners) = line_owners.get(item.line_id.as_str()) {
            for cell_id in owners {
                unresolved.push(UnresolvedCellFormat {
                    cell_id: (*cell_id).to_string(),
                    line_id: item.line_id.clone(),
                    character_index: None,
                    raw_code: item.raw_code,
                    raw_attr: item.raw_attr,
                    reason: item.reason.clone(),
                });
            }
        }
    }

    let mut output_cells = Vec::with_capacity(cells.len());
    for cell in cells {
        let mut text = Vec::new();
        let mut characters = Vec::new();
        for line_id in &cell.line_ids {
            let owner_count = line_owners.get(line_id.as_str()).map_or(0, Vec::len);
            if owner_count != 1 {
                unresolved.push(cell_unresolved(
                    &cell.id,
                    line_id,
                    "line_belongs_to_multiple_cells",
                ));
                continue;
            }
            let (Some(line_text), Some(line_characters)) = (
                state.line_text.get(line_id),
                state.line_characters.get(line_id),
            ) else {
                unresolved.push(cell_unresolved(
                    &cell.id,
                    line_id,
                    "cell_line_format_state_absent",
                ));
                continue;
            };
            let line_text: Vec<char> = line_text.chars().collect();
            if line_text.len() != line_characters.len() {
                unresolved.push(cell_unresolved(
                    &cell.id,
                    line_id,
                    "cell_line_character_count_mismatch",
                ));
                continue;
            }
            text.extend(line_text);
            characters.extend_from_slice(line_characters);
        }
        let spans = build_cell_spans(&cell.id, &text, &characters);
        let per_char: Vec<Option<u32>> = characters.iter().map(|value| value.size).collect();
        let sizes = build_size_runs(&text, &per_char);
        output_cells.push(CellFormatRecord {
            cell_id: cell.id.clone(),
            table_id: cell.table_id.clone(),
            text: text.into_iter().collect(),
            spans,
            sizes,
        });
    }

    CellFormatInventory {
        source: state.inventory.source.clone(),
        source_sha256: state.inventory.source_sha256.clone(),
        cells: output_cells,
        unresolved,
    }
}

fn build_cell_spans(
    cell_id: &str,
    text: &[char],
    characters: &[RuntimeCharacter],
) -> Vec<CellCharacterFormatSpan> {
    let mut spans = Vec::new();
    let mut start = 0_usize;
    while start < characters.len() {
        let character = characters[start];
        let (Some(code), Some(attr)) = (character.code, character.attr.filter(|value| *value != 0))
        else {
            start += 1;
            continue;
        };
        if !character.verified || character.image {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < characters.len()
            && characters[end].code == Some(code)
            && characters[end].attr == Some(attr)
            && characters[end].verified
            && !characters[end].image
        {
            end += 1;
        }
        spans.push(CellCharacterFormatSpan {
            cell_id: cell_id.to_string(),
            start: start as u32,
            end: end as u32,
            text: text[start..end].iter().collect(),
            raw_code: code,
            raw_attr: attr,
            formats: formats_from_attr(attr),
        });
        start = end;
    }
    spans
}

fn cell_unresolved(cell_id: &str, line_id: &str, reason: &str) -> UnresolvedCellFormat {
    UnresolvedCellFormat {
        cell_id: cell_id.to_string(),
        line_id: line_id.to_string(),
        character_index: None,
        raw_code: None,
        raw_attr: None,
        reason: reason.to_string(),
    }
}

fn parse_runtime_objects(
    file: &mut File,
    table: u64,
    inflated: u64,
    source: &str,
) -> Result<Vec<RuntimeObject>, CoreError> {
    if read_byte(file, table, source)? != 0xbf {
        return Err(format_error(
            ErrorCode::StructureInvalid,
            "format_object_table",
            source,
            Some(table),
        ));
    }
    let count = read_u32(file, table + 1, source)?;
    if count == 0 || count >= 1_000_000 {
        return Err(format_error(
            ErrorCode::StructureInvalid,
            "format_object_count",
            source,
            Some(table + 1),
        ));
    }
    let mut objects = Vec::with_capacity(count as usize);
    let mut offset = table.checked_add(5).ok_or_else(|| {
        format_error(
            ErrorCode::StructureInvalid,
            "format_object_table",
            source,
            Some(table),
        )
    })?;
    for _code in 0..count {
        require_range(offset, 100, inflated, source, "format_object_slot")?;
        let object_type = read_u32(file, offset + 40, source)?;
        if crate::inspect::is_image_object(object_type) {
            require_range(offset, 384, inflated, source, "format_image_slot")?;
            let payload = u64::from(read_u32(file, offset + 380, source)?);
            let size = 384_u64.checked_add(payload).ok_or_else(|| {
                format_error(
                    ErrorCode::StructureInvalid,
                    "format_image_slot",
                    source,
                    Some(offset),
                )
            })?;
            require_range(offset, size, inflated, source, "format_image_slot")?;
            objects.push(RuntimeObject {
                kind: RuntimeObjectKind::Image,
                attr: None,
                size: None,
            });
            offset += size;
        } else {
            let attr = read_u32(file, offset, source)?;
            // `+4` 가 포인트 크기다. PCT 저장본: 표 셀 10, 본문 12.
            // 같은 굵게(attr=2)가 두 자리에서 12 와 10 이라 서식으로는 정해지지 않는다.
            let size = read_u32(file, offset + 4, source)?;
            objects.push(RuntimeObject {
                kind: RuntimeObjectKind::CharacterShape,
                attr: Some(attr),
                size: (size != 0).then_some(size),
            });
            offset += 100;
        }
    }
    Ok(objects)
}

/// 글자 크기가 같은 이어진 구간을 만든다. 문단 글자를 빈틈없이 덮는다.
/// 크기를 모르는 글자(그림 자리표, 미해결)는 **바로 앞 구간의 크기를 잇는다.**
/// 문단이 통째로 크기를 모르면 방언 기본값 12pt 로 둔다.
fn build_size_runs(text: &[char], sizes: &[Option<u32>]) -> Vec<CharacterSizeRun> {
    if text.is_empty() {
        return Vec::new();
    }
    let first = sizes
        .iter()
        .flatten()
        .next()
        .copied()
        .unwrap_or(DEFAULT_POINT_SIZE);
    let mut runs: Vec<CharacterSizeRun> = Vec::new();
    let mut current = first;
    let mut start = 0_u32;
    for index in 0..text.len() as u32 {
        let size = sizes
            .get(index as usize)
            .copied()
            .flatten()
            .unwrap_or(current);
        if size != current {
            runs.push(CharacterSizeRun {
                start,
                end: index,
                size: current,
            });
            current = size;
            start = index;
        }
    }
    runs.push(CharacterSizeRun {
        start,
        end: text.len() as u32,
        size: current,
    });
    runs
}

fn build_spans(
    paragraph_id: &str,
    text: &[char],
    atoms: &[CharacterAtom],
) -> Vec<CharacterFormatSpan> {
    let mut spans = Vec::new();
    let mut start = 0_usize;
    while start < atoms.len() {
        let atom = atoms[start];
        let (Some(code), Some(attr)) = (atom.code, atom.attr.filter(|value| *value != 0)) else {
            start += 1;
            continue;
        };
        if !atom.verified || atom.image {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < atoms.len()
            && atoms[end].attr == Some(attr)
            && atoms[end].code == Some(code)
            && atoms[end].verified
            && !atoms[end].image
            && atoms[end].index == atoms[end - 1].index + 1
        {
            end += 1;
        }
        let range_start = atom.index;
        let range_end = atoms[end - 1].index + 1;
        spans.push(CharacterFormatSpan {
            paragraph_id: paragraph_id.to_string(),
            start: range_start,
            end: range_end,
            text: text[range_start as usize..range_end as usize]
                .iter()
                .collect(),
            raw_code: code,
            raw_attr: attr,
            formats: formats_from_attr(attr),
        });
        start = end;
    }
    spans
}

fn formats_from_attr(attr: u32) -> Vec<CharacterFormat> {
    [
        (1, CharacterFormat::Underline),
        (2, CharacterFormat::Bold),
        (4, CharacterFormat::Italic),
        (16, CharacterFormat::Superscript),
        (32, CharacterFormat::Subscript),
    ]
    .into_iter()
    .filter_map(|(bit, format)| (attr & bit != 0).then_some(format))
    .collect()
}

fn unresolved_item(
    paragraph_id: &str,
    line_id: &str,
    character_index: Option<u32>,
    raw_code: Option<u32>,
    raw_attr: Option<u32>,
    reason: &str,
) -> UnresolvedFormat {
    UnresolvedFormat {
        paragraph_id: paragraph_id.to_string(),
        line_id: line_id.to_string(),
        character_index,
        raw_code,
        raw_attr,
        reason: reason.to_string(),
    }
}

fn require_range(
    start: u64,
    length: u64,
    inflated: u64,
    source: &str,
    phase: &str,
) -> Result<(), CoreError> {
    if start.checked_add(length).is_none_or(|end| end > inflated) {
        Err(format_error(
            ErrorCode::StructureInvalid,
            phase,
            source,
            Some(start),
        ))
    } else {
        Ok(())
    }
}

fn read_range(
    file: &mut File,
    start: u64,
    length: u64,
    source: &str,
) -> Result<Vec<u8>, CoreError> {
    let length = usize::try_from(length).map_err(|_| {
        format_error(
            ErrorCode::ResourceTooLarge,
            "format_read",
            source,
            Some(start),
        )
    })?;
    file.seek(SeekFrom::Start(start))
        .map_err(|_| format_error(ErrorCode::IoError, "format_read", source, Some(start)))?;
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes)
        .map_err(|_| format_error(ErrorCode::IoError, "format_read", source, Some(start)))?;
    Ok(bytes)
}

fn read_byte(file: &mut File, offset: u64, source: &str) -> Result<u8, CoreError> {
    Ok(read_range(file, offset, 1, source)?[0])
}

fn read_u32(file: &mut File, offset: u64, source: &str) -> Result<u32, CoreError> {
    Ok(u32::from_le_bytes(
        read_range(file, offset, 4, source)?
            .try_into()
            .expect("four-byte read"),
    ))
}

fn format_error(code: ErrorCode, phase: &str, source: &str, offset: Option<u64>) -> CoreError {
    let mut error = CoreError::new(code, phase, false);
    error.source = safe_source(source);
    error.offset = offset;
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Seek, SeekFrom, Write};

    /// 객체 레코드 하나를 만든다. `object_type` 은 +40, attr 은 +0 이다.
    fn object(object_type: u32, attr: u32, payload: &[u8]) -> Vec<u8> {
        let mut record = vec![0_u8; if payload.is_empty() { 100 } else { 384 }];
        record[..4].copy_from_slice(&attr.to_le_bytes());
        record[40..44].copy_from_slice(&object_type.to_le_bytes());
        if !payload.is_empty() {
            let length = payload.len() as u32;
            record[380..384].copy_from_slice(&length.to_le_bytes());
            record.extend_from_slice(payload);
        }
        record
    }

    /// 국내 저장본: 수학식 자리의 객체가 **타입 4** 이고
    /// 타입 3 과 같은 `384 + payload` 레이아웃에 TIFF(`II*\0`)를 담는다.
    /// 타입 4 를 100바이트 문자모양으로 치면 그 뒤 객체 색인이 통째로 어긋난다.
    #[test]
    fn a_type_four_object_is_an_image_sized_like_a_type_three() {
        let mut bytes = vec![0xbf_u8];
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend(object(0, 2, &[]));
        bytes.extend(object(4, 128, b"II*\0filler"));
        bytes.extend(object(0, 4, &[]));

        let mut file = tempfile::tempfile().unwrap();
        file.write_all(&bytes).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        let inflated = bytes.len() as u64;
        let objects = parse_runtime_objects(&mut file, 0, inflated, "probe.HLT").unwrap();

        assert_eq!(objects.len(), 3);
        assert_eq!(objects[0].kind, RuntimeObjectKind::CharacterShape);
        assert_eq!(objects[0].attr, Some(2));
        assert_eq!(objects[1].kind, RuntimeObjectKind::Image);
        // 세 번째가 제자리에서 읽히면 타입 4 의 크기를 맞게 걸었다는 뜻이다.
        assert_eq!(objects[2].kind, RuntimeObjectKind::CharacterShape);
        assert_eq!(objects[2].attr, Some(4));
    }
}
