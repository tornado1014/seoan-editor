//! 의미 모델(`HmlDocument`) → Word(.docx). 연 HLT 를 Word 로 내보내는 길.
//!
//! 되읽기는 `docx.rs` 가 한다 — 이 파일이 쓰는 것은 그 읽기기가 읽는 것과 짝이다
//! (`jc` · `ind@firstLine` · `b/i/u/vertAlign/sz` · `tblGrid/gridSpan/vMerge` · `tblInd` · `drawing/blip`).
//! 식별번호(【0001】)는 HLT 글에 없고 `number` 도 쓰지 않는다. 표제(【…】)는 문단 글 그대로 나온다.

use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use crate::{CharacterFormat, CoreError, ErrorCode, HmlCell, HmlDocument, HmlInline, HmlTable};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
const EMU_PER_PT: u64 = 12_700;

/// 그림 id → PNG 바이트. 없는 그림은 건너뛴다.
pub fn docx_bytes(
    document: &HmlDocument,
    pngs: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>, CoreError> {
    let mut body = String::new();
    let mut media = Media {
        pngs,
        files: Vec::new(),
        sizes: document
            .images
            .iter()
            .map(|image| {
                let w = image.secondary_width.or(image.width).unwrap_or(0);
                let h = image.secondary_height.or(image.height).unwrap_or(0);
                (image.id.clone(), (w, h))
            })
            .collect(),
    };
    for paragraph in &document.paragraphs {
        let props = paragraph_props(&paragraph.align, paragraph.indent_raw);
        let page = if paragraph.page_break {
            r#"<w:r><w:br w:type="page"/></w:r>"#
        } else {
            ""
        };
        if let Some(table) = paragraph
            .table_id
            .as_deref()
            .and_then(|id| document.tables.iter().find(|table| table.id == id))
        {
            if !page.is_empty() {
                body.push_str(&format!("<w:p>{page}</w:p>"));
            }
            body.push_str(&table_xml(
                document,
                table,
                paragraph.indent_raw,
                &mut media,
            ));
            // ⛔ 표 뒤 문단은 늘 하나 — 읽기기가 표 다음 문단을 꼬리로 삼킨다(꼬리가 없어도 빈 문단).
            body.push_str(&format!(
                "<w:p>{props}{}</w:p>",
                runs(&paragraph.inlines, &mut media)
            ));
            continue;
        }
        body.push_str(&format!(
            "<w:p>{props}{page}{}</w:p>",
            runs(&paragraph.inlines, &mut media)
        ));
    }
    let document_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W}" xmlns:r="{R}" xmlns:wp="{WP}" xmlns:a="{A}" xmlns:pic="{PIC}"><w:body>{body}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1701" w:right="1440" w:bottom="1440" w:left="1440" w:header="851" w:footer="992" w:gutter="0"/></w:sectPr></w:body></w:document>"#
    );
    package(&document_xml, &media.files)
}

/// 보정 전후 대비표(.docx): 제목 한 줄 + [항목 | 보정 전 | 보정 후] 표. 지운 글자는 취소선, 넣은 글자는 밑줄.
pub fn comparison_table_docx(title: &str, rows: &[crate::AmendRow]) -> Result<Vec<u8>, CoreError> {
    // 줄 간격은 표에 맞게 한 줄(기본 문단은 명세서용 2.8 줄).
    const P: &str = r#"<w:pPr><w:spacing w:after="0" w:line="300" w:lineRule="auto"/></w:pPr>"#;
    let marked = |text: &str, ranges: &[(usize, usize)], mark: &str| -> String {
        let chars: Vec<char> = text.chars().collect();
        let inside = |at: usize| ranges.iter().any(|(a, b)| (*a..*b).contains(&at));
        let mut paragraphs = vec![String::new()];
        let mut at = 0;
        while at < chars.len() {
            if chars[at] == '\n' {
                paragraphs.push(String::new());
                at += 1;
                continue;
            }
            let on = inside(at);
            let end = (at..chars.len())
                .find(|k| chars[*k] == '\n' || inside(*k) != on)
                .unwrap_or(chars.len());
            let piece: String = chars[at..end].iter().collect();
            let props = if on {
                format!("<w:rPr>{mark}</w:rPr>")
            } else {
                String::new()
            };
            paragraphs.last_mut().unwrap().push_str(&format!(
                r#"<w:r>{props}<w:t xml:space="preserve">{}</w:t></w:r>"#,
                escape(&piece)
            ));
            at = end;
        }
        paragraphs
            .iter()
            .map(|runs| format!("<w:p>{P}{runs}</w:p>"))
            .collect()
    };
    let cell = |width: u32, content: String| {
        format!(r#"<w:tc><w:tcPr><w:tcW w:w="{width}" w:type="dxa"/></w:tcPr>{content}</w:tc>"#)
    };
    let header = |text: &str| {
        format!(
            r#"<w:p>{P}<w:r><w:rPr><w:b/></w:rPr><w:t>{}</w:t></w:r></w:p>"#,
            escape(text)
        )
    };
    let mut table = String::from(
        r#"<w:tbl><w:tblPr><w:tblW w:w="9026" w:type="dxa"/><w:tblBorders><w:top w:val="single" w:sz="4"/><w:left w:val="single" w:sz="4"/><w:bottom w:val="single" w:sz="4"/><w:right w:val="single" w:sz="4"/><w:insideH w:val="single" w:sz="4"/><w:insideV w:val="single" w:sz="4"/></w:tblBorders><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="1426"/><w:gridCol w:w="3800"/><w:gridCol w:w="3800"/></w:tblGrid>"#,
    );
    table.push_str(&format!(
        "<w:tr><w:trPr><w:tblHeader/></w:trPr>{}{}{}</w:tr>",
        cell(1426, header("항목")),
        cell(3800, header("보정 전")),
        cell(3800, header("보정 후"))
    ));
    for row in rows {
        table.push_str(&format!(
            "<w:tr>{}{}{}</w:tr>",
            cell(1426, marked(&row.label, &[], "")),
            cell(3800, marked(&row.before, &row.deleted, "<w:strike/>")),
            cell(
                3800,
                marked(&row.after, &row.inserted, r#"<w:u w:val="single"/>"#)
            )
        ));
    }
    table.push_str("</w:tbl>");
    let document_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W}" xmlns:r="{R}"><w:body><w:p>{P}<w:r><w:rPr><w:b/></w:rPr><w:t>{}</w:t></w:r></w:p>{table}<w:p/><w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="851" w:footer="992" w:gutter="0"/></w:sectPr></w:body></w:document>"#,
        escape(title)
    );
    package(&document_xml, &[])
}

/// 문서 XML · 그림(PNG, 차례대로 `rIdImg1`…)을 .docx 로 묶는다.
fn package(document_xml: &str, images: &[Vec<u8>]) -> Result<Vec<u8>, CoreError> {
    let mut rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdStyles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#,
    );
    for (index, _) in images.iter().enumerate() {
        rels.push_str(&format!(
            r#"<Relationship Id="rIdImg{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image{n}.png"/>"#,
            n = index + 1
        ));
    }
    rels.push_str("</Relationships>");

    let mut buffer = Vec::new();
    {
        let mut zip = ZipWriter::new(Cursor::new(&mut buffer));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        let mut part = |name: &str, bytes: &[u8]| -> Result<(), CoreError> {
            zip.start_file(name, options).map_err(zip_error)?;
            zip.write_all(bytes).map_err(|_| write_error())
        };
        part("[Content_Types].xml", CONTENT_TYPES.as_bytes())?;
        part("_rels/.rels", ROOT_RELS.as_bytes())?;
        part("word/document.xml", document_xml.as_bytes())?;
        part("word/_rels/document.xml.rels", rels.as_bytes())?;
        part("word/styles.xml", STYLES.as_bytes())?;
        for (index, png) in images.iter().enumerate() {
            part(&format!("word/media/image{}.png", index + 1), png)?;
        }
        zip.finish().map_err(zip_error)?;
    }
    Ok(buffer)
}

struct Media<'a> {
    pngs: &'a BTreeMap<String, Vec<u8>>,
    files: Vec<Vec<u8>>,
    sizes: BTreeMap<String, (u32, u32)>,
}

impl Media<'_> {
    /// 그림 하나를 붙이고 그 런 XML 을 낸다(본문 없는 그림은 빈 글).
    fn drawing(&mut self, image_id: &str) -> String {
        let Some(png) = self.pngs.get(image_id) else {
            return String::new();
        };
        self.files.push(png.clone());
        let n = self.files.len();
        let (mut w, mut h) = self.sizes.get(image_id).copied().unwrap_or((0, 0));
        if w == 0 || h == 0 {
            // 표시 크기가 없으면 화소 크기(96dpi)로.
            let (pw, ph) = image::load_from_memory(png)
                .map(|image| (image.width(), image.height()))
                .unwrap_or((100, 100));
            (w, h) = (pw * 3 / 4, ph * 3 / 4);
        }
        let (cx, cy) = (u64::from(w) * EMU_PER_PT, u64::from(h) * EMU_PER_PT);
        format!(
            r#"<w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{n}" name="그림 {n}"/><a:graphic><a:graphicData uri="{PIC}"><pic:pic><pic:nvPicPr><pic:cNvPr id="{n}" name="image{n}.png"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rIdImg{n}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#
        )
    }
}

fn paragraph_props(align: &str, indent_raw: i32) -> String {
    let jc = match align {
        "left" | "center" | "right" => align,
        _ => "both",
    };
    // 읽기기 역식: indent_raw = (dxa*5+2)/4.
    let first = indent_raw.max(0) * 4 / 5;
    let ind = if first > 0 {
        format!(r#"<w:ind w:firstLine="{first}"/>"#)
    } else {
        String::new()
    };
    format!(r#"<w:pPr><w:jc w:val="{jc}"/>{ind}</w:pPr>"#)
}

fn runs(inlines: &[HmlInline], media: &mut Media<'_>) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            HmlInline::Image { image_id } => out.push_str(&media.drawing(image_id)),
            HmlInline::Text {
                text,
                formats,
                size,
            } => {
                let mut props = String::new();
                for format in formats {
                    props.push_str(match format {
                        CharacterFormat::Bold => "<w:b/>",
                        CharacterFormat::Italic => "<w:i/>",
                        CharacterFormat::Underline => r#"<w:u w:val="single"/>"#,
                        CharacterFormat::Superscript => r#"<w:vertAlign w:val="superscript"/>"#,
                        CharacterFormat::Subscript => r#"<w:vertAlign w:val="subscript"/>"#,
                    });
                }
                if let Some(size) = size {
                    props.push_str(&format!(r#"<w:sz w:val="{}"/>"#, size * 2));
                }
                let props = if props.is_empty() {
                    String::new()
                } else {
                    format!("<w:rPr>{props}</w:rPr>")
                };
                for (index, piece) in text.split('\t').enumerate() {
                    if index > 0 {
                        out.push_str(&format!("<w:r>{props}<w:tab/></w:r>"));
                    }
                    let piece = escape(piece);
                    if !piece.is_empty() {
                        out.push_str(&format!(
                            r#"<w:r>{props}<w:t xml:space="preserve">{piece}</w:t></w:r>"#
                        ));
                    }
                }
            }
        }
    }
    out
}

fn table_xml(
    document: &HmlDocument,
    table: &HmlTable,
    indent_raw: i32,
    media: &mut Media<'_>,
) -> String {
    let cells: Vec<&HmlCell> = table
        .cell_ids
        .iter()
        .filter_map(|id| document.cells.iter().find(|cell| &cell.id == id))
        .collect();
    // 칸 경계(시작 · 끝)로 열을 만든다. 칸마다 걸친 열 수 = gridSpan.
    let mut edges: Vec<u32> = cells
        .iter()
        .filter_map(|cell| Some([cell.start_raw?, cell.start_raw? + cell.width_raw?]))
        .flatten()
        .collect();
    edges.sort_unstable();
    edges.dedup();
    let geometric = edges.len() >= 2
        && cells
            .iter()
            .all(|cell| cell.start_raw.is_some() && cell.width_raw.is_some());
    let columns: Vec<u32> = if geometric {
        edges.windows(2).map(|pair| pair[1] - pair[0]).collect()
    } else {
        // ponytail: 자리 없는 칸은 고른 폭 — HLT 에서 온 칸은 늘 자리가 있다.
        vec![9000 / table.max_columns.max(1); table.max_columns.max(1) as usize]
    };
    let span = |cell: &HmlCell| -> u32 {
        match (geometric, cell.start_raw, cell.width_raw) {
            (true, Some(start), Some(width)) => edges
                .iter()
                .filter(|edge| **edge > start && **edge <= start + width)
                .count() as u32,
            _ => cell.colspan.max(1),
        }
    };
    let mut rows: BTreeMap<u32, Vec<&HmlCell>> = BTreeMap::new();
    for cell in &cells {
        rows.entry(cell.row).or_default().push(cell);
    }
    let mut out = format!(
        r#"<w:tbl><w:tblPr><w:tblInd w:w="{}" w:type="dxa"/><w:tblBorders><w:top w:val="single" w:sz="4"/><w:left w:val="single" w:sz="4"/><w:bottom w:val="single" w:sz="4"/><w:right w:val="single" w:sz="4"/><w:insideH w:val="single" w:sz="4"/><w:insideV w:val="single" w:sz="4"/></w:tblBorders><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid>"#,
        indent_raw.max(0)
    );
    for width in &columns {
        out.push_str(&format!(r#"<w:gridCol w:w="{width}"/>"#));
    }
    out.push_str("</w:tblGrid>");
    for row in rows.values_mut() {
        row.sort_by_key(|cell| (cell.start_raw, cell.column_begin));
        out.push_str("<w:tr>");
        for cell in row.iter() {
            let span = span(cell);
            let mut props = format!(
                r#"<w:tcW w:w="{}" w:type="dxa"/>"#,
                cell.width_raw.unwrap_or(0)
            );
            if span > 1 {
                props.push_str(&format!(r#"<w:gridSpan w:val="{span}"/>"#));
            }
            if cell.merge_continuation {
                props.push_str("<w:vMerge/>");
            } else if cell.rowspan > 1 {
                props.push_str(r#"<w:vMerge w:val="restart"/>"#);
            }
            let para = paragraph_props(&cell.align, 0);
            out.push_str(&format!(
                "<w:tc><w:tcPr>{props}</w:tcPr><w:p>{para}{}</w:p></w:tc>",
                runs(&cell.inlines, media)
            ));
        }
        out.push_str("</w:tr>");
    }
    out.push_str("</w:tbl>");
    out
}

/// XML 글 이스케이프 · XML 1.0 이 못 담는 제어문자는 뺀다(탭은 위에서 따로).
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            ch if ch < ' ' => {}
            ch => out.push(ch),
        }
    }
    out
}

fn zip_error(_: zip::result::ZipError) -> CoreError {
    write_error()
}

fn write_error() -> CoreError {
    CoreError::new(ErrorCode::IoError, "docx_write", false)
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

/// 기본 글꼴 = 바탕 12pt · 줄 간격 배수 2.8(`w:line` = 240 × 2.8 = 672 · 사용자 지정 10-19) — 표 칸 글도 같다.
const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Times New Roman" w:hAnsi="Times New Roman" w:eastAsia="바탕" w:cs="Times New Roman"/><w:sz w:val="24"/><w:szCs w:val="24"/><w:lang w:val="en-US" w:eastAsia="ko-KR"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="0" w:line="672" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style></w:styles>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparison_table_strikes_deleted_and_underlines_inserted_text() {
        let rows = [crate::AmendRow {
            label: "청구항 1".into(),
            before: "가나다\n둘째 줄".into(),
            after: "가X다\n둘째 줄".into(),
            deleted: vec![(1, 2)],
            inserted: vec![(1, 2)],
        }];
        let bytes = comparison_table_docx("보정 전후 대비표 <시험>", &rows).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut xml)
            .unwrap();
        assert!(xml.contains("보정 전후 대비표 &lt;시험&gt;"));
        assert!(xml.contains(
            r#"<w:r><w:rPr><w:strike/></w:rPr><w:t xml:space="preserve">나</w:t></w:r>"#
        ));
        assert!(xml.contains(
            r#"<w:r><w:rPr><w:u w:val="single"/></w:rPr><w:t xml:space="preserve">X</w:t></w:r>"#
        ));
        // 줄마다 문단(`\n` 은 새 문단) · 머리 행 + 한 행.
        assert_eq!(
            xml.matches(r#"<w:t xml:space="preserve">둘째 줄</w:t>"#)
                .count(),
            2
        );
        assert_eq!(xml.matches("</w:tr>").count(), 2);
    }
}
