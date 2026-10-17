use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Cursor, Read},
    path::Path,
};

use image::ImageReader;
use quick_xml::{Reader, events::Event};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::{
    CharacterFormat, GeneratedImagePayload, HmlCell, HmlDocument, HmlImage, HmlInline,
    HmlParagraph, HmlSection, HmlSummary, HmlTable,
    error::{CoreError, ErrorCode, io_error, safe_source},
};

const MAX_DOCX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_XML_PART_BYTES: u64 = 64 * 1024 * 1024;
const MAX_MEDIA_PART_BYTES: u64 = 128 * 1024 * 1024;
const MAX_TOTAL_MEDIA_BYTES: u64 = 512 * 1024 * 1024;
const DOCUMENT_PART: &str = "word/document.xml";
const STYLES_PART: &str = "word/styles.xml";
const RELATIONSHIPS_PART: &str = "word/_rels/document.xml.rels";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocxPackageSummary {
    pub entry_count: u32,
    #[serde(with = "crate::serde_u64::as_string")]
    pub main_document_bytes: u64,
    #[serde(with = "crate::serde_u64::option")]
    pub styles_bytes: Option<u64>,
    #[serde(with = "crate::serde_u64::as_string")]
    pub relationships_bytes: u64,
    pub media_count: u32,
    #[serde(with = "crate::serde_u64::as_string")]
    pub media_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocxMediaAsset {
    pub relationship_id: String,
    pub target: String,
    pub media_name: String,
    #[serde(with = "crate::serde_u64::as_string")]
    pub byte_size: u64,
    pub sha256: String,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocxPageBreak {
    pub body_paragraph_index: u32,
    pub semantic_paragraph_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocxDocument {
    pub source: String,
    pub source_sha256: String,
    pub package: DocxPackageSummary,
    pub page_breaks: Vec<DocxPageBreak>,
    pub media: Vec<DocxMediaAsset>,
    pub document: HmlDocument,
    #[serde(skip)]
    image_sources: Vec<DocxImageSource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DocxImageSource {
    image_id: String,
    relationship_id: String,
    target: String,
}

#[derive(Debug, Clone)]
struct PartInfo {
    index: usize,
    size: u64,
}

#[derive(Debug)]
struct Relationship {
    id: String,
    target: String,
}

#[derive(Debug, Clone)]
enum WordContent {
    Text(String),
    Element(WordNode),
}

#[derive(Debug, Clone)]
struct WordNode {
    name: String,
    attributes: BTreeMap<String, String>,
    content: Vec<WordContent>,
}

impl WordNode {
    fn first_child(&self, name: &str) -> Option<&Self> {
        self.content.iter().find_map(|content| match content {
            WordContent::Element(node) if node.name.eq_ignore_ascii_case(name) => Some(node),
            _ => None,
        })
    }

    fn child_elements(&self) -> impl Iterator<Item = &Self> {
        self.content.iter().filter_map(|content| match content {
            WordContent::Element(node) => Some(node),
            WordContent::Text(_) => None,
        })
    }

    fn first_descendant(&self, name: &str) -> Option<&Self> {
        if self.name.eq_ignore_ascii_case(name) {
            return Some(self);
        }
        self.child_elements()
            .find_map(|child| child.first_descendant(name))
    }

    fn descendants_named<'a>(&'a self, name: &str, output: &mut Vec<&'a Self>) {
        for child in self.child_elements() {
            if child.name.eq_ignore_ascii_case(name) {
                output.push(child);
            }
            child.descendants_named(name, output);
        }
    }

    fn text(&self) -> String {
        let mut output = String::new();
        self.push_text(&mut output);
        output
    }

    fn push_text(&self, output: &mut String) {
        for content in &self.content {
            match content {
                WordContent::Text(text) => output.push_str(text),
                WordContent::Element(node) => node.push_text(output),
            }
        }
    }

    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(String::as_str)
    }
}

#[derive(Debug)]
enum DocxInlineSeed {
    Text {
        text: String,
        formats: Vec<CharacterFormat>,
        size: Option<u32>,
    },
    Image {
        relationship_id: String,
    },
}

#[derive(Debug)]
struct ParagraphDraft {
    inlines: Vec<DocxInlineSeed>,
    align: String,
    indent_raw: i32,
    page_break: bool,
}

pub fn inspect_docx_path(path: &Path) -> Result<DocxDocument, CoreError> {
    let source_text = path.to_string_lossy();
    let metadata =
        fs::metadata(path).map_err(|error| io_error(&error, "docx_read", &source_text))?;
    if metadata.len() > MAX_DOCX_BYTES {
        let mut error = CoreError::new(ErrorCode::SizeLimitExceeded, "docx_read", false);
        error.source = safe_source(&source_text);
        error.expected = Some(MAX_DOCX_BYTES.to_string());
        error.actual = Some(metadata.len().to_string());
        return Err(error);
    }
    let bytes = fs::read(path).map_err(|error| io_error(&error, "docx_read", &source_text))?;
    let source_sha256 = format!("{:X}", Sha256::digest(&bytes));
    let mut archive =
        ZipArchive::new(Cursor::new(bytes.as_slice())).map_err(|_| package_error(&source_text))?;
    let entry_count = u32::try_from(archive.len()).map_err(|_| package_error(&source_text))?;
    let parts = index_parts(&mut archive, &source_text)?;
    let main = required_part(
        &parts,
        DOCUMENT_PART,
        ErrorCode::UnsupportedFormat,
        &source_text,
    )?;
    let relationships = required_part(
        &parts,
        RELATIONSHIPS_PART,
        ErrorCode::StructureInvalid,
        &source_text,
    )?;
    if main.size > MAX_XML_PART_BYTES || relationships.size > MAX_XML_PART_BYTES {
        return Err(part_limit_error(&source_text));
    }
    let styles_bytes = parts.get(STYLES_PART).map(|part| part.size);
    if styles_bytes.is_some_and(|size| size > MAX_XML_PART_BYTES) {
        return Err(part_limit_error(&source_text));
    }

    let media_parts: Vec<_> = parts
        .iter()
        .filter(|(name, _)| name.starts_with("word/media/"))
        .collect();
    let media_bytes = media_parts.iter().try_fold(0_u64, |total, (_, part)| {
        if part.size > MAX_MEDIA_PART_BYTES {
            return Err(part_limit_error(&source_text));
        }
        total
            .checked_add(part.size)
            .filter(|value| *value <= MAX_TOTAL_MEDIA_BYTES)
            .ok_or_else(|| part_limit_error(&source_text))
    })?;
    let media_count = u32::try_from(media_parts.len()).map_err(|_| package_error(&source_text))?;

    let main_bytes = read_part(&mut archive, main, &source_text)?;
    let relationships_bytes = read_part(&mut archive, relationships, &source_text)?;
    let image_relationships = parse_image_relationships(&relationships_bytes, &source_text)?;
    let mut media = Vec::new();
    for relationship in image_relationships {
        let target = normalize_relationship_target(&relationship.target, &source_text)?;
        let part = parts
            .get(target.as_str())
            .ok_or_else(|| relationships_error(&source_text))?;
        let payload = read_part(&mut archive, part, &source_text)?;
        let (pixel_width, pixel_height) = ImageReader::new(Cursor::new(payload.as_slice()))
            .with_guessed_format()
            .map_err(|_| relationships_error(&source_text))?
            .into_dimensions()
            .map_err(|_| relationships_error(&source_text))?;
        let media_name = target.rsplit('/').next().unwrap_or(&target).to_string();
        media.push(DocxMediaAsset {
            relationship_id: relationship.id,
            target,
            media_name,
            byte_size: payload.len() as u64,
            sha256: format!("{:X}", Sha256::digest(&payload)),
            pixel_width,
            pixel_height,
        });
    }

    let source = safe_source(&source_text).unwrap_or_default();
    let (document, page_breaks, image_sources) = normalize_word_document(
        &main_bytes,
        &media,
        source.clone(),
        source_sha256.clone(),
        &source_text,
    )?;
    Ok(DocxDocument {
        source,
        source_sha256,
        package: DocxPackageSummary {
            entry_count,
            main_document_bytes: main.size,
            styles_bytes,
            relationships_bytes: relationships.size,
            media_count,
            media_bytes,
        },
        page_breaks,
        media,
        document,
        image_sources,
    })
}

pub fn load_docx_image_payloads(
    path: &Path,
    document: &DocxDocument,
) -> Result<Vec<GeneratedImagePayload>, CoreError> {
    let source_text = path.to_string_lossy();
    let bytes = fs::read(path).map_err(|error| io_error(&error, "docx_read", &source_text))?;
    if bytes.len() as u64 > MAX_DOCX_BYTES
        || format!("{:X}", Sha256::digest(&bytes)) != document.source_sha256
    {
        return Err(package_error(&source_text));
    }
    let mut archive =
        ZipArchive::new(Cursor::new(bytes.as_slice())).map_err(|_| package_error(&source_text))?;
    let parts = index_parts(&mut archive, &source_text)?;
    let media_by_relationship: BTreeMap<_, _> = document
        .media
        .iter()
        .map(|asset| (asset.relationship_id.as_str(), asset))
        .collect();
    let mut payloads = Vec::with_capacity(document.image_sources.len());
    for source in &document.image_sources {
        let asset = media_by_relationship
            .get(source.relationship_id.as_str())
            .ok_or_else(|| relationships_error(&source_text))?;
        if asset.target != source.target {
            return Err(relationships_error(&source_text));
        }
        let part = parts
            .get(source.target.as_str())
            .ok_or_else(|| relationships_error(&source_text))?;
        if part.size > MAX_MEDIA_PART_BYTES {
            return Err(part_limit_error(&source_text));
        }
        let payload = read_part(&mut archive, part, &source_text)?;
        if payload.len() as u64 != asset.byte_size
            || format!("{:X}", Sha256::digest(&payload)) != asset.sha256
        {
            return Err(package_error(&source_text));
        }
        payloads.push(GeneratedImagePayload {
            image_id: source.image_id.clone(),
            bytes: payload,
        });
    }
    Ok(payloads)
}

fn normalize_word_document(
    bytes: &[u8],
    media: &[DocxMediaAsset],
    source: String,
    source_sha256: String,
    display_source: &str,
) -> Result<(HmlDocument, Vec<DocxPageBreak>, Vec<DocxImageSource>), CoreError> {
    let root = parse_word_xml(bytes, display_source)?;
    if !root.name.eq_ignore_ascii_case("document") {
        return Err(document_error(display_source));
    }
    let body = root
        .first_descendant("body")
        .ok_or_else(|| document_error(display_source))?;
    let media_by_relationship: BTreeMap<_, _> = media
        .iter()
        .map(|asset| (asset.relationship_id.as_str(), asset))
        .collect();
    let mut document = HmlDocument {
        source,
        source_sha256,
        encoding: "utf-8".to_string(),
        title: "특허 출원서의 명세서등 서식".to_string(),
        fonts: vec!["신명조".to_string(); 3],
        summary: empty_hml_summary(),
        sections: Vec::new(),
        paragraphs: Vec::new(),
        tables: Vec::new(),
        cells: Vec::new(),
        images: Vec::new(),
    };
    let mut page_breaks = Vec::new();
    let mut image_sources = Vec::new();
    let mut current_section = None;
    let mut body_paragraph_index = 0_u32;

    let body_children: Vec<_> = body.child_elements().collect();
    let mut body_index = 0_usize;
    while body_index < body_children.len() {
        let child = body_children[body_index];
        if child.name.eq_ignore_ascii_case("p") {
            body_paragraph_index += 1;
            append_word_paragraph(
                &mut document,
                &mut current_section,
                &mut page_breaks,
                &mut image_sources,
                &media_by_relationship,
                child,
                body_paragraph_index,
                display_source,
            )?;
        } else if child.name.eq_ignore_ascii_case("tbl") {
            let tail = body_children.get(body_index + 1).copied().filter(|node| {
                node.name.eq_ignore_ascii_case("p")
                    && node.first_descendant("imagedata").is_none()
                    && node.first_descendant("blip").is_none()
                    && !paragraph_has_page_break(node)
            });
            if tail.is_some() {
                body_paragraph_index += 1;
                body_index += 1;
            }
            append_word_table(
                &mut document,
                &mut image_sources,
                &media_by_relationship,
                child,
                tail,
                display_source,
            )?;
        }
        body_index += 1;
    }

    document.summary = HmlSummary {
        section_count: document.sections.len() as u64,
        top_level_paragraph_count: document.paragraphs.len() as u64,
        content_paragraph_count: document
            .paragraphs
            .iter()
            .filter(|paragraph| paragraph.marker.is_none())
            .count() as u64,
        table_count: document.tables.len() as u64,
        row_count: document
            .tables
            .iter()
            .map(|table| u64::from(table.row_count))
            .sum(),
        cell_count: document.cells.len() as u64,
        image_count: document.images.len() as u64,
    };
    Ok((document, page_breaks, image_sources))
}

#[allow(clippy::too_many_arguments)]
fn append_word_paragraph(
    document: &mut HmlDocument,
    current_section: &mut Option<String>,
    page_breaks: &mut Vec<DocxPageBreak>,
    image_sources: &mut Vec<DocxImageSource>,
    media: &BTreeMap<&str, &DocxMediaAsset>,
    paragraph: &WordNode,
    body_paragraph_index: u32,
    source: &str,
) -> Result<(), CoreError> {
    let draft = parse_word_paragraph(paragraph, source)?;
    let paragraph_id = format!("paragraph:{}", document.paragraphs.len() + 1);
    let (inlines, image_ids) = materialize_docx_inlines(
        document,
        image_sources,
        media,
        &paragraph_id,
        None,
        draft.inlines,
        source,
    )?;
    let text = hml_inline_text(&inlines);
    let marker = section_marker(&text);
    let section_id = if let Some(title) = marker.as_ref() {
        let index = document.sections.len() as u32 + 1;
        let id = format!("section:{index}");
        document.sections.push(HmlSection {
            id: id.clone(),
            index,
            title: title.clone(),
            kind: section_kind(title).to_string(),
            level: section_level(title),
            paragraph_ids: vec![paragraph_id.clone()],
        });
        *current_section = Some(id.clone());
        Some(id)
    } else {
        if let Some(section) = document.sections.last_mut() {
            section.paragraph_ids.push(paragraph_id.clone());
        }
        current_section.clone()
    };
    let lspace = paragraph_lspace(&draft.align, draft.indent_raw);
    document.paragraphs.push(HmlParagraph {
        id: paragraph_id.clone(),
        section_id,
        marker,
        text,
        tail_text: String::new(),
        align: draft.align,
        indent_raw: draft.indent_raw,
        lmargin_raw: 0,
        rmargin_raw: 0,
        lspace,
        page_break: false,
        table_id: None,
        image_ids,
        inlines,
        source_line_bytes: Vec::new(),
        number: None,
    });
    if draft.page_break {
        page_breaks.push(DocxPageBreak {
            body_paragraph_index,
            semantic_paragraph_id: Some(paragraph_id),
        });
    }
    Ok(())
}

fn append_word_table(
    document: &mut HmlDocument,
    image_sources: &mut Vec<DocxImageSource>,
    media: &BTreeMap<&str, &DocxMediaAsset>,
    table: &WordNode,
    tail: Option<&WordNode>,
    source: &str,
) -> Result<(), CoreError> {
    let grid = table
        .first_child("tblGrid")
        .ok_or_else(|| table_error(source))?;
    let mut widths = Vec::new();
    for column in grid
        .child_elements()
        .filter(|node| node.name.eq_ignore_ascii_case("gridCol"))
    {
        let dxa = column
            .attribute("w")
            .ok_or_else(|| table_error(source))?
            .parse::<u32>()
            .map_err(|_| table_error(source))?;
        widths.push(dxa);
    }
    if widths.is_empty() {
        return Err(table_error(source));
    }
    let rows: Vec<_> = table
        .child_elements()
        .filter(|node| node.name.eq_ignore_ascii_case("tr"))
        .collect();
    if rows.is_empty() {
        return Err(table_error(source));
    }
    let occurrence = document.tables.len() as u32 + 1;
    let table_id = format!("table:{occurrence}");
    let table_paragraph_id = format!("paragraph:{}", document.paragraphs.len() + 1);
    let mut cell_ids = Vec::new();
    let mut vertical_anchors: Vec<Option<usize>> = vec![None; widths.len()];

    for (row_index, row) in rows.iter().enumerate() {
        let mut column = 0_usize;
        let mut stored_occurrence = 0_u32;
        for cell in row
            .child_elements()
            .filter(|node| node.name.eq_ignore_ascii_case("tc"))
        {
            let properties = cell.first_child("tcPr");
            let colspan = properties
                .and_then(|node| node.first_child("gridSpan"))
                .and_then(|node| node.attribute("val"))
                .map(|value| value.parse::<usize>().map_err(|_| table_error(source)))
                .transpose()?
                .unwrap_or(1);
            if colspan == 0 || column + colspan > widths.len() {
                return Err(table_error(source));
            }
            let vertical_merge = properties.and_then(|node| node.first_child("vMerge"));
            let merge_continuation = vertical_merge.is_some_and(|node| {
                !node
                    .attribute("val")
                    .is_some_and(|value| value.eq_ignore_ascii_case("restart"))
            });
            let merge_restart = vertical_merge.is_some() && !merge_continuation;
            stored_occurrence += 1;
            let cell_id = format!("cell:{occurrence}:{}:{stored_occurrence}", row_index + 1);
            let mut seeds = Vec::new();
            let mut align = "justified".to_string();
            for paragraph in cell
                .child_elements()
                .filter(|node| node.name.eq_ignore_ascii_case("p"))
            {
                let draft = parse_word_paragraph(paragraph, source)?;
                if draft.align != "justified" {
                    align = draft.align;
                }
                seeds.extend(draft.inlines);
            }
            let (inlines, image_ids) = materialize_docx_inlines(
                document,
                image_sources,
                media,
                &table_paragraph_id,
                Some(&cell_id),
                seeds,
                source,
            )?;
            let cell_index = document.cells.len();
            document.cells.push(HmlCell {
                id: cell_id.clone(),
                table_id: table_id.clone(),
                row: u32::try_from(row_index + 1).map_err(|_| table_error(source))?,
                occurrence: stored_occurrence,
                text: hml_inline_text(&inlines),
                start_raw: None,
                width_raw: Some(widths[column..column + colspan].iter().sum()),
                hwidth_raw: None,
                column_begin: Some(u32::try_from(column + 1).map_err(|_| table_error(source))?),
                row_begin: Some(u32::try_from(row_index + 1).map_err(|_| table_error(source))?),
                rowspan: 1,
                colspan: u32::try_from(colspan).map_err(|_| table_error(source))?,
                merge_continuation,
                align,
                indent_raw: 0,
                lspace: 280,
                image_ids,
                inlines,
            });
            if merge_continuation {
                let anchors: BTreeSet<_> = vertical_anchors[column..column + colspan]
                    .iter()
                    .copied()
                    .collect();
                if anchors.contains(&None) || anchors.len() != 1 {
                    return Err(table_error(source));
                }
                let anchor = anchors.into_iter().next().flatten().expect("checked");
                document.cells[anchor].rowspan += 1;
            } else if merge_restart {
                for slot in &mut vertical_anchors[column..column + colspan] {
                    *slot = Some(cell_index);
                }
            } else {
                for slot in &mut vertical_anchors[column..column + colspan] {
                    *slot = None;
                }
            }
            cell_ids.push(cell_id);
            column += colspan;
        }
        if column != widths.len() {
            return Err(table_error(source));
        }
    }

    let tail_draft = tail
        .map(|paragraph| parse_word_paragraph(paragraph, source))
        .transpose()?;
    let (tail_inlines, tail_image_ids) = materialize_docx_inlines(
        document,
        image_sources,
        media,
        &table_paragraph_id,
        None,
        tail_draft.map(|draft| draft.inlines).unwrap_or_default(),
        source,
    )?;
    let tail_text = hml_inline_text(&tail_inlines);
    let table_indent = table
        .first_child("tblPr")
        .and_then(|node| node.first_child("tblInd"))
        .and_then(|node| node.attribute("w"))
        .map(|value| value.parse::<i32>().map_err(|_| table_error(source)))
        .transpose()?
        .unwrap_or(1000);
    let section_id = document.sections.last().map(|section| section.id.clone());
    if let Some(section) = document.sections.last_mut() {
        section.paragraph_ids.push(table_paragraph_id.clone());
    }
    document.paragraphs.push(HmlParagraph {
        id: table_paragraph_id.clone(),
        section_id,
        marker: None,
        text: tail_text.clone(),
        tail_text,
        align: "justified".to_string(),
        indent_raw: table_indent,
        lmargin_raw: 0,
        rmargin_raw: 0,
        lspace: 280,
        page_break: false,
        table_id: Some(table_id.clone()),
        image_ids: tail_image_ids,
        inlines: tail_inlines,
        source_line_bytes: Vec::new(),
        number: None,
    });
    document.tables.push(HmlTable {
        id: table_id,
        occurrence,
        paragraph_id: table_paragraph_id,
        max_columns: u32::try_from(widths.len()).map_err(|_| table_error(source))?,
        row_count: u32::try_from(rows.len()).map_err(|_| table_error(source))?,
        cell_ids,
    });
    Ok(())
}

/// 문단 줄간격. 원본 저장본에서 유도한 규칙이며 문단 제목을 이름으로 보지 않는다.
///
/// 네이티브 저장본: 줄간격 160 문단모양(가운데, 들여쓰기 0)을 쓰는 문단은
/// 정확히 8건이고, DOCX 에서 `w:jc=center` 이면서 `w:ind firstLine` 이 없는 문단 8건과 1:1
/// 대응한다. 가운데 정렬이지만 첫줄 들여쓰기가 있는 도면 문단 68건은 280 이다. 나머지 문단은
/// 정렬과 무관하게 전부 280 이다 (좌측 147건, 양쪽 325건).
fn paragraph_lspace(align: &str, indent_raw: i32) -> u32 {
    if align == "center" && indent_raw == 0 {
        160
    } else {
        280
    }
}

fn parse_word_paragraph(paragraph: &WordNode, source: &str) -> Result<ParagraphDraft, CoreError> {
    let properties = paragraph.first_child("pPr");
    let align = properties
        .and_then(|node| node.first_child("jc"))
        .and_then(|node| node.attribute("val"))
        .map_or_else(|| "justified".to_string(), normalize_word_align);
    let indent_raw = properties
        .and_then(|node| node.first_child("ind"))
        .and_then(|node| node.attribute("firstline"))
        .map(|value| {
            value
                .parse::<i32>()
                .map(|dxa| (dxa * 5 + 2) / 4)
                .map_err(|_| document_error(source))
        })
        .transpose()?
        .unwrap_or(0);
    let page_break = paragraph_has_page_break(paragraph);
    let mut inlines = Vec::new();
    collect_paragraph_runs(paragraph, &mut inlines, source)?;
    Ok(ParagraphDraft {
        inlines,
        align,
        indent_raw,
        page_break,
    })
}

fn collect_paragraph_runs(
    node: &WordNode,
    inlines: &mut Vec<DocxInlineSeed>,
    source: &str,
) -> Result<(), CoreError> {
    for child in node.child_elements() {
        match child.name.to_ascii_lowercase().as_str() {
            "ppr" | "tbl" => {}
            "r" => parse_word_run(child, inlines, source)?,
            "ins" | "del" => return Err(document_error(source)),
            _ => collect_paragraph_runs(child, inlines, source)?,
        }
    }
    Ok(())
}

fn parse_word_run(
    run: &WordNode,
    inlines: &mut Vec<DocxInlineSeed>,
    source: &str,
) -> Result<(), CoreError> {
    let formats = word_run_formats(run);
    let size = word_run_size(run);
    collect_word_run_content(run, &formats, size, inlines, source)
}

fn collect_word_run_content(
    node: &WordNode,
    formats: &[CharacterFormat],
    size: Option<u32>,
    inlines: &mut Vec<DocxInlineSeed>,
    source: &str,
) -> Result<(), CoreError> {
    for child in node.child_elements() {
        match child.name.to_ascii_lowercase().as_str() {
            "rpr" | "br" | "lastrenderedpagebreak" | "instrtext" => {}
            "t" => push_docx_text(inlines, child.text(), formats.to_vec(), size),
            "tab" => push_docx_text(inlines, "\t".to_string(), formats.to_vec(), size),
            "pict" | "drawing" | "object" => {
                let mut image_nodes = Vec::new();
                child.descendants_named("imagedata", &mut image_nodes);
                child.descendants_named("blip", &mut image_nodes);
                for image in image_nodes {
                    let relationship_id = image
                        .attribute("id")
                        .or_else(|| image.attribute("embed"))
                        .ok_or_else(|| document_error(source))?;
                    inlines.push(DocxInlineSeed::Image {
                        relationship_id: relationship_id.to_string(),
                    });
                }
            }
            _ => collect_word_run_content(child, formats, size, inlines, source)?,
        }
    }
    Ok(())
}

fn word_run_formats(run: &WordNode) -> Vec<CharacterFormat> {
    let Some(properties) = run.first_child("rPr") else {
        return Vec::new();
    };
    let mut formats = Vec::new();
    if properties.first_child("u").is_some_and(property_is_on) {
        formats.push(CharacterFormat::Underline);
    }
    if properties.first_child("b").is_some_and(property_is_on) {
        formats.push(CharacterFormat::Bold);
    }
    if properties.first_child("i").is_some_and(property_is_on) {
        formats.push(CharacterFormat::Italic);
    }
    if let Some(align) = properties
        .first_child("vertAlign")
        .and_then(|node| node.attribute("val"))
    {
        if align.eq_ignore_ascii_case("superscript") {
            formats.push(CharacterFormat::Superscript);
        } else if align.eq_ignore_ascii_case("subscript") {
            formats.push(CharacterFormat::Subscript);
        }
    }
    formats
}

/// 런이 명시한 글자 크기(하프포인트 `w:sz`)를 포인트로 준다.
///
/// 네이티브 저장본: 통특기는 크기를 명시하지 않은
/// 런을 font=12 로, `<w:sz w:val="20"/>`(=10pt)를 단 표 셀 런을 font=10 문자모양
/// (attr=0/font=10, attr=2/font=10)으로 기록한다. 크기 없는 런은 `None` 으로 두어
/// 방언 기본값이 그대로 쓰이게 한다. 글자 없는 이미지 런의 `w:sz w:val="2"` 는
/// 텍스트를 만들지 않으므로 문자모양이 되지 않는다.
fn word_run_size(run: &WordNode) -> Option<u32> {
    let value = run
        .first_child("rPr")?
        .first_child("sz")?
        .attribute("val")?
        .parse::<u32>()
        .ok()?;
    Some(value.div_ceil(2))
}

fn property_is_on(node: &WordNode) -> bool {
    !node.attribute("val").is_some_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "none"
        )
    })
}

fn paragraph_has_page_break(paragraph: &WordNode) -> bool {
    let mut breaks = Vec::new();
    paragraph.descendants_named("br", &mut breaks);
    // 워드 문단 속성 「앞에서 쪽 나눔」(`w:pageBreakBefore` · `w:val="0|false"` 이면 끈 것).
    let mut before = Vec::new();
    paragraph.descendants_named("pageBreakBefore", &mut before);
    breaks.into_iter().any(|node| {
        node.attribute("type")
            .is_some_and(|value| value.eq_ignore_ascii_case("page"))
    }) || before.into_iter().any(|node| {
        !node
            .attribute("val")
            .is_some_and(|value| matches!(value, "0" | "false" | "off"))
    })
}

fn materialize_docx_inlines(
    document: &mut HmlDocument,
    image_sources: &mut Vec<DocxImageSource>,
    media: &BTreeMap<&str, &DocxMediaAsset>,
    paragraph_id: &str,
    cell_id: Option<&str>,
    seeds: Vec<DocxInlineSeed>,
    source: &str,
) -> Result<(Vec<HmlInline>, Vec<String>), CoreError> {
    let mut inlines = Vec::new();
    let mut image_ids = Vec::new();
    for seed in seeds {
        match seed {
            DocxInlineSeed::Text {
                text,
                formats,
                size,
            } => {
                push_hml_text(&mut inlines, text, formats, size);
            }
            DocxInlineSeed::Image { relationship_id } => {
                let asset = media
                    .get(relationship_id.as_str())
                    .ok_or_else(|| relationships_error(source))?;
                let occurrence = document.images.len() as u32;
                let image_id = format!("image:{occurrence}");
                let extension = Path::new(&asset.media_name)
                    .extension()
                    .and_then(|value| value.to_str())
                    .map(|value| value.to_ascii_lowercase())
                    .unwrap_or_else(|| "bin".to_string());
                let extension = if extension == "jpeg" {
                    "jpg".to_string()
                } else {
                    extension
                };
                let (width_mm, height_mm) = fit_image_mm(asset.pixel_width, asset.pixel_height)?;
                let width = image_display_units(width_mm);
                let height = image_display_units(height_mm);
                document.images.push(HmlImage {
                    id: image_id.clone(),
                    occurrence,
                    name: format!("pct{:05}.{extension}", occurrence + 1),
                    width: Some(width),
                    height: Some(height),
                    secondary_width: Some(secondary_image_units(width)),
                    secondary_height: Some(secondary_image_units(height)),
                    paragraph_id: Some(paragraph_id.to_string()),
                    cell_id: cell_id.map(str::to_string),
                });
                image_sources.push(DocxImageSource {
                    image_id: image_id.clone(),
                    relationship_id: asset.relationship_id.clone(),
                    target: asset.target.clone(),
                });
                image_ids.push(image_id.clone());
                inlines.push(HmlInline::Image { image_id });
            }
        }
    }
    Ok((inlines, image_ids))
}

fn fit_image_mm(pixel_width: u32, pixel_height: u32) -> Result<(u32, u32), CoreError> {
    if pixel_width == 0 || pixel_height == 0 {
        return Err(CoreError::new(
            ErrorCode::GeometryInvalid,
            "docx_image",
            false,
        ));
    }
    let scale = (152.0 / f64::from(pixel_width)).min(220.0 / f64::from(pixel_height));
    Ok((
        (f64::from(pixel_width) * scale).round() as u32,
        (f64::from(pixel_height) * scale).round() as u32,
    ))
}

fn image_display_units(mm: u32) -> u32 {
    (mm * 1000 + 141) / 282
}

fn secondary_image_units(primary: u32) -> u32 {
    (primary * 3 + 2) / 4
}

fn push_docx_text(
    inlines: &mut Vec<DocxInlineSeed>,
    text: String,
    formats: Vec<CharacterFormat>,
    size: Option<u32>,
) {
    if text.is_empty() {
        return;
    }
    if let Some(DocxInlineSeed::Text {
        text: previous,
        formats: previous_formats,
        size: previous_size,
    }) = inlines.last_mut()
        && *previous_formats == formats
        && *previous_size == size
    {
        previous.push_str(&text);
        return;
    }
    inlines.push(DocxInlineSeed::Text {
        text,
        formats,
        size,
    });
}

fn push_hml_text(
    inlines: &mut Vec<HmlInline>,
    text: String,
    formats: Vec<CharacterFormat>,
    size: Option<u32>,
) {
    if let Some(HmlInline::Text {
        text: previous,
        formats: previous_formats,
        size: previous_size,
    }) = inlines.last_mut()
        && *previous_formats == formats
        && *previous_size == size
    {
        previous.push_str(&text);
        return;
    }
    inlines.push(HmlInline::Text {
        text,
        formats,
        size,
    });
}

fn hml_inline_text(inlines: &[HmlInline]) -> String {
    inlines
        .iter()
        .filter_map(|inline| match inline {
            HmlInline::Text { text, .. } => Some(text.as_str()),
            HmlInline::Image { .. } => None,
        })
        .collect()
}

fn section_marker(text: &str) -> Option<String> {
    let text = text.trim();
    (text.starts_with('【') && text.ends_with('】')).then(|| text.to_string())
}

fn section_kind(title: &str) -> &'static str {
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

fn section_level(title: &str) -> u32 {
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

fn normalize_word_align(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "left" => "left",
        "center" => "center",
        "right" => "right",
        "both" | "distribute" | "thai_distribute" => "justified",
        _ => "justified",
    }
    .to_string()
}

fn empty_hml_summary() -> HmlSummary {
    HmlSummary {
        section_count: 0,
        top_level_paragraph_count: 0,
        content_paragraph_count: 0,
        table_count: 0,
        row_count: 0,
        cell_count: 0,
        image_count: 0,
    }
}

fn parse_word_xml(bytes: &[u8], source: &str) -> Result<WordNode, CoreError> {
    let text = std::str::from_utf8(bytes).map_err(|_| document_error(source))?;
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(false);
    let mut stack = Vec::new();
    let mut root = None;
    loop {
        match reader.read_event().map_err(|_| document_error(source))? {
            Event::Start(start) => stack.push(word_node(&reader, &start, source)?),
            Event::Empty(start) => {
                let node = word_node(&reader, &start, source)?;
                append_word_node(&mut stack, &mut root, node, source)?;
            }
            Event::End(_) => {
                let node = stack.pop().ok_or_else(|| document_error(source))?;
                append_word_node(&mut stack, &mut root, node, source)?;
            }
            Event::Text(value) => {
                if let Some(parent) = stack.last_mut() {
                    let decoded = value.decode().map_err(|_| document_error(source))?;
                    let unescaped = quick_xml::escape::unescape(&decoded)
                        .map_err(|_| document_error(source))?;
                    parent
                        .content
                        .push(WordContent::Text(unescaped.into_owned()));
                }
            }
            Event::CData(value) => {
                if let Some(parent) = stack.last_mut() {
                    parent.content.push(WordContent::Text(
                        value
                            .decode()
                            .map_err(|_| document_error(source))?
                            .into_owned(),
                    ));
                }
            }
            Event::GeneralRef(reference) => {
                if let Some(parent) = stack.last_mut() {
                    let name = reference.decode().map_err(|_| document_error(source))?;
                    let character =
                        resolve_xml_reference(&name).ok_or_else(|| document_error(source))?;
                    parent
                        .content
                        .push(WordContent::Text(character.to_string()));
                }
            }
            Event::Eof => break,
            Event::Decl(_) | Event::PI(_) | Event::Comment(_) | Event::DocType(_) => {}
        }
    }
    if !stack.is_empty() {
        return Err(document_error(source));
    }
    root.ok_or_else(|| document_error(source))
}

fn word_node(
    reader: &Reader<&[u8]>,
    start: &quick_xml::events::BytesStart<'_>,
    source: &str,
) -> Result<WordNode, CoreError> {
    let raw_name = start.name();
    let qualified = std::str::from_utf8(raw_name.as_ref()).map_err(|_| document_error(source))?;
    let name = qualified
        .rsplit_once(':')
        .map_or(qualified, |(_, local)| local)
        .to_string();
    let mut attributes = BTreeMap::new();
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|_| document_error(source))?;
        let qualified =
            std::str::from_utf8(attribute.key.as_ref()).map_err(|_| document_error(source))?;
        let key = qualified
            .rsplit_once(':')
            .map_or(qualified, |(_, local)| local)
            .to_ascii_lowercase();
        let value = attribute
            .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder())
            .map_err(|_| document_error(source))?
            .into_owned();
        attributes.insert(key, value);
    }
    Ok(WordNode {
        name,
        attributes,
        content: Vec::new(),
    })
}

fn append_word_node(
    stack: &mut [WordNode],
    root: &mut Option<WordNode>,
    node: WordNode,
    source: &str,
) -> Result<(), CoreError> {
    if let Some(parent) = stack.last_mut() {
        parent.content.push(WordContent::Element(node));
    } else if root.is_none() {
        *root = Some(node);
    } else {
        return Err(document_error(source));
    }
    Ok(())
}

fn resolve_xml_reference(name: &str) -> Option<char> {
    match name {
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
    }
}

fn document_error(source: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::StructureInvalid, "docx_document", false);
    error.source = safe_source(source);
    error
}

fn table_error(source: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::GeometryInvalid, "docx_table", false);
    error.source = safe_source(source);
    error
}

fn index_parts(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    source: &str,
) -> Result<BTreeMap<String, PartInfo>, CoreError> {
    let mut parts = BTreeMap::new();
    let mut folded = BTreeSet::new();
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(|_| package_error(source))?;
        if file.encrypted() {
            return Err(package_error(source));
        }
        let name = std::str::from_utf8(file.name_raw()).map_err(|_| package_error(source))?;
        validate_part_name(name, source)?;
        if file.is_dir() {
            continue;
        }
        let key = name.to_ascii_lowercase();
        if !folded.insert(key) || parts.contains_key(name) {
            return Err(duplicate_part_error(source));
        }
        parts.insert(
            name.to_string(),
            PartInfo {
                index,
                size: file.size(),
            },
        );
    }
    Ok(parts)
}

fn validate_part_name(name: &str, source: &str) -> Result<(), CoreError> {
    let unsafe_name = name.is_empty()
        || name.starts_with('/')
        || name.starts_with('\\')
        || name.contains('\\')
        || name.contains(':')
        || name
            .split('/')
            .any(|segment| segment == ".." || segment.is_empty());
    if unsafe_name {
        let mut error = CoreError::new(ErrorCode::PathNotAllowed, "docx_package", false);
        error.source = safe_source(source);
        return Err(error);
    }
    Ok(())
}

fn required_part<'a>(
    parts: &'a BTreeMap<String, PartInfo>,
    name: &str,
    code: ErrorCode,
    source: &str,
) -> Result<&'a PartInfo, CoreError> {
    parts.get(name).ok_or_else(|| {
        let mut error = CoreError::new(code, "docx_package", false);
        error.source = safe_source(source);
        error
    })
}

fn read_part(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    part: &PartInfo,
    source: &str,
) -> Result<Vec<u8>, CoreError> {
    let mut file = archive
        .by_index(part.index)
        .map_err(|_| package_error(source))?;
    let capacity = usize::try_from(part.size).map_err(|_| part_limit_error(source))?;
    let mut bytes = Vec::with_capacity(capacity);
    file.read_to_end(&mut bytes)
        .map_err(|error| io_error(&error, "docx_package", source))?;
    if bytes.len() as u64 != part.size {
        return Err(package_error(source));
    }
    Ok(bytes)
}

fn parse_image_relationships(bytes: &[u8], source: &str) -> Result<Vec<Relationship>, CoreError> {
    let text = std::str::from_utf8(bytes).map_err(|_| relationships_error(source))?;
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut relationships = Vec::new();
    loop {
        match reader
            .read_event()
            .map_err(|_| relationships_error(source))?
        {
            Event::Start(start) | Event::Empty(start)
                if start.name().as_ref().eq_ignore_ascii_case(b"Relationship") =>
            {
                let mut attributes = BTreeMap::new();
                for attribute in start.attributes() {
                    let attribute = attribute.map_err(|_| relationships_error(source))?;
                    let key = std::str::from_utf8(attribute.key.as_ref())
                        .map_err(|_| relationships_error(source))?
                        .to_ascii_lowercase();
                    let value = attribute
                        .decoded_and_normalized_value(
                            quick_xml::XmlVersion::Implicit1_0,
                            reader.decoder(),
                        )
                        .map_err(|_| relationships_error(source))?
                        .into_owned();
                    attributes.insert(key, value);
                }
                let relationship_type = attributes.get("type").cloned().unwrap_or_default();
                let external = attributes
                    .get("targetmode")
                    .is_some_and(|value| value.eq_ignore_ascii_case("external"));
                if relationship_type.ends_with("/image") && !external {
                    relationships.push(Relationship {
                        id: attributes
                            .get("id")
                            .cloned()
                            .ok_or_else(|| relationships_error(source))?,
                        target: attributes
                            .get("target")
                            .cloned()
                            .ok_or_else(|| relationships_error(source))?,
                    });
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(relationships)
}

fn normalize_relationship_target(target: &str, source: &str) -> Result<String, CoreError> {
    if target.is_empty()
        || target.starts_with('/')
        || target.starts_with('\\')
        || target.contains('\\')
        || target.contains(':')
        || target
            .split('/')
            .any(|segment| segment == ".." || segment.is_empty())
    {
        return Err(relationships_error(source));
    }
    Ok(format!("word/{target}"))
}

fn package_error(source: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::UnsupportedFormat, "docx_package", false);
    error.source = safe_source(source);
    error
}

fn duplicate_part_error(source: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::StructureInvalid, "docx_package", false);
    error.source = safe_source(source);
    error
}

fn relationships_error(source: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::StructureInvalid, "docx_relationships", false);
    error.source = safe_source(source);
    error
}

fn part_limit_error(source: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::SizeLimitExceeded, "docx_package", false);
    error.source = safe_source(source);
    error
}
