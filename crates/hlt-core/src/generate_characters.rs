use crate::{
    CharacterFormat, CoreError, ErrorCode, GeneratedLine, GeneratedLineEnding, HltBodyPlan,
};
use encoding_rs::EUC_KR;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const BUILTIN_OBJECT_COUNT: u32 = 24;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedFormatCode {
    pub formats: Vec<CharacterFormat>,
    /// 원본 런이 명시한 글자 크기(포인트). 없으면 방언 기본값 12pt 다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u32>,
    pub code: u32,
    pub attr: u32,
    pub font: u32,
}

impl GeneratedFormatCode {
    pub fn new(
        formats: Vec<CharacterFormat>,
        size: Option<u32>,
        code: u32,
        attr: u32,
        font: u32,
    ) -> Self {
        Self {
            formats,
            size,
            code,
            attr,
            font,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedCharacterPlan {
    pub spans: Vec<u8>,
    pub object_table: Vec<u8>,
    pub object_count: u32,
    pub format_codes: Vec<GeneratedFormatCode>,
}

pub fn plan_hlt_characters(body: &HltBodyPlan) -> Result<GeneratedCharacterPlan, CoreError> {
    let format_codes = allocate_format_codes(body)?;
    let object_table = builtin_palette(&format_codes)?;
    let object_count = u32::from_le_bytes(
        object_table[1..5]
            .try_into()
            .map_err(|_| character_error("palette_count"))?,
    );
    let mut plan = GeneratedCharacterPlan {
        spans: Vec::new(),
        object_table,
        object_count,
        format_codes,
    };
    plan.spans = generate_hlt_character_spans(body, &plan, &BTreeMap::new())?;
    Ok(plan)
}

pub fn generate_hlt_character_spans(
    body: &HltBodyPlan,
    plan: &GeneratedCharacterPlan,
    image_codes: &BTreeMap<String, u32>,
) -> Result<Vec<u8>, CoreError> {
    let codes: BTreeMap<_, _> = plan
        .format_codes
        .iter()
        .map(|entry| ((entry.formats.clone(), entry.size), entry.code))
        .collect();
    let mut used_image_codes = BTreeSet::new();
    for code in image_codes.values() {
        if *code < plan.object_count || !used_image_codes.insert(*code) {
            return Err(character_error("image_code_collision"));
        }
    }
    let mut anchors: BTreeMap<u32, BTreeMap<u32, &str>> = BTreeMap::new();
    for anchor in &body.images {
        let line = anchors.entry(anchor.line_index).or_default();
        if line
            .insert(anchor.character_index, anchor.image_id.as_str())
            .is_some()
        {
            return Err(character_error("duplicate_image_anchor"));
        }
    }

    let mut out = Vec::new();
    for (line_index, line) in body.lines.iter().enumerate() {
        let (mut byte_codes, character_offsets) = line_byte_codes(line, &codes)?;
        if let Some(line_anchors) = anchors.get(&(line_index as u32)) {
            let chars: Vec<_> = line.text.chars().collect();
            for (character_index, image_id) in line_anchors {
                let index = usize::try_from(*character_index)
                    .map_err(|_| character_error("image_character_overflow"))?;
                if chars.get(index) != Some(&'_') {
                    return Err(character_error("image_anchor_not_placeholder"));
                }
                let Some(code) = image_codes.get(*image_id) else {
                    continue;
                };
                let byte_offset = *character_offsets
                    .get(index)
                    .ok_or_else(|| character_error("image_character_range"))?;
                byte_codes[byte_offset] = *code;
            }
        }
        if matches!(
            line.ending,
            GeneratedLineEnding::Paragraph | GeneratedLineEnding::Cell
        ) {
            byte_codes.push(0);
        }
        if byte_codes.is_empty() {
            byte_codes.push(0);
        }
        let force_expanded = line.ending == GeneratedLineEnding::Cell;
        let uniform = byte_codes.iter().all(|code| *code == byte_codes[0]);
        if !force_expanded && uniform {
            push_word(&mut out, 1);
            push_word(&mut out, byte_codes[0]);
        } else {
            push_word(
                &mut out,
                u32::try_from(byte_codes.len())
                    .map_err(|_| character_error("span_length_overflow"))?,
            );
            for code in byte_codes {
                push_word(&mut out, code);
            }
        }
    }
    Ok(out)
}

fn allocate_format_codes(body: &HltBodyPlan) -> Result<Vec<GeneratedFormatCode>, CoreError> {
    let mut assigned = vec![
        entry(vec![CharacterFormat::Bold], None, 2),
        entry(vec![CharacterFormat::Italic], None, 3),
        entry(vec![CharacterFormat::Underline], None, 4),
        entry(vec![CharacterFormat::Subscript], None, 5),
        entry(vec![CharacterFormat::Superscript], None, 6),
    ];
    let mut known: BTreeSet<(Vec<CharacterFormat>, Option<u32>)> = assigned
        .iter()
        .map(|entry| (entry.formats.clone(), entry.size))
        .collect();
    let mut next = 7_u32;
    for line in &body.lines {
        validate_spans(line)?;
        for span in &line.formats {
            let key = (canonical_formats(&span.formats, span.size)?, span.size);
            if known.insert(key.clone()) {
                assigned.push(entry(key.0, key.1, next));
                next = next
                    .checked_add(1)
                    .ok_or_else(|| character_error("format_code_overflow"))?;
            }
        }
    }
    Ok(assigned)
}

fn line_byte_codes(
    line: &GeneratedLine,
    codes: &BTreeMap<(Vec<CharacterFormat>, Option<u32>), u32>,
) -> Result<(Vec<u32>, Vec<usize>), CoreError> {
    validate_spans(line)?;
    let chars: Vec<_> = line.text.chars().collect();
    let mut formats = vec![(Vec::new(), None); chars.len()];
    for span in &line.formats {
        let key = (canonical_formats(&span.formats, span.size)?, span.size);
        for slot in &mut formats[span.start as usize..span.end as usize] {
            *slot = key.clone();
        }
    }
    let mut byte_codes = Vec::new();
    let mut offsets = Vec::with_capacity(chars.len());
    for (character, key) in chars.iter().zip(formats.iter()) {
        offsets.push(byte_codes.len());
        let code = if key.0.is_empty() && key.1.is_none() {
            0
        } else {
            *codes
                .get(key)
                .ok_or_else(|| character_error("missing_format_code"))?
        };
        let text = character.to_string();
        let (bytes, _, bad) = EUC_KR.encode(&text);
        if bad {
            return Err(CoreError::new(
                ErrorCode::Cp949Invalid,
                "generate_characters",
                false,
            ));
        }
        byte_codes.extend(std::iter::repeat_n(code, bytes.len()));
    }
    Ok((byte_codes, offsets))
}

fn validate_spans(line: &GeneratedLine) -> Result<(), CoreError> {
    let length = line.text.chars().count() as u32;
    let mut end = 0;
    for span in &line.formats {
        if span.start >= span.end || span.end > length || span.start < end {
            return Err(character_error("format_span_range"));
        }
        canonical_formats(&span.formats, span.size)?;
        end = span.end;
    }
    Ok(())
}

fn canonical_formats(
    formats: &[CharacterFormat],
    size: Option<u32>,
) -> Result<Vec<CharacterFormat>, CoreError> {
    let mut key = formats.to_vec();
    key.sort_by_key(|format| rank(*format));
    key.dedup();
    // 크기만 다른 평문 구간은 서식이 비어 있어도 자기 문자모양을 받는다.
    if key.is_empty() && size.is_none() {
        return Err(character_error("empty_format_span"));
    }
    Ok(key)
}

fn rank(format: CharacterFormat) -> u8 {
    match format {
        CharacterFormat::Bold => 0,
        CharacterFormat::Italic => 1,
        CharacterFormat::Underline => 2,
        CharacterFormat::Subscript => 3,
        CharacterFormat::Superscript => 4,
    }
}

fn attr(format: CharacterFormat) -> u32 {
    match format {
        CharacterFormat::Underline => 1,
        CharacterFormat::Bold => 2,
        CharacterFormat::Italic => 4,
        CharacterFormat::Superscript => 16,
        CharacterFormat::Subscript => 32,
    }
}

/// 글자 크기는 원본 런에서 온다. 명시가 없으면 방언 기본값 12pt 다.
///
/// 네이티브 저장본: 같은 굵게가 본문에서는 attr=2/font=12,
/// 표 셀에서는 attr=2/font=10 이다. 즉 크기는 서식 종류로 정해지지 않는다.
const DEFAULT_FONT_POINTS: u32 = 12;

fn entry(formats: Vec<CharacterFormat>, size: Option<u32>, code: u32) -> GeneratedFormatCode {
    GeneratedFormatCode {
        attr: formats.iter().copied().map(attr).fold(0, |a, b| a | b),
        font: size.unwrap_or(DEFAULT_FONT_POINTS),
        formats,
        size,
        code,
    }
}

fn builtin_palette(format_codes: &[GeneratedFormatCode]) -> Result<Vec<u8>, CoreError> {
    let max_code = format_codes
        .iter()
        .map(|entry| entry.code)
        .max()
        .unwrap_or(0);
    let count = BUILTIN_OBJECT_COUNT.max(
        max_code
            .checked_add(1)
            .ok_or_else(|| character_error("palette_count_overflow"))?,
    );
    let count_usize = usize::try_from(count).map_err(|_| character_error("palette_size"))?;
    let mut out = Vec::with_capacity(5 + count_usize * 100);
    out.push(0xbf);
    push_word(&mut out, count);
    out.resize(5 + count_usize * 100, 0);

    let plain = font_record(0, 12, "바탕체")?;
    out[5..105].copy_from_slice(&plain);
    let mut symbol = font_record(0, 9, "Symbol")?;
    symbol[0x44..0x47].copy_from_slice(&[0xc0, 0xc0, 0xc0]);
    out[105..205].copy_from_slice(&symbol);
    for index in 2..count_usize {
        let offset = 5 + index * 100;
        out[offset + 0x40..offset + 0x43].copy_from_slice(&[0xff, 0xff, 0xff]);
        out[offset + 0x50..offset + 0x54].copy_from_slice(&1_u32.to_le_bytes());
    }
    for entry in format_codes {
        let offset = 5 + entry.code as usize * 100;
        out[offset..offset + 100].copy_from_slice(&font_record(entry.attr, entry.font, "바탕체")?);
    }
    Ok(out)
}

fn font_record(attr: u32, font: u32, name: &str) -> Result<[u8; 100], CoreError> {
    let (encoded, _, bad) = EUC_KR.encode(name);
    if bad || encoded.len() > 31 {
        return Err(CoreError::new(
            ErrorCode::Cp949Invalid,
            "generate_characters",
            false,
        ));
    }
    let mut record = [0_u8; 100];
    record[0..4].copy_from_slice(&attr.to_le_bytes());
    record[4..8].copy_from_slice(&font.to_le_bytes());
    record[8..8 + encoded.len()].copy_from_slice(&encoded);
    record[39] = 0x10;
    record[0x40..0x43].copy_from_slice(&[0xff, 0xff, 0xff]);
    record[0x50..0x54].copy_from_slice(&1_u32.to_le_bytes());
    Ok(record)
}

fn push_word(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn character_error(reason: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::StructureInvalid, "generate_characters", false);
    error.details.insert("reason".into(), reason.into());
    error
}
