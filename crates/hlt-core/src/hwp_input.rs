//! 한글 원본(.hwp · .hwpx) 입력.
//!
//! [rhwp](https://github.com/edwardkim/rhwp)(MIT)로 한글 문서를 읽어 **최소 워드(.docx) 패키지**로 옮긴 뒤,
//! 기존 워드 입력 경로([`crate::inspect_docx_path`])를 그대로 탄다. 표제 인식 · 표 병합 · 그림 크기 ·
//! 글자 서식 · 들여쓰기 규칙을 워드 입력과 하나로 유지하기 위해서다(규칙을 두 벌로 키우지 않는다).
//!
//! 옮기는 것: 문단 글 · 정렬 · 첫 줄 들여쓰기/내어쓰기 · 왼쪽 여백 · 쪽 나누기 · 굵게/기울임/밑줄/위·아래 첨자 ·
//! 표(가로 · 세로 병합, 칸 너비) · 글자처럼 놓인 그림과 묶음 안 그림.
//! 옮기지 않는 것: 머리말/꼬리말 · 각주/미주 · 수식 개체 · 글상자 · 도형(그림이 아닌 것) · 표 안의 표(글만 남긴다).

use std::fmt::Write as _;
use std::io::{Cursor, Write};
use std::path::Path;

use rhwp::model::control::Control;
use rhwp::model::document::Document;
use rhwp::model::image::Picture;
use rhwp::model::paragraph::{ColumnBreakType, Paragraph};
use rhwp::model::shape::ShapeObject;
use rhwp::model::style::{Alignment, UnderlineType};
use rhwp::model::table::Table;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use crate::error::{CoreError, ErrorCode, io_error, safe_source};

const MAX_HWP_BYTES: u64 = 256 * 1024 * 1024;
/// 1 HWPUNIT = 1/7200 in, 1 dxa = 1/1440 in.
const HU_PER_DXA: i64 = 5;
/// 1 HWPUNIT = 914400/7200 EMU.
const EMU_PER_HU: i64 = 127;
/// 「글자」 단위 여백(홀수 저장값)의 한 글자 폭 — 10pt 반각(500 HU)으로 본다.
const CHAR_UNIT_HU: i64 = 500;

/// 바꾼 결과: 워드 패키지 바이트와, 옮기지 못해 버린 개체 수.
#[derive(Debug, Default)]
pub struct HwpDocxConversion {
    pub docx: Vec<u8>,
    pub skipped_objects: usize,
}

/// 한글 파일(.hwp · .hwpx · 옛 한글 3.0)을 읽어 최소 워드 패키지로 바꾼다.
pub fn hwp_path_to_docx(path: &Path) -> Result<HwpDocxConversion, CoreError> {
    let source_text = path.to_string_lossy();
    let metadata =
        std::fs::metadata(path).map_err(|error| io_error(&error, "hwp_read", &source_text))?;
    if metadata.len() > MAX_HWP_BYTES {
        let mut error = CoreError::new(ErrorCode::SizeLimitExceeded, "hwp_read", false);
        error.source = safe_source(&source_text);
        error.expected = Some(MAX_HWP_BYTES.to_string());
        error.actual = Some(metadata.len().to_string());
        return Err(error);
    }
    let bytes = std::fs::read(path).map_err(|error| io_error(&error, "hwp_read", &source_text))?;
    hwp_bytes_to_docx(&bytes).map_err(|mut error| {
        error.source = safe_source(&source_text);
        error
    })
}

/// 바이트에서 바로 — 형식(HWP 5.0 · HWPX · HWP 3.0)은 rhwp 가 알아본다.
pub fn hwp_bytes_to_docx(bytes: &[u8]) -> Result<HwpDocxConversion, CoreError> {
    let document = rhwp::parse_document(bytes).map_err(|error| {
        let mut core = CoreError::new(ErrorCode::UnsupportedFormat, "hwp_parse", false);
        core.details
            .insert("reason".into(), "hwp_unreadable".into());
        // 암호 · DRM 문서도 여기로 온다 — 원인 글을 그대로 붙인다(경로는 담지 않는다).
        core.details.insert(
            "parser".into(),
            error.to_string().chars().take(200).collect(),
        );
        core
    })?;
    let mut writer = DocxWriter::new(&document);
    for section in &document.sections {
        for paragraph in &section.paragraphs {
            writer.body_paragraph(paragraph);
        }
    }
    writer.finish()
}

struct Media {
    relationship_id: String,
    part: String,
    bytes: Vec<u8>,
}

struct DocxWriter<'a> {
    document: &'a Document,
    body: String,
    media: Vec<Media>,
    skipped: usize,
    /// 첫 【…】 표제를 지났는가. 그 앞(표지 · 서지 표 등)의 표는 생성기가 받지 않으므로 글 문단으로 푼다.
    seen_heading: bool,
}

/// 한 문단 안의 조각: 글 · 그림 · (본문 문단에서만) 표.
enum Piece<'a> {
    Text(String, u32),
    Picture(&'a Picture),
    Table(&'a Table),
    LineBreak,
}

impl<'a> DocxWriter<'a> {
    fn new(document: &'a Document) -> Self {
        Self {
            document,
            body: String::new(),
            media: Vec::new(),
            skipped: 0,
            seen_heading: false,
        }
    }

    /// 본문 문단: 표가 들어 있으면 표 앞 글 · 표 · 표 뒤 글로 나눠 블록을 차례로 낸다.
    fn body_paragraph(&mut self, paragraph: &'a Paragraph) {
        let properties = self.paragraph_properties(paragraph);
        let trimmed = paragraph.text.trim();
        if trimmed.starts_with('【') && trimmed.ends_with('】') {
            self.seen_heading = true;
        }
        let mut page_break = matches!(paragraph.column_type, ColumnBreakType::Page);
        let mut runs = String::new();
        for piece in self.pieces(paragraph, true) {
            match piece {
                Piece::Table(table) => {
                    if !runs.is_empty() || page_break {
                        self.emit_paragraph(&properties, &runs, page_break);
                        page_break = false;
                        runs.clear();
                    }
                    if self.seen_heading {
                        self.emit_table(table);
                    } else {
                        self.emit_table_as_text(table, &properties);
                    }
                }
                Piece::LineBreak => {
                    self.emit_paragraph(&properties, &runs, page_break);
                    page_break = false;
                    runs.clear();
                }
                other => runs.push_str(&self.run(other)),
            }
        }
        self.emit_paragraph(&properties, &runs, page_break);
    }

    fn emit_paragraph(&mut self, properties: &str, runs: &str, page_break: bool) {
        self.body.push_str("<w:p>");
        self.body.push_str(properties);
        if page_break {
            self.body.push_str(r#"<w:r><w:br w:type="page"/></w:r>"#);
        }
        self.body.push_str(runs);
        self.body.push_str("</w:p>\n");
    }

    /// 문단 글을 글자 모양이 같은 덩어리로 자르고, 개체를 제자리(글자 위치)에 끼운다.
    fn pieces(&mut self, paragraph: &'a Paragraph, allow_tables: bool) -> Vec<Piece<'a>> {
        let positions = paragraph.control_text_positions();
        let mut anchored: Vec<(usize, Piece<'a>)> = Vec::new();
        for (index, control) in paragraph.controls.iter().enumerate() {
            let at = positions.get(index).copied().unwrap_or(usize::MAX);
            match control {
                Control::Picture(picture) => anchored.push((at, Piece::Picture(picture))),
                Control::Table(table) if allow_tables => anchored.push((at, Piece::Table(table))),
                Control::Table(table) => {
                    // 표 안의 표: 글만 남긴다.
                    self.skipped += 1;
                    let text: Vec<String> = table
                        .cells
                        .iter()
                        .flat_map(|cell| cell.paragraphs.iter().map(|p| p.text.clone()))
                        .filter(|text| !text.trim().is_empty())
                        .collect();
                    if !text.is_empty() {
                        anchored.push((at, Piece::Text(text.join(" "), u32::MAX)));
                    }
                }
                Control::Shape(shape) => {
                    let mut pictures = Vec::new();
                    collect_shape_pictures(shape, &mut pictures);
                    if pictures.is_empty() {
                        self.skipped += 1;
                    }
                    for picture in pictures {
                        anchored.push((at, Piece::Picture(picture)));
                    }
                }
                Control::Equation(_) | Control::Footnote(_) | Control::Endnote(_) => {
                    self.skipped += 1;
                }
                _ => {}
            }
        }
        let chars: Vec<char> = paragraph.text.chars().collect();
        let mut pieces = Vec::new();
        let mut anchored = anchored.into_iter().peekable();
        let mut current = String::new();
        let mut current_shape = u32::MAX;
        for (index, ch) in chars.iter().copied().enumerate() {
            while anchored.peek().is_some_and(|(at, _)| *at <= index) {
                if !current.is_empty() {
                    pieces.push(Piece::Text(std::mem::take(&mut current), current_shape));
                }
                pieces.push(anchored.next().expect("peeked").1);
            }
            if ch == '\n' || ch == '\r' {
                if !current.is_empty() {
                    pieces.push(Piece::Text(std::mem::take(&mut current), current_shape));
                }
                pieces.push(Piece::LineBreak);
                continue;
            }
            let shape = paragraph.char_shape_id_at(index).unwrap_or(u32::MAX);
            if shape != current_shape && !current.is_empty() {
                pieces.push(Piece::Text(std::mem::take(&mut current), current_shape));
            }
            current_shape = shape;
            current.push(ch);
        }
        if !current.is_empty() {
            pieces.push(Piece::Text(current, current_shape));
        }
        pieces.extend(anchored.map(|(_, piece)| piece));
        // 문단 끝의 줄바꿈은 빈 문단을 하나 더 만들지 않게 버린다.
        while matches!(pieces.last(), Some(Piece::LineBreak)) {
            pieces.pop();
        }
        pieces
    }

    fn run(&mut self, piece: Piece<'a>) -> String {
        match piece {
            Piece::Text(text, shape) => {
                let mut run = String::from("<w:r>");
                run.push_str(&self.run_properties(shape));
                for (index, part) in text.split('\t').enumerate() {
                    if index > 0 {
                        run.push_str("<w:tab/>");
                    }
                    if !part.is_empty() {
                        let _ = write!(run, r#"<w:t xml:space="preserve">{}</w:t>"#, escape(part));
                    }
                }
                run.push_str("</w:r>");
                run
            }
            Piece::Picture(picture) => self.picture_run(picture),
            Piece::Table(_) | Piece::LineBreak => String::new(),
        }
    }

    fn run_properties(&self, shape: u32) -> String {
        let Some(shape) = self.document.doc_info.char_shapes.get(shape as usize) else {
            return String::new();
        };
        let mut properties = String::new();
        if shape.bold {
            properties.push_str("<w:b/>");
        }
        if shape.italic {
            properties.push_str("<w:i/>");
        }
        if !matches!(shape.underline_type, UnderlineType::None) {
            properties.push_str(r#"<w:u w:val="single"/>"#);
        }
        if shape.superscript {
            properties.push_str(r#"<w:vertAlign w:val="superscript"/>"#);
        } else if shape.subscript {
            properties.push_str(r#"<w:vertAlign w:val="subscript"/>"#);
        }
        if properties.is_empty() {
            properties
        } else {
            format!("<w:rPr>{properties}</w:rPr>")
        }
    }

    fn picture_run(&mut self, picture: &Picture) -> String {
        let id = picture.image_attr.bin_data_id;
        let contents = &self.document.bin_data_content;
        let found = usize::from(id)
            .checked_sub(1)
            .filter(|index| *index < contents.len())
            .or_else(|| contents.iter().position(|content| content.id == id));
        let Some(content) = found.map(|index| &contents[index]) else {
            self.skipped += 1;
            return String::new();
        };
        let bytes = content.data.load();
        if bytes.is_empty() {
            self.skipped += 1;
            return String::new();
        }
        let number = self.media.len() + 1;
        let extension = content
            .extension
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect::<String>();
        let extension = if extension.is_empty() {
            "bin".to_string()
        } else {
            extension
        };
        let relationship_id = format!("rIdHwp{number}");
        self.media.push(Media {
            relationship_id: relationship_id.clone(),
            part: format!("media/image{number}.{extension}"),
            bytes,
        });
        let cx = i64::from(picture.common.width).max(1) * EMU_PER_HU;
        let cy = i64::from(picture.common.height).max(1) * EMU_PER_HU;
        format!(
            r#"<w:r><w:drawing><wp:inline><wp:extent cx="{cx}" cy="{cy}"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="{relationship_id}"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#
        )
    }

    fn paragraph_properties(&self, paragraph: &Paragraph) -> String {
        let Some(shape) = self
            .document
            .doc_info
            .para_shapes
            .get(usize::from(paragraph.para_shape_id))
        else {
            return String::new();
        };
        let jc = match shape.alignment {
            Alignment::Left => "left",
            Alignment::Right => "right",
            Alignment::Center => "center",
            Alignment::Justify | Alignment::Distribute | Alignment::Split => "both",
        };
        let left = margin_dxa(shape.margin_left).max(0);
        let first = margin_dxa(shape.indent);
        let mut ind = String::new();
        if left != 0 || first != 0 {
            ind.push_str("<w:ind");
            if left != 0 {
                let _ = write!(ind, r#" w:left="{left}""#);
            }
            if first != 0 {
                let _ = write!(ind, r#" w:firstLine="{first}""#);
            }
            ind.push_str("/>");
        }
        format!(r#"<w:pPr><w:jc w:val="{jc}"/>{ind}</w:pPr>"#)
    }

    /// 첫 표제 앞의 표: 행마다 칸 글을 탭으로 이은 문단 하나씩.
    fn emit_table_as_text(&mut self, table: &'a Table, properties: &str) {
        for row in 0..table.row_count {
            let mut cells: Vec<_> = table.cells.iter().filter(|cell| cell.row == row).collect();
            cells.sort_by_key(|cell| cell.col);
            let text: Vec<String> = cells
                .iter()
                .map(|cell| {
                    cell.paragraphs
                        .iter()
                        .map(|p| p.text.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect();
            if text.iter().all(String::is_empty) {
                continue;
            }
            let runs = self.run(Piece::Text(text.join("\t"), u32::MAX));
            self.emit_paragraph(properties, &runs, false);
        }
    }

    fn emit_table(&mut self, table: &'a Table) {
        let rows = usize::from(table.row_count);
        let columns = usize::from(table.col_count);
        if rows == 0 || columns == 0 || table.cells.is_empty() {
            self.skipped += 1;
            return;
        }
        // 칸 너비: 한 칸짜리 셀의 너비를 먼저, 없으면 걸친 셀 너비를 고르게 나눈다.
        let mut widths: Vec<Option<i64>> = vec![None; columns];
        for cell in table.cells.iter().filter(|cell| cell.col_span == 1) {
            if let Some(slot) = widths.get_mut(usize::from(cell.col)) {
                slot.get_or_insert(i64::from(cell.width) / HU_PER_DXA);
            }
        }
        for cell in table.cells.iter().filter(|cell| cell.col_span > 1) {
            let start = usize::from(cell.col);
            let end = (start + usize::from(cell.col_span)).min(columns);
            let known: i64 = widths[start..end].iter().flatten().sum();
            let unknown = widths[start..end].iter().filter(|w| w.is_none()).count() as i64;
            if unknown > 0 {
                let share = ((i64::from(cell.width) / HU_PER_DXA - known) / unknown).max(1);
                for slot in &mut widths[start..end] {
                    slot.get_or_insert(share);
                }
            }
        }
        let widths: Vec<i64> = widths
            .into_iter()
            .map(|w| w.unwrap_or(1000).max(1))
            .collect();

        let mut xml = String::from("<w:tbl><w:tblPr/><w:tblGrid>");
        for width in &widths {
            let _ = write!(xml, r#"<w:gridCol w:w="{width}"/>"#);
        }
        xml.push_str("</w:tblGrid>");
        for row in 0..rows {
            xml.push_str("<w:tr>");
            let mut column = 0;
            while column < columns {
                let starts = table
                    .cells
                    .iter()
                    .find(|cell| usize::from(cell.row) == row && usize::from(cell.col) == column);
                let covering = table.cells.iter().find(|cell| {
                    let (r, c) = (usize::from(cell.row), usize::from(cell.col));
                    r < row && row < r + usize::from(cell.row_span.max(1)) && c == column
                });
                if let Some(cell) = starts {
                    let span = usize::from(cell.col_span.max(1)).min(columns - column);
                    let width: i64 = widths[column..column + span].iter().sum();
                    let _ = write!(xml, r#"<w:tc><w:tcPr><w:tcW w:w="{width}"/>"#);
                    if span > 1 {
                        let _ = write!(xml, r#"<w:gridSpan w:val="{span}"/>"#);
                    }
                    if cell.row_span > 1 {
                        xml.push_str(r#"<w:vMerge w:val="restart"/>"#);
                    }
                    xml.push_str("</w:tcPr>");
                    let mut wrote = false;
                    for paragraph in &cell.paragraphs {
                        let properties = self.paragraph_properties(paragraph);
                        let mut runs = String::new();
                        for piece in self.pieces(paragraph, false) {
                            match piece {
                                // 칸 안 줄바꿈은 칸 문단 하나로 잇는다(칸은 한 문단씩 읽힌다).
                                Piece::LineBreak => {
                                    runs.push_str("<w:r><w:t xml:space=\"preserve\"> </w:t></w:r>")
                                }
                                other => runs.push_str(&self.run(other)),
                            }
                        }
                        let _ = write!(xml, "<w:p>{properties}{runs}</w:p>");
                        wrote = true;
                    }
                    if !wrote {
                        xml.push_str("<w:p/>");
                    }
                    xml.push_str("</w:tc>");
                    column += span;
                } else if let Some(cell) = covering {
                    let span = usize::from(cell.col_span.max(1)).min(columns - column);
                    let width: i64 = widths[column..column + span].iter().sum();
                    let _ = write!(xml, r#"<w:tc><w:tcPr><w:tcW w:w="{width}"/>"#);
                    if span > 1 {
                        let _ = write!(xml, r#"<w:gridSpan w:val="{span}"/>"#);
                    }
                    xml.push_str("<w:vMerge/></w:tcPr><w:p/></w:tc>");
                    column += span;
                } else {
                    // 빈 자리(가로 병합 안쪽이 아닌 결손) — 빈 칸으로 채워 격자를 맞춘다.
                    let _ = write!(
                        xml,
                        r#"<w:tc><w:tcPr><w:tcW w:w="{}"/></w:tcPr><w:p/></w:tc>"#,
                        widths[column]
                    );
                    column += 1;
                }
            }
            xml.push_str("</w:tr>");
        }
        xml.push_str("</w:tbl>\n");
        self.body.push_str(&xml);
    }

    fn finish(self) -> Result<HwpDocxConversion, CoreError> {
        let document = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><w:body>
{}</w:body></w:document>"#,
            self.body
        );
        let mut relationships = String::from(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        );
        for media in &self.media {
            let _ = write!(
                relationships,
                r#"<Relationship Id="{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="{}"/>"#,
                media.relationship_id, media.part
            );
        }
        relationships.push_str("</Relationships>");
        let content_types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
        let root_relationships = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

        let package_error = || {
            let mut error = CoreError::new(ErrorCode::StructureInvalid, "hwp_convert", false);
            error.details.insert("reason".into(), "docx_package".into());
            error
        };
        let mut bytes = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut bytes);
            let options =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            let mut put = |name: &str, content: &[u8]| -> Result<(), CoreError> {
                zip.start_file(name, options).map_err(|_| package_error())?;
                zip.write_all(content).map_err(|_| package_error())
            };
            put("[Content_Types].xml", content_types.as_bytes())?;
            put("_rels/.rels", root_relationships.as_bytes())?;
            put("word/document.xml", document.as_bytes())?;
            put("word/_rels/document.xml.rels", relationships.as_bytes())?;
            for media in &self.media {
                put(&format!("word/{}", media.part), &media.bytes)?;
            }
            zip.finish().map_err(|_| package_error())?;
        }
        Ok(HwpDocxConversion {
            docx: bytes.into_inner(),
            skipped_objects: self.skipped,
        })
    }
}

fn collect_shape_pictures<'s>(shape: &'s ShapeObject, out: &mut Vec<&'s Picture>) {
    match shape {
        ShapeObject::Picture(picture) => out.push(picture),
        ShapeObject::Group(group) => {
            for child in &group.children {
                collect_shape_pictures(child, out);
            }
        }
        _ => {}
    }
}

/// 문단 여백 저장값 → dxa. 짝수 = HWPUNIT × 2, 홀수 = 「글자」 단위(HWP 5.0 사양 · rhwp `para_margin_hwpunit`).
fn margin_dxa(stored: i32) -> i64 {
    let stored = i64::from(stored);
    let hu = if stored % 2 != 0 {
        (stored - stored.signum()) / 2 * CHAR_UNIT_HU
    } else {
        stored / 2
    };
    hu / HU_PER_DXA
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // XML 1.0 에 못 넣는 제어 글자는 버린다.
            c if (c as u32) < 0x20 => {}
            '\u{FFFE}' | '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::margin_dxa;

    #[test]
    fn stored_margins_convert_to_dxa() {
        // 짝수: HWPUNIT × 2 — 20pt(2000 HU) 들여쓰기 = 4000 저장 = 400 dxa.
        assert_eq!(margin_dxa(4000), 400);
        assert_eq!(margin_dxa(-4000), -400);
        assert_eq!(margin_dxa(0), 0);
        // 홀수: 글자 단위 — 2글자.
        assert_eq!(margin_dxa(5), 200);
    }
}
