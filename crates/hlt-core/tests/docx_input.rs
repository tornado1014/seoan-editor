mod support;

use std::io::{Cursor, Write};

use hlt_core::{CharacterFormat, ErrorCode, HmlInline, hml_semantic_sha256, inspect_docx_path};
use sha2::{Digest, Sha256};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const DOCUMENT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
 xmlns:v="urn:schemas-microsoft-com:vml"><w:body><w:p><w:r><w:t>시험</w:t></w:r></w:p></w:body></w:document>"#;

const RELS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/>
</Relationships>"#;

const PARAGRAPH_DOCUMENT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
 xmlns:v="urn:schemas-microsoft-com:vml"><w:body>
<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:t>【발명의 설명】</w:t></w:r></w:p>
<w:p><w:pPr><w:jc w:val="left"/></w:pPr><w:r><w:t>【발명의 명칭】</w:t></w:r></w:p>
<w:p><w:pPr><w:ind w:firstLine="800"/></w:pPr><w:r><w:t>시험 발명</w:t></w:r></w:p>
<w:p><w:r><w:t>【기술분야】</w:t></w:r></w:p>
<w:p><w:pPr><w:jc w:val="center"/><w:rPr><w:b/><w:i/><w:u w:val="single"/></w:rPr></w:pPr><w:r><w:rPr><w:u w:val="single"/></w:rPr><w:t>관련 출원</w:t></w:r></w:p>
<w:p><w:pPr><w:ind w:firstLine="800"/></w:pPr>
 <w:r><w:t xml:space="preserve">일반 </w:t></w:r>
 <w:r><w:rPr><w:b/></w:rPr><w:t>굵게</w:t></w:r>
 <w:r><w:rPr><w:b/><w:i/></w:rPr><w:t>둘</w:t></w:r>
 <w:r><w:pict><v:shape><v:imagedata r:id="rId4"/></v:shape></w:pict></w:r>
 <w:r><w:tab/></w:r><w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr><w:t>2</w:t></w:r><w:r><w:t xml:space="preserve"> 끝</w:t></w:r>
</w:p>
<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:br w:type="page"/><w:t>【청구범위】</w:t></w:r></w:p>
<w:p><w:r><w:t>【청구항 5】</w:t></w:r></w:p>
<w:p><w:pPr><w:ind w:firstLine="800"/></w:pPr><w:r><w:t>청구 내용</w:t></w:r></w:p>
</w:body></w:document>"#;

const TABLE_DOCUMENT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
 xmlns:v="urn:schemas-microsoft-com:vml"><w:body>
<w:p><w:r><w:t>【발명의 설명】</w:t></w:r></w:p>
<w:p><w:r><w:t>【발명을 실시하기 위한 구체적인 내용】</w:t></w:r></w:p>
<w:p><w:r><w:t>【표 1】</w:t></w:r></w:p>
<w:p><w:pPr><w:ind w:firstLine="640"/></w:pPr><w:r><w:t>구성품 시험 결과</w:t></w:r></w:p>
<w:tbl><w:tblPr><w:tblInd w:w="1188" w:type="dxa"/></w:tblPr><w:tblGrid>
 <w:gridCol w:w="2520"/><w:gridCol w:w="2940"/><w:gridCol w:w="3150"/></w:tblGrid>
<w:tr><w:tc><w:tcPr><w:tcW w:w="2520"/><w:vMerge w:val="restart"/></w:tcPr><w:p><w:pPr><w:jc w:val="left"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>화합물</w:t></w:r></w:p></w:tc>
<w:tc><w:tcPr><w:tcW w:w="6090"/><w:gridSpan w:val="2"/></w:tcPr><w:p><w:pPr><w:jc w:val="left"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>분석</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:tc><w:tcPr><w:tcW w:w="2520"/><w:vMerge/></w:tcPr><w:p/></w:tc>
<w:tc><w:tcPr><w:tcW w:w="2940"/></w:tcPr><w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:t>t-값</w:t></w:r></w:p></w:tc>
<w:tc><w:tcPr><w:tcW w:w="3150"/></w:tcPr><w:p><w:pPr><w:jc w:val="left"/></w:pPr><w:r><w:pict><v:shape><v:imagedata r:id="rId4"/></v:shape></w:pict></w:r><w:r><w:t>0.01</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>C</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:tc><w:p><w:r><w:t>D</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>E</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>F</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:tc><w:p><w:r><w:t>G</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>H</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>I</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:tc><w:p><w:r><w:t>J</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>K</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>L</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:tc><w:p><w:r><w:t>M</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>N</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>O</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:tc><w:p><w:r><w:t>P</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Q</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>R</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p><w:pPr><w:ind w:firstLine="800"/></w:pPr><w:r><w:t>표 뒤 문단</w:t></w:r></w:p>
</w:body></w:document>"#;

const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0xFC, 0xCF, 0xC0, 0x50,
    0x0F, 0x00, 0x05, 0x80, 0x01, 0xFF, 0x96, 0xBC, 0xA1, 0xD9, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

fn write_docx(parts: &[(&str, &[u8])]) -> tempfile::NamedTempFile {
    let mut bytes = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut bytes);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, content) in parts {
            writer.start_file(*name, options).unwrap();
            writer.write_all(content).unwrap();
        }
        writer.finish().unwrap();
    }
    let mut file = tempfile::Builder::new().suffix(".docx").tempfile().unwrap();
    file.write_all(bytes.get_ref()).unwrap();
    file.flush().unwrap();
    file
}

fn valid_parts() -> Vec<(&'static str, &'static [u8])> {
    vec![
        ("[Content_Types].xml", b"<Types/>"),
        ("word/document.xml", DOCUMENT_XML.as_bytes()),
        ("word/styles.xml", b"<w:styles xmlns:w=\"x\"/>"),
        ("word/_rels/document.xml.rels", RELS_XML.as_bytes()),
        ("word/media/image1.png", PNG_1X1),
    ]
}

#[test]
fn package_inventory_resolves_and_hashes_media_without_extracting() {
    let file = write_docx(&valid_parts());
    let bytes = std::fs::read(file.path()).unwrap();
    let result = inspect_docx_path(file.path()).unwrap();

    assert_eq!(
        result.source,
        file.path().file_name().unwrap().to_str().unwrap()
    );
    assert_eq!(
        result.source_sha256,
        format!("{:X}", Sha256::digest(&bytes))
    );
    assert_eq!(result.package.entry_count, 5);
    assert_eq!(
        result.package.main_document_bytes,
        DOCUMENT_XML.len() as u64
    );
    assert_eq!(
        result.package.styles_bytes,
        Some(b"<w:styles xmlns:w=\"x\"/>".len() as u64)
    );
    assert_eq!(result.package.relationships_bytes, RELS_XML.len() as u64);
    assert_eq!(result.package.media_count, 1);
    assert_eq!(result.package.media_bytes, PNG_1X1.len() as u64);
    assert_eq!(result.media.len(), 1);

    let media = &result.media[0];
    assert_eq!(media.relationship_id, "rId4");
    assert_eq!(media.target, "word/media/image1.png");
    assert_eq!(media.media_name, "image1.png");
    assert_eq!(media.byte_size, PNG_1X1.len() as u64);
    assert_eq!(media.sha256, format!("{:X}", Sha256::digest(PNG_1X1)));
    assert_eq!(media.pixel_width, 1);
    assert_eq!(media.pixel_height, 1);
}

#[test]
fn package_rejects_missing_main_part_or_relationship_target() {
    let missing_main = write_docx(&[("[Content_Types].xml", b"<Types/>")]);
    let error = inspect_docx_path(missing_main.path()).unwrap_err();
    assert_eq!(error.code, ErrorCode::UnsupportedFormat);
    assert_eq!(error.phase, "docx_package");

    let missing_media = write_docx(&[
        ("word/document.xml", DOCUMENT_XML.as_bytes()),
        ("word/_rels/document.xml.rels", RELS_XML.as_bytes()),
    ]);
    let error = inspect_docx_path(missing_media.path()).unwrap_err();
    assert_eq!(error.code, ErrorCode::StructureInvalid);
    assert_eq!(error.phase, "docx_relationships");
}

#[test]
fn package_rejects_duplicate_or_unsafe_part_names() {
    let duplicate = write_docx(&[
        ("word/document.xml", DOCUMENT_XML.as_bytes()),
        ("WORD/DOCUMENT.XML", DOCUMENT_XML.as_bytes()),
        ("word/_rels/document.xml.rels", b"<Relationships/>"),
    ]);
    let error = inspect_docx_path(duplicate.path()).unwrap_err();
    assert_eq!(error.code, ErrorCode::StructureInvalid);
    assert_eq!(error.phase, "docx_package");

    for unsafe_name in ["../evil.xml", "/evil.xml", "C:/evil.xml", "word\\evil.xml"] {
        let file = write_docx(&[
            ("word/document.xml", DOCUMENT_XML.as_bytes()),
            ("word/_rels/document.xml.rels", b"<Relationships/>"),
            (unsafe_name, b"evil"),
        ]);
        let error = inspect_docx_path(file.path()).unwrap_err();
        assert_eq!(error.code, ErrorCode::PathNotAllowed);
        assert_eq!(error.phase, "docx_package");
    }
}

#[test]
fn package_rejects_invalid_or_oversized_docx_before_parts_are_read() {
    let mut invalid = tempfile::Builder::new().suffix(".docx").tempfile().unwrap();
    invalid.write_all(b"not a zip").unwrap();
    invalid.flush().unwrap();
    let error = inspect_docx_path(invalid.path()).unwrap_err();
    assert_eq!(error.code, ErrorCode::UnsupportedFormat);
    assert_eq!(error.phase, "docx_package");

    let oversized = tempfile::Builder::new().suffix(".docx").tempfile().unwrap();
    oversized.as_file().set_len(64 * 1024 * 1024 + 1).unwrap();
    let error = inspect_docx_path(oversized.path()).unwrap_err();
    assert_eq!(error.code, ErrorCode::SizeLimitExceeded);
    assert_eq!(error.phase, "docx_read");
    assert_eq!(error.expected.as_deref(), Some("67108864"));
    assert_eq!(error.actual.as_deref(), Some("67108865"));
}

#[test]
#[ignore = "실물 work/pct_a 필요(공개 저장소에 없음 · 있으면 --ignored 로)"]
fn real_package_inventory_matches_the_original_docx() {
    let path = support::repo_root().join("work/pct_a/diagnostics/pct_a_source.docx");
    let result = inspect_docx_path(&path).unwrap();

    assert_eq!(
        result.source_sha256,
        "FC02485BA73976BBA7D89F07ED9E1F6A941930B13176B217D13F4117630D73FD"
    );
    assert_eq!(result.package.entry_count, 80);
    assert_eq!(result.package.main_document_bytes, 448_848);
    assert_eq!(result.package.styles_bytes, Some(29_775));
    assert_eq!(result.package.relationships_bytes, 10_119);
    assert_eq!(result.package.media_count, 69);
    assert_eq!(result.package.media_bytes, 2_181_213);
    assert_eq!(result.media.len(), 69);
    let first = result
        .media
        .iter()
        .find(|media| media.relationship_id == "rId4")
        .unwrap();
    assert_eq!(first.target, "word/media/image1.jpeg");
    assert_eq!(first.pixel_width, 493);
    assert_eq!(first.pixel_height, 702);
}

#[test]
fn paragraph_normalization_preserves_markers_runs_images_and_source_page_breaks() {
    let parts = vec![
        ("[Content_Types].xml", b"<Types/>".as_slice()),
        ("word/document.xml", PARAGRAPH_DOCUMENT_XML.as_bytes()),
        ("word/_rels/document.xml.rels", RELS_XML.as_bytes()),
        ("word/media/image1.png", PNG_1X1),
    ];
    let file = write_docx(&parts);
    let result = inspect_docx_path(file.path()).unwrap();
    let document = &result.document;

    assert_eq!(
        document
            .sections
            .iter()
            .map(|section| section.title.as_str())
            .collect::<Vec<_>>(),
        [
            "【발명의 설명】",
            "【발명의 명칭】",
            "【기술분야】",
            "【청구범위】",
            "【청구항 5】",
        ]
    );
    assert_eq!(document.summary.section_count, 5);
    assert_eq!(document.summary.top_level_paragraph_count, 9);
    assert_eq!(document.summary.content_paragraph_count, 4);
    assert_eq!(document.summary.image_count, 1);

    assert_eq!(document.paragraphs[0].align, "center");
    assert_eq!(document.paragraphs[2].text, "시험 발명");
    assert_eq!(document.paragraphs[2].align, "justified");
    assert_eq!(document.paragraphs[2].indent_raw, 1000);

    let heading = &document.paragraphs[4];
    assert_eq!(heading.text, "관련 출원");
    assert_eq!(heading.align, "center");
    assert_eq!(heading.indent_raw, 0);
    assert_eq!(heading.lspace, 160);
    assert!(
        matches!(&heading.inlines[0], HmlInline::Text { text, formats, .. } if text == "관련 출원" && formats == &[CharacterFormat::Underline])
    );

    let mixed = &document.paragraphs[5];
    assert_eq!(mixed.text, "일반 굵게둘\t2 끝");
    assert_eq!(mixed.indent_raw, 1000);
    assert_eq!(mixed.inlines.len(), 7);
    assert!(
        matches!(&mixed.inlines[0], HmlInline::Text { text, formats, .. } if text == "일반 " && formats.is_empty())
    );
    assert!(
        matches!(&mixed.inlines[1], HmlInline::Text { text, formats, .. } if text == "굵게" && formats == &[CharacterFormat::Bold])
    );
    assert!(
        matches!(&mixed.inlines[2], HmlInline::Text { text, formats, .. } if text == "둘" && formats == &[CharacterFormat::Bold, CharacterFormat::Italic])
    );
    assert!(matches!(&mixed.inlines[3], HmlInline::Image { image_id } if image_id == "image:0"));
    assert!(
        matches!(&mixed.inlines[4], HmlInline::Text { text, formats, .. } if text == "\t" && formats.is_empty())
    );
    assert!(
        matches!(&mixed.inlines[5], HmlInline::Text { text, formats, .. } if text == "2" && formats == &[CharacterFormat::Superscript])
    );
    assert!(
        matches!(&mixed.inlines[6], HmlInline::Text { text, formats, .. } if text == " 끝" && formats.is_empty())
    );

    let image = &document.images[0];
    assert_eq!(image.name, "pct00001.png");
    assert_eq!(image.width, Some(539));
    assert_eq!(image.height, Some(539));
    assert_eq!(image.secondary_width, Some(404));
    assert_eq!(image.secondary_height, Some(404));
    assert_eq!(image.paragraph_id.as_deref(), Some("paragraph:6"));

    assert_eq!(result.page_breaks.len(), 1);
    assert_eq!(result.page_breaks[0].body_paragraph_index, 7);
    assert_eq!(
        result.page_breaks[0].semantic_paragraph_id.as_deref(),
        Some("paragraph:7")
    );
    assert!(!document.paragraphs[6].page_break);
}

#[test]
fn table_normalization_preserves_grid_merges_image_caption_and_tail_order() {
    let parts = vec![
        ("word/document.xml", TABLE_DOCUMENT_XML.as_bytes()),
        ("word/_rels/document.xml.rels", RELS_XML.as_bytes()),
        ("word/media/image1.png", PNG_1X1),
    ];
    let file = write_docx(&parts);
    let result = inspect_docx_path(file.path()).unwrap();
    let document = &result.document;

    assert_eq!(document.summary.section_count, 3);
    assert_eq!(document.summary.top_level_paragraph_count, 5);
    assert_eq!(document.summary.content_paragraph_count, 2);
    assert_eq!(document.summary.table_count, 1);
    assert_eq!(document.summary.row_count, 8);
    assert_eq!(document.summary.cell_count, 23);
    assert_eq!(document.summary.image_count, 1);
    assert_eq!(document.sections[2].title, "【표 1】");

    let caption = &document.paragraphs[3];
    assert_eq!(caption.text, "구성품 시험 결과");
    assert_eq!(caption.indent_raw, 800);
    let table_paragraph = &document.paragraphs[4];
    assert_eq!(table_paragraph.table_id.as_deref(), Some("table:1"));
    assert_eq!(table_paragraph.indent_raw, 1188);
    assert_eq!(table_paragraph.text, "표 뒤 문단");
    assert_eq!(table_paragraph.tail_text, "표 뒤 문단");

    let table = &document.tables[0];
    assert_eq!(table.paragraph_id, table_paragraph.id);
    assert_eq!(table.max_columns, 3);
    assert_eq!(table.row_count, 8);
    assert_eq!(table.cell_ids.len(), 23);

    let first = &document.cells[0];
    assert_eq!(first.id, "cell:1:1:1");
    assert_eq!(first.text, "화합물");
    assert_eq!(first.start_raw, None);
    assert_eq!(first.width_raw, Some(2520));
    assert_eq!(first.hwidth_raw, None);
    assert_eq!(first.rowspan, 2);
    assert_eq!(first.colspan, 1);
    assert!(!first.merge_continuation);
    assert!(
        matches!(&first.inlines[0], HmlInline::Text { text, formats, .. } if text == "화합물" && formats == &[CharacterFormat::Bold])
    );

    let wide = &document.cells[1];
    assert_eq!(wide.width_raw, Some(6090));
    assert_eq!(wide.hwidth_raw, None);
    assert_eq!(wide.colspan, 2);
    let continuation = &document.cells[2];
    assert_eq!(continuation.id, "cell:1:2:1");
    assert_eq!(continuation.width_raw, Some(2520));
    assert_eq!(continuation.hwidth_raw, None);
    assert!(continuation.merge_continuation);
    assert_eq!(continuation.text, "");
    assert_eq!(document.cells[3].text, "t-값");
    assert_eq!(document.cells[3].align, "center");
    assert_eq!(document.cells[4].text, "0.01");
    assert_eq!(document.cells[4].image_ids, ["image:0"]);
    assert_eq!(document.images[0].cell_id.as_deref(), Some("cell:1:2:3"));
    assert_eq!(
        document.images[0].paragraph_id.as_deref(),
        Some("paragraph:5")
    );
}

#[test]
#[ignore = "실물 work/pct_a 필요(공개 저장소에 없음 · 있으면 --ignored 로)"]
fn real_docx_preserves_normalized_counts_and_native_word_table_geometry() {
    let path = support::repo_root().join("work/pct_a/diagnostics/pct_a_source.docx");
    let result = inspect_docx_path(&path).unwrap();
    let document = &result.document;

    assert_eq!(document.summary.section_count, 151);
    assert_eq!(document.summary.top_level_paragraph_count, 561);
    assert_eq!(document.summary.content_paragraph_count, 410);
    assert_eq!(document.summary.table_count, 1);
    assert_eq!(document.summary.row_count, 8);
    assert_eq!(document.summary.cell_count, 16);
    assert_eq!(document.summary.image_count, 69);
    assert_eq!(result.page_breaks.len(), 58);
    assert_eq!(result.media.len(), 69);
    assert_eq!(
        hml_semantic_sha256(document),
        "7DD8B6ACA163097A7706DE15FBFE4FBB3E332940B109B0FBE75A9A49E3C677F4"
    );

    assert_eq!(document.sections[0].title, "【발명의 설명】");
    assert_eq!(document.sections[7].title, "【표 1】");
    assert_eq!(document.sections[8].title, "【청구범위】");
    assert_eq!(document.sections[9].title, "【청구항 1】");
    assert_eq!(document.sections[78].title, "【요약서】");
    assert_eq!(document.sections[81].title, "【도면】");
    assert_eq!(document.sections[82].title, "【도 1】");

    let related = document
        .paragraphs
        .iter()
        .find(|paragraph| paragraph.text == "관련 출원")
        .unwrap();
    assert_eq!(related.align, "center");
    assert_eq!(related.indent_raw, 0);
    assert_eq!(related.lspace, 160);
    assert!(
        matches!(&related.inlines[0], HmlInline::Text { formats, .. } if formats == &[CharacterFormat::Underline])
    );

    let table = &document.tables[0];
    assert_eq!(table.max_columns, 2);
    assert_eq!(table.row_count, 8);
    assert_eq!(document.cells[0].text, "구성품");
    assert_eq!(document.cells[0].width_raw, Some(3600));
    assert_eq!(document.cells[0].hwidth_raw, None);
    assert_eq!(document.cells[1].text, "t-값");
    assert_eq!(document.cells[1].width_raw, Some(3510));
    assert_eq!(document.cells[1].hwidth_raw, None);
    assert!(
        matches!(&document.cells[0].inlines[0], HmlInline::Text { formats, .. } if formats == &[CharacterFormat::Bold])
    );
    assert!(
        matches!(&document.cells[1].inlines[0], HmlInline::Text { formats, .. } if formats == &[CharacterFormat::Bold])
    );

    let first_image = &document.images[0];
    assert_eq!(first_image.name, "pct00001.jpg");
    assert_eq!(first_image.width, Some(539));
    assert_eq!(first_image.height, Some(766));
    assert_eq!(first_image.secondary_width, Some(404));
    assert_eq!(first_image.secondary_height, Some(575));
}

#[test]
#[ignore = "실물 work/pct_a 필요(공개 저장소에 없음 · 있으면 --ignored 로)"]
fn real_docx_table_runs_carry_their_ten_point_size_while_body_runs_stay_default() {
    // 네이티브 저장본 `pct_a_source.HLT` 실측: 표 셀 문자는 문자모양
    // attr=0/font=10, attr=2/font=10 만 쓰고 본문은 font=12 다. DOCX 는 정확히 그
    // 셀 런에만 `<w:sz w:val="20"/>`(=10pt) 를 달아 두었다. 이미지 런의 sz=2 는
    // 글자가 없으므로 크기를 만들지 않는다.
    let path = support::repo_root().join("work/pct_a/diagnostics/pct_a_source.docx");
    let document = inspect_docx_path(&path).unwrap().document;

    for (index, cell) in document.cells.iter().enumerate() {
        for inline in &cell.inlines {
            assert!(
                matches!(inline, HmlInline::Text { size, .. } if *size == Some(10)),
                "cell {index} inline lost its 10pt size: {inline:?}"
            );
        }
    }

    let body_sizes: Vec<_> = document
        .paragraphs
        .iter()
        .filter(|paragraph| paragraph.table_id.is_none())
        .flat_map(|paragraph| paragraph.inlines.iter())
        .filter_map(|inline| match inline {
            HmlInline::Text { size, .. } => Some(*size),
            HmlInline::Image { .. } => None,
        })
        .collect();
    assert!(!body_sizes.is_empty());
    assert!(body_sizes.iter().all(Option::is_none));
}

#[test]
#[ignore = "실물 work/pct_a 필요(공개 저장소에 없음 · 있으면 --ignored 로)"]
fn centered_paragraphs_without_first_line_indent_take_the_native_line_spacing() {
    // 네이티브 저장본 `pct_a_source.HLT` 실측: 줄간격 160 문단모양
    // (가운데, 들여쓰기 0)을 쓰는 문단은 정확히 8건이고 -- 【발명의 설명】,【청구범위】,
    // 【요약서】,【도면】, 관련 출원, 발명의 분야, 정부 지원, 도면 문단 1건 -- DOCX 에서
    // `w:jc=center` 이면서 `w:ind firstLine` 이 없는 문단 8건과 1:1 대응한다. 가운데
    // 정렬이지만 들여쓰기 800 인 도면 문단 68건은 원본에서 280 이다.
    let path = support::repo_root().join("work/pct_a/diagnostics/pct_a_source.docx");
    let document = inspect_docx_path(&path).unwrap().document;

    let flat: Vec<_> = document
        .paragraphs
        .iter()
        .filter(|paragraph| paragraph.align == "center" && paragraph.indent_raw == 0)
        .collect();
    assert_eq!(flat.len(), 8);
    assert!(flat.iter().all(|paragraph| paragraph.lspace == 160));
    assert_eq!(flat[0].text, "【발명의 설명】");
    assert_eq!(flat[1].text, "관련 출원");
    assert_eq!(flat[4].text, "【청구범위】");

    let indented: Vec<_> = document
        .paragraphs
        .iter()
        .filter(|paragraph| paragraph.align == "center" && paragraph.indent_raw != 0)
        .collect();
    assert_eq!(indented.len(), 68);
    assert!(indented.iter().all(|paragraph| paragraph.lspace == 280));
}

/// 칸 그림이 글 줄보다 높으면 그 행 상자가 그림을 다 덮고 다음 행은 그림 아래에서 시작한다(겹쳐 그리지 않는다).
#[test]
fn a_table_row_grows_to_its_cell_picture_when_printed() {
    // 칸 그림 = Word 크기 40×30mm(글 한 줄 ≈ 5mm 보다 훨씬 높다).
    let xml = TABLE_DOCUMENT_XML.replace(
        r#"<w:pict><v:shape><v:imagedata r:id="rId4"/></v:shape></w:pict>"#,
        r#"<w:drawing><wp:inline xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"><wp:extent cx="1440000" cy="1080000"/><a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:graphicData><pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:blipFill><a:blip r:embed="rId4"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing>"#,
    );
    let mut png = Vec::new();
    image::RgbImage::from_pixel(80, 60, image::Rgb([0, 0, 0]))
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let parts = vec![
        ("word/document.xml", xml.as_bytes()),
        ("word/_rels/document.xml.rels", RELS_XML.as_bytes()),
        ("word/media/image1.png", png.as_slice()),
    ];
    let file = write_docx(&parts);
    let (document, payloads, _) = hlt_core::load_generation_input(file.path()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let hlt = dir.path().join("표그림.HLT");
    hlt_core::save_generated_hlt_as_new(
        file.path(),
        &document,
        hlt_core::GenerationDialect::PctPatent,
        &payloads,
        &hlt,
        hlt_core::WriteOptions::default(),
    )
    .unwrap();
    let (_, inflated) = hlt_core::inflate_hlt(&hlt).unwrap();
    let inspected = hlt_core::inspect_path(&hlt, hlt_core::InspectOptions::default()).unwrap();
    let plan = hlt_core::page_plan(&inspected, &inflated, hlt_core::PrintOptions::default());
    let page = plan
        .pages
        .iter()
        .find(|page| !page.cells.is_empty())
        .unwrap();
    let image = page
        .images
        .iter()
        .find(|image| image.height > 50.0)
        .expect("칸 그림");
    let holder = page
        .cells
        .iter()
        .filter(|cell| cell.top <= image.top + 1.0 && image.top < cell.top + cell.height)
        .min_by(|a, b| a.height.total_cmp(&b.height))
        .expect("그림을 품은 칸");
    assert!(
        holder.top + holder.height >= image.top + image.height,
        "칸 {holder:?} 이 그림 {image:?} 을 덮는다"
    );
    let below = page
        .cells
        .iter()
        .filter(|cell| cell.top > holder.top + 1.0)
        .map(|cell| cell.top)
        .fold(f64::INFINITY, f64::min);
    assert!(
        below >= image.top + image.height,
        "다음 행 {below} 은 그림 아래"
    );
}

/// CP949 에 없는 글자: 대응표에 있으면 바꿔 굽고(목록을 낸다) · 없으면 그런 글자 전부를 들고 거부한다.
#[test]
fn word_characters_outside_cp949_are_substituted_or_listed() {
    let xml = TABLE_DOCUMENT_XML
        .replace("구성품 시험 결과", "5 \u{2212} 3 \u{00B5}m\u{00A0}값")
        .replace(">t-값<", ">café<");
    let parts = vec![
        ("word/document.xml", xml.as_bytes()),
        ("word/_rels/document.xml.rels", RELS_XML.as_bytes()),
        ("word/media/image1.png", PNG_1X1),
    ];
    let file = write_docx(&parts);
    let (document, _, substitutions) = hlt_core::load_generation_input(file.path()).unwrap();
    let pairs: Vec<(char, &str)> = substitutions
        .iter()
        .map(|item| (item.from, item.to.as_str()))
        .collect();
    assert_eq!(
        pairs,
        [
            ('\u{A0}', " "),
            ('\u{B5}', "\u{3BC}"),
            ('é', "e"),
            ('\u{2212}', "-")
        ]
    );
    let caption = document
        .paragraphs
        .iter()
        .find(|p| p.text.starts_with("5 "))
        .unwrap();
    assert_eq!(caption.text, "5 - 3 \u{3BC}m 값");
    let run_text: String = caption
        .inlines
        .iter()
        .filter_map(|inline| match inline {
            HmlInline::Text { text, .. } => Some(text.as_str()),
            HmlInline::Image { .. } => None,
        })
        .collect();
    assert_eq!(run_text, caption.text, "런 글도 같게 바뀐다");
    assert!(document.cells.iter().any(|cell| cell.text == "cafe"));

    // 대응표에 없는 글자 둘 → 둘 다 이름을 들고 거부.
    let xml = TABLE_DOCUMENT_XML.replace("구성품 시험 결과", "\u{1F600} \u{2603}");
    let parts = vec![
        ("word/document.xml", xml.as_bytes()),
        ("word/_rels/document.xml.rels", RELS_XML.as_bytes()),
        ("word/media/image1.png", PNG_1X1),
    ];
    let file = write_docx(&parts);
    let error = hlt_core::load_generation_input(file.path()).unwrap_err();
    assert_eq!(error.code, ErrorCode::Cp949Invalid);
    let chars = &error.details["chars"];
    assert!(
        chars.contains("U+1F600") && chars.contains("U+2603"),
        "{chars}"
    );
}

/// 확인 표 두 단계: 찾기(바꾸지 않음 · 종류별 횟수 · 앞뒤 글 · 제안) → 고른 대응대로 바꾸기.
#[test]
fn cp949_review_scans_then_applies_the_chosen_replacements() {
    let xml = TABLE_DOCUMENT_XML
        .replace("구성품 시험 결과", "10\u{2075} \u{2212} 3 \u{2212} 1")
        .replace(">t-값<", ">café<");
    let parts = vec![
        ("word/document.xml", xml.as_bytes()),
        ("word/_rels/document.xml.rels", RELS_XML.as_bytes()),
        ("word/media/image1.png", PNG_1X1),
    ];
    let file = write_docx(&parts);
    let (document, _) = hlt_core::load_generation_source(file.path()).unwrap();
    let findings = hlt_core::scan_cp949(&document);
    let rows: Vec<(char, Option<&str>, usize)> = findings
        .iter()
        .map(|item| (item.from, item.suggestion.as_deref(), item.count))
        .collect();
    // 첨자 ⁵ 는 대응표에 없다(None) · − 는 두 번(런과 문단 글을 두 번 세지 않는다).
    assert_eq!(
        rows,
        [
            ('é', Some("e"), 1),
            ('\u{2075}', None, 1),
            ('\u{2212}', Some("-"), 2)
        ]
    );
    assert!(
        findings[1].context.contains("10\u{2075} \u{2212} 3"),
        "{:?}",
        findings[1]
    );
    assert_eq!(findings[1].code, "U+2075");

    // 고르지 않은 대응 없는 글자(⁵)는 여전히 거부.
    let mut untouched = document.clone();
    assert!(hlt_core::make_cp949_safe(&mut untouched).is_err());
    // 고른 글이 CP949 밖이면 거부.
    let mut bad = document.clone();
    let error = hlt_core::apply_cp949(&mut bad, &[('\u{2075}', "\u{2076}".to_string())].into())
        .unwrap_err();
    assert_eq!(error.phase, "cp949_choice");

    // 고른 대응: ⁵ → ^5 · − → 대응표와 다른 ～ · é → 지움.
    let mut chosen = document.clone();
    let choices = [
        ('\u{2075}', "^5".to_string()),
        ('\u{2212}', "\u{FF5E}".to_string()),
        ('é', String::new()),
    ]
    .into();
    hlt_core::apply_cp949(&mut chosen, &choices).unwrap();
    let caption = chosen
        .paragraphs
        .iter()
        .find(|p| p.text.starts_with("10"))
        .unwrap();
    assert_eq!(caption.text, "10^5 \u{FF5E} 3 \u{FF5E} 1");
    assert!(chosen.cells.iter().any(|cell| cell.text == "caf"));
}
