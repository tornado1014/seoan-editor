use std::{borrow::Cow, collections::BTreeMap, fs, path::Path};

use encoding_rs::{EUC_KR, UTF_8, UTF_16BE, UTF_16LE};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    CharacterFormat,
    error::{CoreError, ErrorCode, io_error, safe_source},
};

const MAX_HML_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HmlSummary {
    #[serde(with = "crate::serde_u64::as_string")]
    pub section_count: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub top_level_paragraph_count: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub content_paragraph_count: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub table_count: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub row_count: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub cell_count: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub image_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HmlInline {
    Text {
        text: String,
        formats: Vec<CharacterFormat>,
        /// 원본 런이 명시한 글자 크기(포인트). 없으면 방언 기본값(12pt)이다.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size: Option<u32>,
    },
    Image {
        image_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HmlSection {
    pub id: String,
    pub index: u32,
    pub title: String,
    pub kind: String,
    pub level: u32,
    pub paragraph_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HmlParagraph {
    pub id: String,
    pub section_id: Option<String>,
    pub marker: Option<String>,
    pub text: String,
    pub tail_text: String,
    pub align: String,
    pub indent_raw: i32,
    pub lmargin_raw: i32,
    pub rmargin_raw: i32,
    pub lspace: u32,
    pub page_break: bool,
    pub table_id: Option<String>,
    pub image_ids: Vec<String>,
    pub inlines: Vec<HmlInline>,
    /// 원본 HLT 줄나눔. 줄마다 EUC-KR 바이트 길이다. 비어 있으면 생성기가 스스로 흘린다.
    ///
    /// ⛔ **통특기 줄나눔은 폭으로 다시 계산할 수 없다.** 네이티브 PCT 의 줄 경계 1,845개
    /// 다음 글자가 한글인 경계만 봐도 줄 폭이 최대 39.5, 최소 33.0 자리에서 감긴다
    /// (한글 1.0, ASCII 0.5 기준). 한계값 하나로 둘을 다 설명할 수 없다. 진짜 규칙은
    /// 비례 폰트 메트릭이다. 통특기는 본문 문단을 다시 흘리지 않으므로
    /// 원본이 들고 있는 줄나눔을 계산하지 말고 그대로 옮긴다.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_line_bytes: Vec<u32>,
    /// 식별번호(최종본 `.fin` 의 `<p num="0001">`) — 통특기는 최종본을 열었을 때만 문단 왼쪽에 「[0001]」로 보인다.
    /// 생성기는 HLT 에 싣지 않는다(편집 세션이 문단과 함께 들고 다니며 화면에만 쓴다). 새로 생긴 문단은 없다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HmlTable {
    pub id: String,
    pub occurrence: u32,
    pub paragraph_id: String,
    pub max_columns: u32,
    pub row_count: u32,
    pub cell_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HmlCell {
    pub id: String,
    pub table_id: String,
    pub row: u32,
    pub occurrence: u32,
    pub text: String,
    pub start_raw: Option<u32>,
    pub width_raw: Option<u32>,
    pub hwidth_raw: Option<u32>,
    pub column_begin: Option<u32>,
    pub row_begin: Option<u32>,
    pub rowspan: u32,
    pub colspan: u32,
    pub merge_continuation: bool,
    pub align: String,
    pub indent_raw: i32,
    pub lspace: u32,
    pub image_ids: Vec<String>,
    pub inlines: Vec<HmlInline>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HmlImage {
    pub id: String,
    pub occurrence: u32,
    pub name: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub secondary_width: Option<u32>,
    pub secondary_height: Option<u32>,
    pub paragraph_id: Option<String>,
    pub cell_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HmlDocument {
    pub source: String,
    pub source_sha256: String,
    pub encoding: String,
    pub title: String,
    pub fonts: Vec<String>,
    pub summary: HmlSummary,
    pub sections: Vec<HmlSection>,
    pub paragraphs: Vec<HmlParagraph>,
    pub tables: Vec<HmlTable>,
    pub cells: Vec<HmlCell>,
    pub images: Vec<HmlImage>,
}

#[derive(Serialize)]
struct HmlSemanticDocument<'a> {
    title: &'a str,
    fonts: &'a [String],
    summary: &'a HmlSummary,
    sections: &'a [HmlSection],
    paragraphs: &'a [HmlParagraph],
    tables: &'a [HmlTable],
    cells: &'a [HmlCell],
    images: &'a [HmlImage],
}

pub fn hml_semantic_sha256(document: &HmlDocument) -> String {
    let semantic = HmlSemanticDocument {
        title: &document.title,
        fonts: &document.fonts,
        summary: &document.summary,
        sections: &document.sections,
        paragraphs: &document.paragraphs,
        tables: &document.tables,
        cells: &document.cells,
        images: &document.images,
    };
    let bytes = serde_json::to_vec(&semantic).expect("HML semantic model is serializable");
    format!("{:X}", Sha256::digest(bytes))
}

#[derive(Debug)]
struct Tag {
    name: String,
    attrs: BTreeMap<String, String>,
    end: bool,
}

#[derive(Debug)]
enum Token {
    Tag(Tag),
    Text(String),
}

#[derive(Debug)]
struct ParagraphBuilder {
    id: Option<String>,
    page_break: bool,
    align: String,
    indent_raw: i32,
    lmargin_raw: i32,
    rmargin_raw: i32,
    lspace: u32,
    table_id: Option<String>,
    image_ids: Vec<String>,
    inlines: Vec<HmlInline>,
    tail_start: Option<usize>,
    tail_text: String,
}

impl ParagraphBuilder {
    fn new(id: Option<String>, attrs: &BTreeMap<String, String>) -> Self {
        Self {
            id,
            page_break: attr_bool(attrs, "pagebreak"),
            align: "justified".to_string(),
            indent_raw: 0,
            lmargin_raw: 0,
            rmargin_raw: 0,
            lspace: 160,
            table_id: None,
            image_ids: Vec::new(),
            inlines: Vec::new(),
            tail_start: None,
            tail_text: String::new(),
        }
    }

    fn apply_shape(&mut self, attrs: &BTreeMap<String, String>) {
        self.align = normalized_align(attrs.get("align").map(String::as_str));
        self.indent_raw = attr_i32(attrs, "indent").unwrap_or(0);
        self.lmargin_raw = attr_i32(attrs, "lmargin").unwrap_or(0);
        self.rmargin_raw = attr_i32(attrs, "rmargin").unwrap_or(0);
        self.lspace = attr_u32(attrs, "lspace").unwrap_or(160);
    }
}

#[derive(Debug)]
struct TableBuilder {
    id: String,
    occurrence: u32,
    paragraph_id: String,
    max_columns: u32,
    row_count: u32,
    row_cell_count: u32,
    cell_ids: Vec<String>,
}

#[derive(Debug)]
struct CellBuilder {
    id: String,
    table_id: String,
    row: u32,
    occurrence: u32,
    start_raw: Option<u32>,
    width_raw: Option<u32>,
    hwidth_raw: Option<u32>,
    column_begin: Option<u32>,
    row_begin: Option<u32>,
    rowspan: u32,
    colspan: u32,
    merge_continuation: bool,
    align: String,
    indent_raw: i32,
    lspace: u32,
    image_ids: Vec<String>,
    inlines: Vec<HmlInline>,
}

pub fn inspect_hml_path(path: &Path) -> Result<HmlDocument, CoreError> {
    let source_text = path.to_string_lossy();
    let metadata =
        fs::metadata(path).map_err(|error| io_error(&error, "hml_read", &source_text))?;
    if metadata.len() > MAX_HML_BYTES {
        let mut error = CoreError::new(ErrorCode::SizeLimitExceeded, "hml_read", false);
        error.source = safe_source(&source_text);
        error.expected = Some(MAX_HML_BYTES.to_string());
        error.actual = Some(metadata.len().to_string());
        return Err(error);
    }
    let bytes = fs::read(path).map_err(|error| io_error(&error, "hml_read", &source_text))?;
    let source_sha256 = format!("{:X}", Sha256::digest(&bytes));
    let (decoded, encoding) = decode_hml(&bytes, &source_text)?;
    parse_hml(
        decoded.as_ref(),
        safe_source(&source_text).unwrap_or_default(),
        source_sha256,
        encoding,
    )
}

fn decode_hml<'a>(bytes: &'a [u8], source: &str) -> Result<(Cow<'a, str>, String), CoreError> {
    let prefix = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_ascii_lowercase();
    let explicit_utf8 = prefix.contains("encoding=\"utf-8\"")
        || prefix.contains("encoding='utf-8'")
        || prefix.contains("encoding=\"utf8\"")
        || prefix.contains("encoding='utf8'");

    let decoded = if bytes.starts_with(&[0xFF, 0xFE]) {
        UTF_16LE
            .decode_without_bom_handling_and_without_replacement(&bytes[2..])
            .map(|value| (value, "utf-16le".to_string()))
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        UTF_16BE
            .decode_without_bom_handling_and_without_replacement(&bytes[2..])
            .map(|value| (value, "utf-16be".to_string()))
    } else if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) || explicit_utf8 {
        let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        UTF_8
            .decode_without_bom_handling_and_without_replacement(body)
            .map(|value| (value, "utf-8".to_string()))
    } else {
        EUC_KR
            .decode_without_bom_handling_and_without_replacement(bytes)
            .map(|value| (value, "cp949".to_string()))
            .or_else(|| {
                UTF_8
                    .decode_without_bom_handling_and_without_replacement(bytes)
                    .map(|value| (value, "utf-8".to_string()))
            })
    };

    decoded.ok_or_else(|| {
        let mut error = CoreError::new(ErrorCode::Cp949Invalid, "hml_decode", false);
        error.source = safe_source(source);
        error
    })
}

fn parse_hml(
    source_text: &str,
    source: String,
    source_sha256: String,
    encoding: String,
) -> Result<HmlDocument, CoreError> {
    let tokens = tokenize(source_text)?;
    let mut title = String::new();
    let mut fonts: Vec<String> = Vec::new();
    let mut sections: Vec<HmlSection> = Vec::new();
    let mut paragraphs = Vec::new();
    let mut tables = Vec::new();
    let mut cells = Vec::new();
    let mut images = Vec::new();
    let mut paragraph_stack: Vec<ParagraphBuilder> = Vec::new();
    let mut current_table: Option<TableBuilder> = None;
    let mut current_cell: Option<CellBuilder> = None;
    let mut current_section: Option<String> = None;
    let mut formats: Vec<CharacterFormat> = Vec::new();
    let mut saw_hwpml = false;
    let mut saw_body = false;
    let mut in_head = false;
    let mut in_body = false;
    let mut in_title = false;
    let mut in_font = false;
    let mut text_depth = 0_u32;
    let mut image_ctrl_depth = 0_u32;

    for token in tokens {
        match token {
            Token::Text(text) => {
                if in_title {
                    title.push_str(&decode_entities(&text));
                }
                if in_font && let Some(font) = fonts.last_mut() {
                    font.push_str(&decode_entities(&text));
                }
                if in_body && text_depth > 0 && !paragraph_stack.is_empty() {
                    let text = decode_entities(&text).replace(['\r', '\n'], "");
                    if !text.is_empty() {
                        push_text(
                            &mut paragraph_stack.last_mut().expect("checked").inlines,
                            text,
                            normalized_formats(&formats),
                        );
                    }
                }
            }
            Token::Tag(tag) if tag.end => match tag.name.as_str() {
                "head" => in_head = false,
                "body" => in_body = false,
                "title" => in_title = false,
                "font" => in_font = false,
                "text" => text_depth = text_depth.saturating_sub(1),
                "b" | "i" | "u" | "sub" | "sup" => {
                    if let Some(format) = tag_format(&tag.name)
                        && let Some(index) = formats.iter().rposition(|value| *value == format)
                    {
                        formats.remove(index);
                    }
                }
                "tail" => {
                    if let Some(paragraph) = paragraph_stack.last_mut()
                        && let Some(start) = paragraph.tail_start.take()
                    {
                        paragraph.tail_text = inline_text(&paragraph.inlines[start..]);
                    }
                }
                "p" => {
                    let Some(paragraph) = paragraph_stack.pop() else {
                        return Err(structure_error("closing p without an open p"));
                    };
                    if let Some(id) = paragraph.id.clone() {
                        let text = inline_text(&paragraph.inlines);
                        let marker = section_marker(&text);
                        let section_id = if let Some(marker_text) = marker.as_ref() {
                            let index = sections.len() as u32 + 1;
                            let id = format!("section:{index}");
                            let previous: Vec<&str> =
                                sections.iter().map(|item| item.title.as_str()).collect();
                            sections.push(HmlSection {
                                id: id.clone(),
                                index,
                                title: marker_text.clone(),
                                kind: section_kind(marker_text).to_string(),
                                level: section_level(marker_text, &previous),
                                paragraph_ids: vec![id_for_paragraph(&paragraph)],
                            });
                            current_section = Some(id.clone());
                            Some(id)
                        } else {
                            if let Some(section_id) = current_section.as_ref()
                                && let Some(section) = sections.last_mut()
                            {
                                section.paragraph_ids.push(id.clone());
                                debug_assert_eq!(&section.id, section_id);
                            }
                            current_section.clone()
                        };
                        paragraphs.push(HmlParagraph {
                            id,
                            section_id,
                            marker,
                            text,
                            tail_text: paragraph.tail_text,
                            align: paragraph.align,
                            indent_raw: paragraph.indent_raw,
                            lmargin_raw: paragraph.lmargin_raw,
                            rmargin_raw: paragraph.rmargin_raw,
                            lspace: paragraph.lspace,
                            page_break: paragraph.page_break,
                            table_id: paragraph.table_id,
                            image_ids: paragraph.image_ids,
                            inlines: paragraph.inlines,
                            source_line_bytes: Vec::new(),
                            number: None,
                        });
                    } else if let Some(cell) = current_cell.as_mut() {
                        cell.align = paragraph.align;
                        cell.indent_raw = paragraph.indent_raw;
                        cell.lspace = paragraph.lspace;
                        cell.image_ids.extend(paragraph.image_ids);
                        cell.inlines.extend(paragraph.inlines);
                    }
                }
                "cell" => {
                    let Some(cell) = current_cell.take() else {
                        return Err(structure_error("closing cell without an open cell"));
                    };
                    let text = inline_text(&cell.inlines);
                    cells.push(HmlCell {
                        id: cell.id,
                        table_id: cell.table_id,
                        row: cell.row,
                        occurrence: cell.occurrence,
                        text,
                        start_raw: cell.start_raw,
                        width_raw: cell.width_raw,
                        hwidth_raw: cell.hwidth_raw,
                        column_begin: cell.column_begin,
                        row_begin: cell.row_begin,
                        rowspan: cell.rowspan,
                        colspan: cell.colspan,
                        merge_continuation: cell.merge_continuation,
                        align: cell.align,
                        indent_raw: cell.indent_raw,
                        lspace: cell.lspace,
                        image_ids: cell.image_ids,
                        inlines: cell.inlines,
                    });
                }
                "box" => {
                    if let Some(table) = current_table.take() {
                        tables.push(HmlTable {
                            id: table.id,
                            occurrence: table.occurrence,
                            paragraph_id: table.paragraph_id,
                            max_columns: table.max_columns,
                            row_count: table.row_count,
                            cell_ids: table.cell_ids,
                        });
                    }
                }
                "ctrlcode" => image_ctrl_depth = image_ctrl_depth.saturating_sub(1),
                _ => {}
            },
            Token::Tag(tag) => match tag.name.as_str() {
                "hwpml" => saw_hwpml = true,
                "head" => in_head = true,
                "body" => {
                    saw_body = true;
                    in_body = true;
                }
                "title" if in_head => {
                    in_title = true;
                    title.clear();
                }
                "font" if in_head => {
                    in_font = true;
                    fonts.push(String::new());
                }
                "p" if in_body => {
                    let id = if paragraph_stack.is_empty() {
                        Some(format!("paragraph:{}", paragraphs.len() + 1))
                    } else {
                        None
                    };
                    paragraph_stack.push(ParagraphBuilder::new(id, &tag.attrs));
                }
                "pstyle" if in_body => {
                    if let Some(paragraph) = paragraph_stack.last_mut() {
                        paragraph.apply_shape(&tag.attrs);
                    }
                }
                "text" if in_body => text_depth += 1,
                "b" | "i" | "u" | "sub" | "sup" if text_depth > 0 => {
                    if let Some(format) = tag_format(&tag.name) {
                        formats.push(format);
                    }
                }
                "ctrlcode" if in_body => {
                    if tag
                        .attrs
                        .get("id")
                        .is_some_and(|value| value.eq_ignore_ascii_case("image"))
                    {
                        image_ctrl_depth += 1;
                    }
                }
                "data" if in_body && image_ctrl_depth > 0 => {
                    let occurrence = images.len() as u32;
                    let image_id = format!("image:{occurrence}");
                    let paragraph_id = paragraph_stack.first().and_then(|item| item.id.clone());
                    let cell_id = current_cell.as_ref().map(|item| item.id.clone());
                    images.push(HmlImage {
                        id: image_id.clone(),
                        occurrence,
                        name: tag.attrs.get("name").cloned().unwrap_or_default(),
                        width: attr_u32(&tag.attrs, "width"),
                        height: attr_u32(&tag.attrs, "height"),
                        secondary_width: attr_u32(&tag.attrs, "w2"),
                        secondary_height: attr_u32(&tag.attrs, "h2"),
                        paragraph_id,
                        cell_id,
                    });
                    if let Some(paragraph) = paragraph_stack.last_mut() {
                        paragraph.image_ids.push(image_id.clone());
                        paragraph.inlines.push(HmlInline::Image { image_id });
                    }
                }
                "box" if in_body => {
                    if tag
                        .attrs
                        .get("type")
                        .is_some_and(|value| value.eq_ignore_ascii_case("table"))
                    {
                        let Some(paragraph_id) =
                            paragraph_stack.first().and_then(|item| item.id.clone())
                        else {
                            return Err(structure_error("table outside a top-level paragraph"));
                        };
                        let occurrence = tables.len() as u32 + 1;
                        let id = format!("table:{occurrence}");
                        if let Some(paragraph) = paragraph_stack.first_mut() {
                            paragraph.table_id = Some(id.clone());
                        }
                        current_table = Some(TableBuilder {
                            id,
                            occurrence,
                            paragraph_id,
                            max_columns: attr_u32(&tag.attrs, "maxcol").unwrap_or(0),
                            row_count: 0,
                            row_cell_count: 0,
                            cell_ids: Vec::new(),
                        });
                    }
                }
                "tr" if current_table.is_some() => {
                    let table = current_table.as_mut().expect("checked");
                    table.row_count += 1;
                    table.row_cell_count = 0;
                }
                "cell" if current_table.is_some() => {
                    if current_cell.is_some() {
                        return Err(structure_error("nested cell"));
                    }
                    let table = current_table.as_mut().expect("checked");
                    if table.row_count == 0 {
                        return Err(structure_error("cell outside a table row"));
                    }
                    table.row_cell_count += 1;
                    let id = format!(
                        "cell:{}:{}:{}",
                        table.occurrence, table.row_count, table.row_cell_count
                    );
                    table.cell_ids.push(id.clone());
                    current_cell = Some(CellBuilder {
                        id,
                        table_id: table.id.clone(),
                        row: table.row_count,
                        occurrence: table.row_cell_count,
                        start_raw: attr_u32(&tag.attrs, "cstart"),
                        width_raw: attr_u32(&tag.attrs, "cwidth"),
                        hwidth_raw: attr_u32(&tag.attrs, "hwidth"),
                        column_begin: attr_u32(&tag.attrs, "colbegin"),
                        row_begin: attr_u32(&tag.attrs, "rowbegin"),
                        rowspan: attr_u32(&tag.attrs, "rowspan").unwrap_or(1),
                        colspan: attr_u32(&tag.attrs, "colspan").unwrap_or(1),
                        merge_continuation: attr_bool(&tag.attrs, "mergecont"),
                        align: normalized_align(tag.attrs.get("align").map(String::as_str)),
                        indent_raw: attr_i32(&tag.attrs, "pindent").unwrap_or(0),
                        lspace: attr_u32(&tag.attrs, "lspace").unwrap_or(160),
                        image_ids: Vec::new(),
                        inlines: Vec::new(),
                    });
                }
                "tail" if in_body => {
                    if let Some(paragraph) = paragraph_stack.last_mut() {
                        paragraph.tail_start = Some(paragraph.inlines.len());
                    }
                }
                _ => {}
            },
        }
    }

    if !saw_hwpml || !saw_body {
        return Err(structure_error("not an HWPML document"));
    }
    if !paragraph_stack.is_empty() || current_cell.is_some() || current_table.is_some() {
        return Err(structure_error("unterminated HML structure"));
    }
    if source_text.to_ascii_lowercase().contains("<body") && paragraphs.is_empty() {
        return Err(structure_error("HML body has no paragraphs"));
    }

    for font in &mut fonts {
        *font = font.trim().to_string();
    }
    title = title.trim().to_string();
    let content_paragraph_count = paragraphs
        .iter()
        .filter(|paragraph| paragraph.marker.is_none())
        .count() as u64;
    let summary = HmlSummary {
        section_count: sections.len() as u64,
        top_level_paragraph_count: paragraphs.len() as u64,
        content_paragraph_count,
        table_count: tables.len() as u64,
        row_count: tables.iter().map(|table| u64::from(table.row_count)).sum(),
        cell_count: cells.len() as u64,
        image_count: images.len() as u64,
    };

    Ok(HmlDocument {
        source,
        source_sha256,
        encoding,
        title,
        fonts,
        summary,
        sections,
        paragraphs,
        tables,
        cells,
        images,
    })
}

fn tokenize(source: &str) -> Result<Vec<Token>, CoreError> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        if bytes[cursor] != b'<' {
            let end = bytes[cursor..]
                .iter()
                .position(|byte| *byte == b'<')
                .map_or(bytes.len(), |offset| cursor + offset);
            tokens.push(Token::Text(source[cursor..end].to_string()));
            cursor = end;
            continue;
        }
        if source[cursor..].starts_with("<!--") {
            let Some(offset) = source[cursor + 4..].find("-->") else {
                return Err(structure_error("unterminated HML comment"));
            };
            cursor += 4 + offset + 3;
            continue;
        }
        if source[cursor..].starts_with("<?") {
            let Some(offset) = source[cursor + 2..].find("?>") else {
                return Err(structure_error("unterminated HML declaration"));
            };
            cursor += 2 + offset + 2;
            continue;
        }
        let mut end = cursor + 1;
        let mut quote = None;
        while end < bytes.len() {
            let byte = bytes[end];
            if let Some(open_quote) = quote {
                if byte == open_quote {
                    quote = None;
                }
            } else if byte == b'\'' || byte == b'"' {
                quote = Some(byte);
            } else if byte == b'>' {
                break;
            }
            end += 1;
        }
        if end == bytes.len() {
            return Err(structure_error("unterminated HML tag"));
        }
        let inside = source[cursor + 1..end].trim();
        cursor = end + 1;
        if inside.is_empty() || inside.starts_with('!') {
            continue;
        }
        tokens.push(Token::Tag(parse_tag(inside)?));
    }
    Ok(tokens)
}

fn parse_tag(source: &str) -> Result<Tag, CoreError> {
    let source = source.trim().trim_end_matches('/').trim_end();
    let end = source.starts_with('/');
    let source = source.strip_prefix('/').unwrap_or(source).trim_start();
    let name_end = source
        .find(|character: char| character.is_whitespace())
        .unwrap_or(source.len());
    if name_end == 0 {
        return Err(structure_error("HML tag has no name"));
    }
    let name = source[..name_end].to_ascii_lowercase();
    let attrs = if end {
        BTreeMap::new()
    } else {
        parse_attrs(&source[name_end..])
    };
    Ok(Tag { name, attrs, end })
}

fn parse_attrs(source: &str) -> BTreeMap<String, String> {
    let bytes = source.as_bytes();
    let mut attrs = BTreeMap::new();
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let key_start = cursor;
        while cursor < bytes.len() && !bytes[cursor].is_ascii_whitespace() && bytes[cursor] != b'='
        {
            cursor += 1;
        }
        if cursor == key_start {
            cursor += 1;
            continue;
        }
        let key = source[key_start..cursor].to_ascii_lowercase();
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let mut value = String::new();
        if cursor < bytes.len() && bytes[cursor] == b'=' {
            cursor += 1;
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if cursor < bytes.len() && (bytes[cursor] == b'\'' || bytes[cursor] == b'"') {
                let quote = bytes[cursor];
                cursor += 1;
                let value_start = cursor;
                while cursor < bytes.len() && bytes[cursor] != quote {
                    cursor += 1;
                }
                value = decode_entities(&source[value_start..cursor]);
                if cursor < bytes.len() {
                    cursor += 1;
                }
            } else {
                let value_start = cursor;
                while cursor < bytes.len() && !bytes[cursor].is_ascii_whitespace() {
                    cursor += 1;
                }
                value = decode_entities(&source[value_start..cursor]);
            }
        }
        attrs.insert(key, value);
    }
    attrs
}

fn decode_entities(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(start) = rest.find('&') {
        output.push_str(&rest[..start]);
        let entity = &rest[start + 1..];
        let Some(end) = entity.find(';') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let name = &entity[..end];
        let replacement = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if name.starts_with("#x") || name.starts_with("#X") => {
                u32::from_str_radix(&name[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
            }
            _ if name.starts_with('#') => name[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        if let Some(character) = replacement {
            output.push(character);
        } else {
            output.push('&');
            output.push_str(name);
            output.push(';');
        }
        rest = &entity[end + 1..];
    }
    output.push_str(rest);
    output
}

fn normalized_align(value: Option<&str>) -> String {
    match value.unwrap_or("justified").to_ascii_lowercase().as_str() {
        "justify" | "justified" => "justified",
        "left" => "left",
        "center" => "center",
        "right" => "right",
        "distribute" => "distribute",
        _ => "justified",
    }
    .to_string()
}

fn attr_u32(attrs: &BTreeMap<String, String>, name: &str) -> Option<u32> {
    attrs.get(name).and_then(|value| value.parse().ok())
}

fn attr_i32(attrs: &BTreeMap<String, String>, name: &str) -> Option<i32> {
    attrs.get(name).and_then(|value| value.parse().ok())
}

fn attr_bool(attrs: &BTreeMap<String, String>, name: &str) -> bool {
    attrs
        .get(name)
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE"))
}

fn tag_format(name: &str) -> Option<CharacterFormat> {
    match name {
        "b" => Some(CharacterFormat::Bold),
        "i" => Some(CharacterFormat::Italic),
        "u" => Some(CharacterFormat::Underline),
        "sub" => Some(CharacterFormat::Subscript),
        "sup" => Some(CharacterFormat::Superscript),
        _ => None,
    }
}

fn normalized_formats(formats: &[CharacterFormat]) -> Vec<CharacterFormat> {
    [
        CharacterFormat::Underline,
        CharacterFormat::Bold,
        CharacterFormat::Italic,
        CharacterFormat::Superscript,
        CharacterFormat::Subscript,
    ]
    .into_iter()
    .filter(|format| formats.contains(format))
    .collect()
}

fn push_text(inlines: &mut Vec<HmlInline>, text: String, formats: Vec<CharacterFormat>) {
    if let Some(HmlInline::Text {
        text: previous,
        formats: previous_formats,
        size: None,
    }) = inlines.last_mut()
        && *previous_formats == formats
    {
        previous.push_str(&text);
        return;
    }
    inlines.push(HmlInline::Text {
        text,
        formats,
        size: None,
    });
}

fn inline_text(inlines: &[HmlInline]) -> String {
    inlines
        .iter()
        .filter_map(|inline| match inline {
            HmlInline::Text { text, .. } => Some(text.as_str()),
            HmlInline::Image { .. } => None,
        })
        .collect()
}

fn id_for_paragraph(paragraph: &ParagraphBuilder) -> String {
    paragraph.id.clone().expect("top-level paragraph has an id")
}

fn section_marker(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (trimmed.starts_with('【') && trimmed.ends_with('】')).then(|| trimmed.to_string())
}

pub(crate) fn section_kind(title: &str) -> &'static str {
    let title = title.trim_matches(['【', '】', ' ']);
    if title.starts_with("표 ") {
        "table"
    } else if title.starts_with("도 ") {
        "figure"
    } else if title.starts_with("청구항") {
        "claim"
    } else {
        "section"
    }
}

pub(crate) fn section_level(title: &str, _previous: &[&str]) -> u32 {
    let title = title.trim_matches(['【', '】', ' ']);
    match section_kind(title) {
        "table" => 2,
        "claim" | "figure" => 1,
        _ => match title {
            "발명의 설명" | "청구범위" | "요약서" | "도면" => 0,
            "과제의 해결 수단" => 2,
            _ => 1,
        },
    }
}

fn structure_error(message: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::StructureInvalid, "hml_parse", false);
    error.message = message.to_string();
    error
}
