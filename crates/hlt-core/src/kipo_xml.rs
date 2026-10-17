use std::{borrow::Cow, collections::BTreeMap, fs, path::Path};

use encoding_rs::{EUC_KR, UTF_8, UTF_16BE, UTF_16LE};
use quick_xml::{Reader, events::Event};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    CharacterFormat, HmlCell, HmlDocument, HmlImage, HmlInline, HmlParagraph, HmlSection,
    HmlSummary, HmlTable,
    error::{CoreError, ErrorCode, io_error, safe_source},
};

const MAX_KIPO_XML_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KipoXmlDialect {
    PctPatentCafdoc,
    PatentCafdoc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KipoXmlDocument {
    pub source: String,
    pub source_sha256: String,
    pub input_encoding: String,
    pub document_root: String,
    pub dialect: KipoXmlDialect,
    pub invention_title: String,
    pub document: HmlDocument,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum XmlContent {
    Text(String),
    Element(XmlNode),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct XmlNode {
    pub(crate) name: String,
    pub(crate) attributes: BTreeMap<String, String>,
    pub(crate) content: Vec<XmlContent>,
}

impl XmlNode {
    fn first_child(&self, name: &str) -> Option<&Self> {
        self.content.iter().find_map(|item| match item {
            XmlContent::Element(node) if node.name.eq_ignore_ascii_case(name) => Some(node),
            _ => None,
        })
    }

    fn first_descendant(&self, name: &str) -> Option<&Self> {
        if self.name.eq_ignore_ascii_case(name) {
            return Some(self);
        }
        self.content.iter().find_map(|item| match item {
            XmlContent::Element(node) => node.first_descendant(name),
            XmlContent::Text(_) => None,
        })
    }

    fn descendants_named<'a>(&'a self, name: &str, output: &mut Vec<&'a Self>) {
        for child in self.child_elements() {
            if child.name.eq_ignore_ascii_case(name) {
                output.push(child);
            }
            child.descendants_named(name, output);
        }
    }

    fn child_elements(&self) -> impl Iterator<Item = &Self> {
        self.content.iter().filter_map(|item| match item {
            XmlContent::Element(node) => Some(node),
            XmlContent::Text(_) => None,
        })
    }

    fn text(&self) -> String {
        let mut output = String::new();
        self.push_text(&mut output);
        output
    }

    fn push_text(&self, output: &mut String) {
        for item in &self.content {
            match item {
                XmlContent::Text(text) => output.push_str(text),
                XmlContent::Element(node) => node.push_text(output),
            }
        }
    }
}

pub fn inspect_kipo_xml_path(path: &Path) -> Result<KipoXmlDocument, CoreError> {
    let source_text = path.to_string_lossy();
    let metadata =
        fs::metadata(path).map_err(|error| io_error(&error, "kipo_xml_read", &source_text))?;
    if metadata.len() > MAX_KIPO_XML_BYTES {
        let mut error = CoreError::new(ErrorCode::SizeLimitExceeded, "kipo_xml_read", false);
        error.source = safe_source(&source_text);
        error.expected = Some(MAX_KIPO_XML_BYTES.to_string());
        error.actual = Some(metadata.len().to_string());
        return Err(error);
    }
    let bytes = fs::read(path).map_err(|error| io_error(&error, "kipo_xml_read", &source_text))?;
    let source_sha256 = format!("{:X}", Sha256::digest(&bytes));
    let (decoded, input_encoding) = decode_xml(&bytes, &source_text)?;
    let root = parse_xml(decoded.as_ref(), &source_text)?;
    if !root.name.eq_ignore_ascii_case("KIPO") {
        return Err(dialect_error(&source_text));
    }

    let (document_root, dialect, patent) = if let Some(node) = root.first_child("PCTPatentCAFDOC") {
        (
            "PCTPatentCAFDOC".to_string(),
            KipoXmlDialect::PctPatentCafdoc,
            node,
        )
    } else if let Some(node) = root.first_child("PatentCAFDOC") {
        (
            "PatentCAFDOC".to_string(),
            KipoXmlDialect::PatentCafdoc,
            node,
        )
    } else {
        return Err(dialect_error(&source_text));
    };

    let source = safe_source(&source_text).unwrap_or_default();
    let invention_title = patent
        .first_descendant("invention-title")
        .map_or_else(String::new, |node| node.text().trim().to_string());
    let document = normalize_patent(
        patent,
        dialect,
        source.clone(),
        source_sha256.clone(),
        input_encoding.clone(),
    )?;

    Ok(KipoXmlDocument {
        source,
        source_sha256,
        input_encoding,
        document_root,
        dialect,
        invention_title,
        document,
    })
}

#[derive(Debug, Clone)]
enum InlineSeed {
    Text {
        text: String,
        formats: Vec<CharacterFormat>,
    },
    Image {
        name: String,
        width_mm: Option<u32>,
        height_mm: Option<u32>,
    },
}

struct SemanticBuilder {
    document: HmlDocument,
    current_section: Option<String>,
    dialect: KipoXmlDialect,
}

impl SemanticBuilder {
    fn new(
        dialect: KipoXmlDialect,
        source: String,
        source_sha256: String,
        encoding: String,
    ) -> Self {
        let title = match dialect {
            KipoXmlDialect::PctPatentCafdoc => "특허 출원서의 명세서등 서식".to_string(),
            KipoXmlDialect::PatentCafdoc => String::new(),
        };
        Self {
            document: HmlDocument {
                source,
                source_sha256,
                encoding,
                title,
                fonts: vec!["신명조".to_string(); 3],
                summary: empty_summary(),
                sections: Vec::new(),
                paragraphs: Vec::new(),
                tables: Vec::new(),
                cells: Vec::new(),
                images: Vec::new(),
            },
            current_section: None,
            dialect,
        }
    }

    fn emit_section(&mut self, title: &str) {
        let section_index = self.document.sections.len() as u32 + 1;
        let section_id = format!("section:{section_index}");
        let paragraph_id = format!("paragraph:{}", self.document.paragraphs.len() + 1);
        let align = if matches!(
            title,
            "【발명의 설명】" | "【청구범위】" | "【요약서】" | "【도면】"
        ) {
            "center"
        } else {
            "justified"
        };
        let level = section_level(title);
        self.document.sections.push(HmlSection {
            id: section_id.clone(),
            index: section_index,
            title: title.to_string(),
            kind: section_kind(title).to_string(),
            level,
            paragraph_ids: vec![paragraph_id.clone()],
        });
        self.document.paragraphs.push(HmlParagraph {
            id: paragraph_id,
            section_id: Some(section_id.clone()),
            marker: Some(title.to_string()),
            text: title.to_string(),
            tail_text: String::new(),
            align: align.to_string(),
            indent_raw: 0,
            lmargin_raw: 0,
            rmargin_raw: 0,
            lspace: 280,
            page_break: false,
            table_id: None,
            image_ids: Vec::new(),
            inlines: vec![HmlInline::Text {
                text: title.to_string(),
                formats: Vec::new(),
                size: None,
            }],
            source_line_bytes: Vec::new(),
            number: None,
        });
        self.current_section = Some(section_id);
    }

    fn emit_plain(&mut self, text: String, align: &str, indent_raw: i32, lspace: u32) {
        self.emit_seeds(
            vec![InlineSeed::Text {
                text,
                formats: Vec::new(),
            }],
            align,
            indent_raw,
            lspace,
        );
    }

    fn emit_seeds(&mut self, seeds: Vec<InlineSeed>, align: &str, indent_raw: i32, lspace: u32) {
        let visible = seed_text(&seeds);
        let has_image = seeds
            .iter()
            .any(|seed| matches!(seed, InlineSeed::Image { .. }));
        if visible.trim().is_empty() && !has_image {
            return;
        }

        let paragraph_id = format!("paragraph:{}", self.document.paragraphs.len() + 1);
        let mut inlines = Vec::new();
        let mut image_ids = Vec::new();
        for seed in seeds {
            match seed {
                InlineSeed::Text { text, formats } => {
                    push_inline_text(&mut inlines, text, formats);
                }
                InlineSeed::Image {
                    name,
                    width_mm,
                    height_mm,
                } => {
                    let occurrence = self.document.images.len() as u32;
                    let image_id = format!("image:{occurrence}");
                    let width = width_mm.map(image_display_units);
                    let height = height_mm.map(image_display_units);
                    self.document.images.push(HmlImage {
                        id: image_id.clone(),
                        occurrence,
                        name,
                        width,
                        height,
                        secondary_width: width.map(secondary_image_units),
                        secondary_height: height.map(secondary_image_units),
                        paragraph_id: Some(paragraph_id.clone()),
                        cell_id: None,
                    });
                    image_ids.push(image_id.clone());
                    inlines.push(HmlInline::Image { image_id });
                }
            }
        }
        let section_id = self.current_section.clone();
        if let Some(section) = self.document.sections.last_mut() {
            section.paragraph_ids.push(paragraph_id.clone());
        }
        self.document.paragraphs.push(HmlParagraph {
            id: paragraph_id,
            section_id,
            marker: None,
            text: inline_text(&inlines),
            tail_text: String::new(),
            align: align.to_string(),
            indent_raw,
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
    }

    fn emit_node_paragraphs(&mut self, node: &XmlNode) -> Result<(), CoreError> {
        let paragraphs: Vec<_> = node
            .child_elements()
            .filter(|child| child.name.eq_ignore_ascii_case("p"))
            .collect();
        let mut paragraph_index = 0_usize;
        while paragraph_index < paragraphs.len() {
            let paragraph = paragraphs[paragraph_index];
            // 식별번호 `<p num="0001">` 은 이 `<p>` 가 만든 첫 본문 문단에 싣는다(표 · 글 · 식 모두 — 표제는 번호가 없다).
            let first_new = self.document.paragraphs.len();
            let number = paragraph.attributes.get("num").cloned();
            if let Some(tables) = paragraph.first_descendant("tables") {
                let mut tail = None;
                if let Some(candidate) = paragraphs.get(paragraph_index + 1)
                    && !contains_any(candidate, &["tables", "img", "br"])
                {
                    tail = Some(*candidate);
                    paragraph_index += 1;
                }
                let table_nodes: Vec<_> = tables
                    .child_elements()
                    .filter(|child| child.name.eq_ignore_ascii_case("table"))
                    .collect();
                let table_count = table_nodes.len();
                for (table_index, table) in table_nodes.into_iter().enumerate() {
                    self.emit_table(
                        table,
                        (table_index + 1 == table_count).then_some(tail).flatten(),
                    )?;
                }
                paragraph_index += 1;
                self.number_first(first_new, number);
                continue;
            }
            // 식 문단: 제출 XML 은 표제 없이 `<p><maths num="1"><img/></maths></p>` 만 싣는다 —
            // 통특기 본문처럼 【수학식 1】 · 【화학식 1】 표제 문단을 앞에 되살린다(제출파일을 열 때 표제가 사라지지 않게).
            for (element, label) in [("maths", "수학식"), ("chemistry", "화학식")] {
                if let Some(number) = paragraph
                    .first_descendant(element)
                    .and_then(|node| node.attributes.get("num"))
                {
                    self.emit_section(&format!("【{label} {number}】"));
                }
            }
            for seeds in inline_segments(paragraph) {
                let visible = seed_text(&seeds).trim().to_string();
                let centered =
                    matches!(visible.as_str(), "관련 출원" | "발명의 분야" | "정부 지원");
                self.emit_seeds(
                    seeds,
                    if centered { "center" } else { "justified" },
                    if centered { 0 } else { 1000 },
                    if centered { 160 } else { 280 },
                );
            }
            self.number_first(first_new, number);
            paragraph_index += 1;
        }
        Ok(())
    }

    /// `from` 뒤로 새로 생긴 문단 가운데 첫 본문 문단(표제 아님)에 식별번호를 단다.
    fn number_first(&mut self, from: usize, number: Option<String>) {
        let from = from.min(self.document.paragraphs.len());
        if let Some(paragraph) = self.document.paragraphs[from..]
            .iter_mut()
            .find(|paragraph| paragraph.marker.is_none())
        {
            paragraph.number = number;
        }
    }

    fn emit_table(&mut self, table: &XmlNode, tail: Option<&XmlNode>) -> Result<(), CoreError> {
        let occurrence = self.document.tables.len() as u32 + 1;
        let table_id = format!("table:{occurrence}");
        let title_text = format!("【표 {occurrence}】");
        // 통특기 실물(pct_a.HLT, KIPO XML 들여오기, 2026-09-07): 표 문단 바로 뒤에 표가 오면 뒤 표의
        // 【표 N】 표제는 제 문단이 아니라 **앞 표 문단의 꼬리 줄**이다(표 1 청크 꼬리 = 「【표 2】」).
        // 절 레지스터 레코드는 그대로 둔다(정본 레지스터에도 【표 1】【표 2】 둘 다 있다).
        // 앞이 글 문단이면(noop_v15 【표 1】) 표제는 제 청크다.
        let hang_on_previous =
            self.document.paragraphs.last().is_some_and(|previous| {
                previous.table_id.is_some() && previous.tail_text.is_empty()
            });
        if hang_on_previous {
            let previous = self
                .document
                .paragraphs
                .last_mut()
                .expect("previous paragraph");
            previous.text = title_text.clone();
            previous.tail_text = title_text.clone();
            previous.inlines = vec![HmlInline::Text {
                text: title_text.clone(),
                formats: Vec::new(),
                size: None,
            }];
            let previous_id = previous.id.clone();
            let section_index = self.document.sections.len() as u32 + 1;
            let section_id = format!("section:{section_index}");
            self.document.sections.push(HmlSection {
                id: section_id.clone(),
                index: section_index,
                title: title_text.clone(),
                kind: section_kind(&title_text).to_string(),
                level: section_level(&title_text),
                paragraph_ids: vec![previous_id],
            });
            self.current_section = Some(section_id);
        } else {
            self.emit_section(&title_text);
        }

        if let Some(title) = table.first_child("title") {
            // 표 제목도 서식을 지닌다: 통특기가 들여온 표 1 제목 줄에 `[EC<sub>50</sub>]` 의
            // 아래첨자 런이 살아 있다. 평문으로 내면 첨자가 사라진다.
            if !title.text().trim().is_empty() {
                for seeds in inline_segments(title) {
                    self.emit_seeds(seeds, "justified", 800, 280);
                }
            }
        }

        let tgroup = table.first_descendant("tgroup").ok_or_else(table_error)?;
        let max_columns = parse_required_u32(tgroup, "cols")?;
        if max_columns == 0 {
            return Err(table_error());
        }
        let max_columns_usize = usize::try_from(max_columns).map_err(|_| table_error())?;
        let mut column_names = Vec::new();
        let mut widths = Vec::new();
        for colspec in tgroup
            .child_elements()
            .filter(|child| child.name.eq_ignore_ascii_case("colspec"))
        {
            column_names.push(
                colspec
                    .attributes
                    .get("colname")
                    .cloned()
                    .unwrap_or_else(|| format!("col{}", column_names.len() + 1)),
            );
            widths.push(parse_leading_u32(colspec, "colwidth")?);
        }
        if column_names.len() > max_columns_usize {
            return Err(table_error());
        }
        if column_names.len() < max_columns_usize {
            let used: u32 = widths.iter().sum();
            let missing = max_columns_usize - column_names.len();
            let fallback = 10_260_u32.saturating_sub(used)
                / u32::try_from(missing).map_err(|_| table_error())?;
            for index in column_names.len()..max_columns_usize {
                column_names.push(format!("col{}", index + 1));
                widths.push(fallback.max(1));
            }
        }
        if widths.contains(&0) {
            return Err(table_error());
        }
        let name_to_index: BTreeMap<_, _> = column_names
            .iter()
            .enumerate()
            .map(|(index, name)| (name.as_str(), index))
            .collect();
        let tbody = tgroup.first_descendant("tbody").unwrap_or(tgroup);
        let rows: Vec<_> = tbody
            .child_elements()
            .filter(|child| child.name.eq_ignore_ascii_case("row"))
            .collect();
        if rows.is_empty() {
            return Err(table_error());
        }

        let table_paragraph_id = format!("paragraph:{}", self.document.paragraphs.len() + 1);
        let default_align = match self.dialect {
            KipoXmlDialect::PctPatentCafdoc => "left",
            KipoXmlDialect::PatentCafdoc => "center",
        };
        let mut vertical_remaining = vec![0_u32; max_columns_usize];
        let mut cell_ids = Vec::new();
        for (row_index, row) in rows.iter().enumerate() {
            let mut slots = vec![None; max_columns_usize];
            for (column, remaining) in vertical_remaining.iter_mut().enumerate() {
                if *remaining > 0 {
                    slots[column] = Some(CellSlot::Continuation);
                    *remaining -= 1;
                }
            }
            let mut cursor = 0_usize;
            for entry in row
                .child_elements()
                .filter(|child| child.name.eq_ignore_ascii_case("entry"))
            {
                let start = if let Some(name) = entry
                    .attributes
                    .get("namest")
                    .or_else(|| entry.attributes.get("colname"))
                {
                    *name_to_index.get(name.as_str()).ok_or_else(table_error)?
                } else {
                    while cursor < max_columns_usize && slots[cursor].is_some() {
                        cursor += 1;
                    }
                    if cursor == max_columns_usize {
                        return Err(table_error());
                    }
                    cursor
                };
                let end = if let Some(name) = entry.attributes.get("nameend") {
                    *name_to_index.get(name.as_str()).ok_or_else(table_error)?
                } else {
                    start
                };
                if end < start
                    || end >= max_columns_usize
                    || slots[start..=end].iter().any(Option::is_some)
                {
                    return Err(table_error());
                }
                slots[start] = Some(CellSlot::Entry { entry, end });
                for slot in &mut slots[start + 1..=end] {
                    *slot = Some(CellSlot::Covered);
                }
                let more_rows = parse_optional_u32(entry, "morerows")?.unwrap_or(0);
                for remaining in &mut vertical_remaining[start..=end] {
                    *remaining = (*remaining).max(more_rows);
                }
                cursor = end + 1;
            }

            let mut stored_occurrence = 0_u32;
            for (column, slot) in slots.into_iter().enumerate() {
                let Some(slot) = slot else {
                    continue;
                };
                let (entry, end, merge_continuation) = match slot {
                    CellSlot::Continuation => (None, column, true),
                    CellSlot::Entry { entry, end } => (Some(entry), end, false),
                    CellSlot::Covered => continue,
                };
                stored_occurrence += 1;
                let cell_id = format!("cell:{occurrence}:{}:{stored_occurrence}", row_index + 1);
                let hwidth_raw = widths[column..=end].iter().sum();
                let align = entry
                    .and_then(|node| node.attributes.get("align"))
                    .and_then(|value| normalized_cell_align(value))
                    .unwrap_or(default_align)
                    .to_string();
                let rowspan = entry
                    .map(|node| parse_optional_u32(node, "morerows"))
                    .transpose()?
                    .flatten()
                    .unwrap_or(0)
                    + 1;
                let seeds = entry.map_or_else(Vec::new, |node| {
                    inline_segments(node).into_iter().flatten().collect()
                });
                let (mut inlines, image_ids) =
                    self.materialize_inlines(&table_paragraph_id, Some(&cell_id), seeds);
                // 통특기 표 셀 글자는 10pt 다(실물 셀 줄 글자 크기 전수: noop_v15·pct_a·pct 전부 {10}, 2026-09-07).
                // 12pt 로 두면 셀 글이 넓어져 통특기가 줄을 더 접고 쪽이 밀린다(noop 인쇄 52쪽 vs 정본 51쪽).
                for inline in &mut inlines {
                    if let HmlInline::Text { size, .. } = inline
                        && size.is_none()
                    {
                        *size = Some(10);
                    }
                }
                let text = inline_text(&inlines);
                self.document.cells.push(HmlCell {
                    id: cell_id.clone(),
                    table_id: table_id.clone(),
                    row: u32::try_from(row_index + 1).map_err(|_| table_error())?,
                    occurrence: stored_occurrence,
                    text,
                    start_raw: None,
                    width_raw: None,
                    hwidth_raw: Some(hwidth_raw),
                    column_begin: Some(u32::try_from(column + 1).map_err(|_| table_error())?),
                    row_begin: Some(u32::try_from(row_index + 1).map_err(|_| table_error())?),
                    rowspan,
                    colspan: u32::try_from(end - column + 1).map_err(|_| table_error())?,
                    merge_continuation,
                    align,
                    indent_raw: 0,
                    lspace: 280,
                    image_ids,
                    inlines,
                });
                cell_ids.push(cell_id);
            }
        }

        let tail_seeds: Vec<_> = tail
            .map(inline_segments)
            .unwrap_or_default()
            .into_iter()
            .flatten()
            .collect();
        let (tail_inlines, tail_image_ids) =
            self.materialize_inlines(&table_paragraph_id, None, tail_seeds);
        let tail_text = inline_text(&tail_inlines);
        let section_id = self.current_section.clone();
        if let Some(section) = self.document.sections.last_mut() {
            section.paragraph_ids.push(table_paragraph_id.clone());
        }
        self.document.paragraphs.push(HmlParagraph {
            id: table_paragraph_id.clone(),
            section_id,
            marker: None,
            text: tail_text.clone(),
            tail_text,
            align: "justified".to_string(),
            indent_raw: parse_optional_i32(table, "hlt-indent")?.unwrap_or(1000),
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
        self.document.tables.push(HmlTable {
            id: table_id,
            occurrence,
            paragraph_id: table_paragraph_id,
            max_columns,
            row_count: u32::try_from(rows.len()).map_err(|_| table_error())?,
            cell_ids,
        });
        Ok(())
    }

    fn materialize_inlines(
        &mut self,
        paragraph_id: &str,
        cell_id: Option<&str>,
        seeds: Vec<InlineSeed>,
    ) -> (Vec<HmlInline>, Vec<String>) {
        let mut inlines = Vec::new();
        let mut image_ids = Vec::new();
        for seed in seeds {
            match seed {
                InlineSeed::Text { text, formats } => {
                    push_inline_text(&mut inlines, text, formats);
                }
                InlineSeed::Image {
                    name,
                    width_mm,
                    height_mm,
                } => {
                    let occurrence = self.document.images.len() as u32;
                    let image_id = format!("image:{occurrence}");
                    let width = width_mm.map(image_display_units);
                    let height = height_mm.map(image_display_units);
                    self.document.images.push(HmlImage {
                        id: image_id.clone(),
                        occurrence,
                        name,
                        width,
                        height,
                        secondary_width: width.map(secondary_image_units),
                        secondary_height: height.map(secondary_image_units),
                        paragraph_id: Some(paragraph_id.to_string()),
                        cell_id: cell_id.map(str::to_string),
                    });
                    image_ids.push(image_id.clone());
                    inlines.push(HmlInline::Image { image_id });
                }
            }
        }
        (inlines, image_ids)
    }

    fn finish(mut self) -> HmlDocument {
        self.document.summary = HmlSummary {
            section_count: self.document.sections.len() as u64,
            top_level_paragraph_count: self.document.paragraphs.len() as u64,
            content_paragraph_count: self
                .document
                .paragraphs
                .iter()
                .filter(|paragraph| paragraph.marker.is_none())
                .count() as u64,
            table_count: self.document.tables.len() as u64,
            row_count: self
                .document
                .tables
                .iter()
                .map(|table| u64::from(table.row_count))
                .sum(),
            cell_count: self.document.cells.len() as u64,
            image_count: self.document.images.len() as u64,
        };
        self.document
    }
}

#[derive(Clone, Copy)]
enum CellSlot<'a> {
    Continuation,
    Entry { entry: &'a XmlNode, end: usize },
    Covered,
}

fn normalize_patent(
    patent: &XmlNode,
    dialect: KipoXmlDialect,
    source: String,
    source_sha256: String,
    encoding: String,
) -> Result<HmlDocument, CoreError> {
    let mut builder = SemanticBuilder::new(dialect, source, source_sha256, encoding);
    let Some(description) = patent.first_descendant("description") else {
        return Ok(builder.finish());
    };

    builder.emit_section("【발명의 설명】");
    if let Some(title) = description.first_child("invention-title") {
        builder.emit_section("【발명의 명칭】");
        let text = title.text().trim().to_string();
        if !text.is_empty() {
            // 명칭 문단 정렬은 서식마다 다르다: 국내 = 양쪽(2048), PCT = 왼쪽
            // (통특기 RTF `\ql`). 둘 다 들여쓰기 800.
            let align = match dialect {
                KipoXmlDialect::PatentCafdoc => "justified",
                KipoXmlDialect::PctPatentCafdoc => "left",
            };
            builder.emit_plain(text, align, 1000, 280);
        }
    }
    if let Some(section) = description.first_child("technical-field") {
        builder.emit_section("【기술분야】");
        builder.emit_node_paragraphs(section)?;
    }
    if let Some(section) = description.first_child("background-art") {
        // 표제 문구도 서식마다 다르다: 국내는 「【발명의 배경이 되는 기술】」(레지스터·본문 둘 다),
        // PCT 는 「【배경기술】」.
        builder.emit_section(match dialect {
            KipoXmlDialect::PatentCafdoc => "【발명의 배경이 되는 기술】",
            KipoXmlDialect::PctPatentCafdoc => "【배경기술】",
        });
        builder.emit_node_paragraphs(section)?;
    }
    if let Some(summary) = description.first_child("summary-of-invention") {
        builder.emit_section("【발명의 내용】");
        let mut emitted_subsection = false;
        for (tag, title) in [
            ("tech-problem", "【해결하고자 하는 과제】"),
            ("tech-solution", "【과제의 해결 수단】"),
            ("advantageous-effects", "【발명의 효과】"),
        ] {
            if let Some(section) = summary.first_descendant(tag) {
                emitted_subsection = true;
                builder.emit_section(title);
                builder.emit_node_paragraphs(section)?;
            }
        }
        if !emitted_subsection {
            builder.emit_node_paragraphs(summary)?;
        }
    }
    for (tag, title) in [
        ("description-of-drawings", "【도면의 간단한 설명】"),
        (
            "description-of-embodiments",
            "【발명을 실시하기 위한 구체적인 내용】",
        ),
        ("reference-signs-list", "【부호의 설명】"),
    ] {
        if let Some(section) = description.first_child(tag) {
            builder.emit_section(title);
            builder.emit_node_paragraphs(section)?;
        }
    }

    if let Some(claims) = patent.first_descendant("claims") {
        let claim_nodes: Vec<_> = claims
            .child_elements()
            .filter(|child| child.name.eq_ignore_ascii_case("claim"))
            .collect();
        if !claim_nodes.is_empty() {
            builder.emit_section("【청구범위】");
            for (index, claim) in claim_nodes.into_iter().enumerate() {
                let number = claim
                    .attributes
                    .get("num")
                    .cloned()
                    .unwrap_or_else(|| (index + 1).to_string());
                builder.emit_section(&format!("【청구항 {number}】"));
                if let Some(claim_text) = claim.first_descendant("claim-text") {
                    for seeds in inline_segments(claim_text) {
                        builder.emit_seeds(seeds, "justified", 1000, 280);
                    }
                }
            }
        }
    }

    if let Some(abstract_node) = patent.first_descendant("abstract") {
        builder.emit_section("【요약서】");
        if let Some(summary) = abstract_node.first_child("summary") {
            builder.emit_section("【요약】");
            builder.emit_node_paragraphs(summary)?;
        }
        if let Some(abstract_figure) = abstract_node.first_child("abstract-figure")
            && let Some(figref) = abstract_figure.first_descendant("figref")
            && let Some(number) = figref.attributes.get("num")
        {
            builder.emit_section("【대표도】");
            builder.emit_plain(format!("도 {number}"), "justified", 1000, 280);
        }
    }

    if let Some(drawings) = patent.first_descendant("drawings") {
        let figures: Vec<_> = drawings
            .child_elements()
            .filter(|child| child.name.eq_ignore_ascii_case("figure"))
            .collect();
        if !figures.is_empty() {
            builder.emit_section("【도면】");
            for (index, figure) in figures.into_iter().enumerate() {
                let number = figure
                    .attributes
                    .get("num")
                    .cloned()
                    .unwrap_or_else(|| (index + 1).to_string());
                builder.emit_section(&format!("【도 {number}】"));
                let mut images = Vec::new();
                figure.descendants_named("img", &mut images);
                for image in images {
                    builder.emit_seeds(
                        vec![InlineSeed::Image {
                            name: image.attributes.get("file").cloned().unwrap_or_default(),
                            width_mm: attribute_u32(image, "wi"),
                            height_mm: attribute_u32(image, "he"),
                        }],
                        "left",
                        1000,
                        280,
                    );
                }
            }
        }
    }
    Ok(builder.finish())
}

fn inline_segments(node: &XmlNode) -> Vec<Vec<InlineSeed>> {
    let mut segments = vec![Vec::new()];
    collect_inline(node, &[], &mut segments);
    segments
}

fn collect_inline(
    node: &XmlNode,
    formats: &[CharacterFormat],
    segments: &mut Vec<Vec<InlineSeed>>,
) {
    for item in &node.content {
        match item {
            XmlContent::Text(text) => {
                let text = text.replace(['\r', '\n'], "");
                if !text.is_empty() {
                    let current = segments.last_mut().expect("initial segment");
                    push_seed_text(current, text, normalized_formats(formats));
                }
            }
            XmlContent::Element(child) if child.name.eq_ignore_ascii_case("br") => {
                segments.push(Vec::new());
            }
            XmlContent::Element(child) if child.name.eq_ignore_ascii_case("img") => {
                segments
                    .last_mut()
                    .expect("initial segment")
                    .push(InlineSeed::Image {
                        name: child.attributes.get("file").cloned().unwrap_or_default(),
                        width_mm: attribute_u32(child, "wi"),
                        height_mm: attribute_u32(child, "he"),
                    });
            }
            XmlContent::Element(child) if child.name.eq_ignore_ascii_case("tables") => {}
            XmlContent::Element(child) => {
                let mut child_formats = formats.to_vec();
                if let Some(format) = inline_format(&child.name) {
                    child_formats.push(format);
                }
                collect_inline(child, &child_formats, segments);
            }
        }
    }
}

fn attribute_u32(node: &XmlNode, name: &str) -> Option<u32> {
    node.attributes
        .get(name)
        .and_then(|value| value.parse().ok())
}

fn parse_required_u32(node: &XmlNode, name: &str) -> Result<u32, CoreError> {
    parse_optional_u32(node, name)?.ok_or_else(table_error)
}

fn parse_optional_u32(node: &XmlNode, name: &str) -> Result<Option<u32>, CoreError> {
    node.attributes
        .get(name)
        .map(|value| value.parse().map_err(|_| table_error()))
        .transpose()
}

fn parse_optional_i32(node: &XmlNode, name: &str) -> Result<Option<i32>, CoreError> {
    node.attributes
        .get(name)
        .map(|value| value.parse().map_err(|_| table_error()))
        .transpose()
}

fn parse_leading_u32(node: &XmlNode, name: &str) -> Result<u32, CoreError> {
    let value = node.attributes.get(name).ok_or_else(table_error)?;
    let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return Err(table_error());
    }
    digits.parse().map_err(|_| table_error())
}

fn normalized_cell_align(value: &str) -> Option<&'static str> {
    match value.to_ascii_lowercase().as_str() {
        "left" => Some("left"),
        "center" => Some("center"),
        "right" => Some("right"),
        // 통특기 제출 변환이 국내 표(셀 모양 = 양쪽 2048)를 `align="justify"` 로 낸다. 모르는 값으로 두면 국내 기본
        // center 로 떨어져 셀 33개 전부가 가운데가 됐다.
        "justify" | "justified" => Some("justified"),
        _ => None,
    }
}

fn contains_any(node: &XmlNode, names: &[&str]) -> bool {
    names
        .iter()
        .any(|name| node.first_descendant(name).is_some())
}

fn inline_format(name: &str) -> Option<CharacterFormat> {
    match name.to_ascii_lowercase().as_str() {
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

fn push_seed_text(seeds: &mut Vec<InlineSeed>, text: String, formats: Vec<CharacterFormat>) {
    if let Some(InlineSeed::Text {
        text: previous,
        formats: previous_formats,
    }) = seeds.last_mut()
        && *previous_formats == formats
    {
        previous.push_str(&text);
        return;
    }
    seeds.push(InlineSeed::Text { text, formats });
}

fn push_inline_text(inlines: &mut Vec<HmlInline>, text: String, formats: Vec<CharacterFormat>) {
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

fn seed_text(seeds: &[InlineSeed]) -> String {
    seeds
        .iter()
        .filter_map(|seed| match seed {
            InlineSeed::Text { text, .. } => Some(text.as_str()),
            InlineSeed::Image { .. } => None,
        })
        .collect()
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

fn image_display_units(mm: u32) -> u32 {
    (mm * 1000 + 141) / 282
}

fn secondary_image_units(primary: u32) -> u32 {
    (primary * 3 + 2) / 4
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
            "과제의 해결 수단" | "해결하고자 하는 과제" | "발명의 효과" => 2,
            _ => 1,
        },
    }
}

fn empty_summary() -> HmlSummary {
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

fn decode_xml<'a>(bytes: &'a [u8], source: &str) -> Result<(Cow<'a, str>, String), CoreError> {
    let prefix = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_ascii_lowercase();
    let cp949 = ["euc-kr", "ks-c-5601-1987", "cp949", "ms949"]
        .iter()
        .any(|encoding| encoding_declared(&prefix, encoding));
    let utf8 = ["utf-8", "utf8"]
        .iter()
        .any(|encoding| encoding_declared(&prefix, encoding));

    let decoded = if bytes.starts_with(&[0xFF, 0xFE]) {
        UTF_16LE
            .decode_without_bom_handling_and_without_replacement(&bytes[2..])
            .map(|value| (value, "utf-16le".to_string()))
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        UTF_16BE
            .decode_without_bom_handling_and_without_replacement(&bytes[2..])
            .map(|value| (value, "utf-16be".to_string()))
    } else if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) || utf8 {
        let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        UTF_8
            .decode_without_bom_handling_and_without_replacement(body)
            .map(|value| (value, "utf-8".to_string()))
    } else if cp949 {
        EUC_KR
            .decode_without_bom_handling_and_without_replacement(bytes)
            .map(|value| (value, "cp949".to_string()))
    } else {
        UTF_8
            .decode_without_bom_handling_and_without_replacement(bytes)
            .map(|value| (value, "utf-8".to_string()))
    };

    decoded.ok_or_else(|| {
        let mut error = CoreError::new(ErrorCode::Cp949Invalid, "kipo_xml_decode", false);
        error.source = safe_source(source);
        error
    })
}

fn encoding_declared(prefix: &str, encoding: &str) -> bool {
    prefix.contains(&format!("encoding=\"{encoding}\""))
        || prefix.contains(&format!("encoding='{encoding}'"))
}

pub(crate) fn parse_xml(source: &str, display_source: &str) -> Result<XmlNode, CoreError> {
    let mut reader = Reader::from_str(source);
    reader.config_mut().trim_text(false);
    let mut stack: Vec<XmlNode> = Vec::new();
    let mut root = None;

    loop {
        let event = reader
            .read_event()
            .map_err(|_| parse_error(display_source))?;
        match event {
            Event::Start(start) => {
                stack.push(node_from_start(&reader, &start, display_source)?);
            }
            Event::Empty(start) => {
                let node = node_from_start(&reader, &start, display_source)?;
                append_node(&mut stack, &mut root, node, display_source)?;
            }
            Event::End(_) => {
                let Some(node) = stack.pop() else {
                    return Err(parse_error(display_source));
                };
                append_node(&mut stack, &mut root, node, display_source)?;
            }
            Event::Text(text) => {
                if let Some(parent) = stack.last_mut() {
                    let decoded = text.decode().map_err(|_| parse_error(display_source))?;
                    let unescaped = quick_xml::escape::unescape(&decoded)
                        .map_err(|_| parse_error(display_source))?;
                    parent
                        .content
                        .push(XmlContent::Text(unescaped.into_owned()));
                }
            }
            Event::CData(text) => {
                if let Some(parent) = stack.last_mut() {
                    let decoded = text.decode().map_err(|_| parse_error(display_source))?;
                    parent.content.push(XmlContent::Text(decoded.into_owned()));
                }
            }
            Event::GeneralRef(reference) => {
                if let Some(parent) = stack.last_mut() {
                    let name = reference
                        .decode()
                        .map_err(|_| parse_error(display_source))?;
                    let character =
                        resolve_reference(&name).ok_or_else(|| parse_error(display_source))?;
                    parent.content.push(XmlContent::Text(character.to_string()));
                }
            }
            Event::Eof => break,
            Event::Decl(_) | Event::PI(_) | Event::Comment(_) | Event::DocType(_) => {}
        }
    }

    if !stack.is_empty() {
        return Err(parse_error(display_source));
    }
    root.ok_or_else(|| parse_error(display_source))
}

fn resolve_reference(name: &str) -> Option<char> {
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

fn node_from_start(
    reader: &Reader<&[u8]>,
    start: &quick_xml::events::BytesStart<'_>,
    source: &str,
) -> Result<XmlNode, CoreError> {
    let start_name = start.name();
    let qualified = std::str::from_utf8(start_name.as_ref()).map_err(|_| parse_error(source))?;
    let name = qualified
        .rsplit_once(':')
        .map_or(qualified, |(_, local)| local)
        .to_string();
    let mut attributes = BTreeMap::new();
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|_| parse_error(source))?;
        let qualified =
            std::str::from_utf8(attribute.key.as_ref()).map_err(|_| parse_error(source))?;
        let key = qualified
            .rsplit_once(':')
            .map_or(qualified, |(_, local)| local)
            .to_ascii_lowercase();
        let value = attribute
            .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder())
            .map_err(|_| parse_error(source))?
            .into_owned();
        attributes.insert(key, value);
    }
    Ok(XmlNode {
        name,
        attributes,
        content: Vec::new(),
    })
}

fn append_node(
    stack: &mut [XmlNode],
    root: &mut Option<XmlNode>,
    node: XmlNode,
    source: &str,
) -> Result<(), CoreError> {
    if let Some(parent) = stack.last_mut() {
        parent.content.push(XmlContent::Element(node));
        return Ok(());
    }
    if root.is_some() {
        return Err(parse_error(source));
    }
    *root = Some(node);
    Ok(())
}

fn parse_error(source: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::StructureInvalid, "kipo_xml_parse", false);
    error.source = safe_source(source);
    error
}

fn dialect_error(source: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::UnsupportedFormat, "kipo_xml_dialect", false);
    error.source = safe_source(source);
    error
}

fn table_error() -> CoreError {
    CoreError::new(ErrorCode::GeometryInvalid, "kipo_xml_table", false)
}
