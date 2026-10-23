mod support;

use std::io::Write;

use encoding_rs::EUC_KR;
use hlt_core::{
    CharacterFormat, ErrorCode, HmlInline, KipoXmlDialect, hml_semantic_sha256,
    inspect_kipo_xml_path,
};
use sha2::{Digest, Sha256};

fn write_xml(bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut file = tempfile::Builder::new().suffix(".xml").tempfile().unwrap();
    file.write_all(bytes).unwrap();
    file.flush().unwrap();
    file
}

#[test]
fn decoder_classifies_utf8_pct_xml_and_decodes_entities() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<KIPO xmlns="http://www.kipo.go.kr">
  <PCTPatentCAFDOC documentID="10">
    <description><invention-title>시험 &amp; 문서</invention-title></description>
  </PCTPatentCAFDOC>
</KIPO>"#
        .as_bytes();
    let file = write_xml(xml);
    let result = inspect_kipo_xml_path(file.path()).unwrap();

    assert_eq!(
        result.source,
        file.path().file_name().unwrap().to_str().unwrap()
    );
    assert_eq!(result.source_sha256, format!("{:X}", Sha256::digest(xml)));
    assert_eq!(result.input_encoding, "utf-8");
    assert_eq!(result.document_root, "PCTPatentCAFDOC");
    assert_eq!(result.dialect, KipoXmlDialect::PctPatentCafdoc);
    assert_eq!(result.invention_title, "시험 & 문서");
}

#[test]
fn decoder_classifies_cp949_domestic_patent_xml_without_replacement() {
    let source = r#"<?xml version="1.0" encoding="EUC-KR"?>
<KIPO><PatentCAFDOC><description><invention-title>국내 특허</invention-title>
</description></PatentCAFDOC></KIPO>"#;
    let (bytes, _, had_errors) = EUC_KR.encode(source);
    assert!(!had_errors);
    let file = write_xml(&bytes);
    let result = inspect_kipo_xml_path(file.path()).unwrap();

    assert_eq!(result.input_encoding, "cp949");
    assert_eq!(result.document_root, "PatentCAFDOC");
    assert_eq!(result.dialect, KipoXmlDialect::PatentCafdoc);
    assert_eq!(result.invention_title, "국내 특허");
    assert!(!result.invention_title.contains('\u{fffd}'));
}

#[test]
fn decoder_rejects_invalid_declared_encoding_bytes() {
    let file = write_xml(
        b"<?xml version=\"1.0\" encoding=\"EUC-KR\"?><KIPO><PatentCAFDOC>\xff\xff</PatentCAFDOC></KIPO>",
    );
    let error = inspect_kipo_xml_path(file.path()).unwrap_err();

    assert_eq!(error.code, ErrorCode::Cp949Invalid);
    assert_eq!(error.phase, "kipo_xml_decode");
}

#[test]
fn decoder_rejects_malformed_xml() {
    let file = write_xml(b"<KIPO><PCTPatentCAFDOC><description></KIPO>");
    let error = inspect_kipo_xml_path(file.path()).unwrap_err();

    assert_eq!(error.code, ErrorCode::StructureInvalid);
    assert_eq!(error.phase, "kipo_xml_parse");
}

#[test]
fn decoder_rejects_non_kipo_and_unknown_document_roots() {
    for source in [
        b"<root><PCTPatentCAFDOC/></root>".as_slice(),
        b"<KIPO><DesignCAFDOC/></KIPO>".as_slice(),
    ] {
        let file = write_xml(source);
        let error = inspect_kipo_xml_path(file.path()).unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsupportedFormat);
        assert_eq!(error.phase, "kipo_xml_dialect");
    }
}

#[test]
fn decoder_rejects_an_xml_larger_than_sixty_four_mib_before_reading() {
    let file = tempfile::Builder::new().suffix(".xml").tempfile().unwrap();
    file.as_file().set_len(64 * 1024 * 1024 + 1).unwrap();
    let error = inspect_kipo_xml_path(file.path()).unwrap_err();

    assert_eq!(error.code, ErrorCode::SizeLimitExceeded);
    assert_eq!(error.phase, "kipo_xml_read");
    assert_eq!(error.expected.as_deref(), Some("67108864"));
    assert_eq!(error.actual.as_deref(), Some("67108865"));
}

#[test]
fn description_normalizes_sections_shapes_breaks_and_mixed_inline_order() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<KIPO><PCTPatentCAFDOC><description>
<invention-title>시험 발명</invention-title>
<technical-field>
  <p num="0001"><u>관련 출원</u></p>
  <p num="0002">일반 <b>굵게<i>둘</i></b><img file="inline.jpg" wi="42" he="24"/> 뒤<br/>둘째</p>
</technical-field>
<background-art><p num="0003">배경 문단</p></background-art>
<summary-of-invention><tech-solution><p num="0004">해결 문단</p></tech-solution></summary-of-invention>
<description-of-drawings><p num="0005">도 1은 시험이다.</p></description-of-drawings>
<description-of-embodiments><p num="0006">구체 문단</p></description-of-embodiments>
<reference-signs-list><p num="0007">10: 장치</p></reference-signs-list>
</description></PCTPatentCAFDOC></KIPO>"#;
    let file = write_xml(xml.as_bytes());
    let result = inspect_kipo_xml_path(file.path()).unwrap();
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
            "【배경기술】",
            "【발명의 내용】",
            "【과제의 해결 수단】",
            "【도면의 간단한 설명】",
            "【발명을 실시하기 위한 구체적인 내용】",
            "【부호의 설명】",
        ]
    );
    assert_eq!(document.summary.section_count, 9);
    assert_eq!(document.summary.top_level_paragraph_count, 18);
    assert_eq!(document.summary.content_paragraph_count, 9);
    assert_eq!(document.summary.image_count, 1);

    let top = &document.paragraphs[0];
    assert_eq!(top.marker.as_deref(), Some("【발명의 설명】"));
    assert_eq!(top.align, "center");
    assert_eq!(top.indent_raw, 0);
    assert_eq!(top.lspace, 280);

    let title = &document.paragraphs[2];
    assert_eq!(title.text, "시험 발명");
    assert_eq!(title.section_id.as_deref(), Some("section:2"));
    assert_eq!(title.align, "left");
    assert_eq!(title.indent_raw, 1000);
    assert_eq!(title.lspace, 280);

    let centered_heading = &document.paragraphs[4];
    assert_eq!(centered_heading.text, "관련 출원");
    assert_eq!(centered_heading.align, "center");
    assert_eq!(centered_heading.indent_raw, 0);
    assert_eq!(centered_heading.lspace, 160);
    assert!(matches!(
        &centered_heading.inlines[0],
        HmlInline::Text { text, formats, .. }
            if text == "관련 출원" && formats == &[CharacterFormat::Underline]
    ));

    let mixed = &document.paragraphs[5];
    assert_eq!(mixed.text, "일반 굵게둘 뒤");
    assert_eq!(mixed.align, "justified");
    assert_eq!(mixed.indent_raw, 1000);
    assert_eq!(mixed.lspace, 280);
    assert_eq!(mixed.inlines.len(), 5);
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
        matches!(&mixed.inlines[4], HmlInline::Text { text, formats, .. } if text == " 뒤" && formats.is_empty())
    );

    let after_break = &document.paragraphs[6];
    assert_eq!(after_break.text, "둘째");
    assert_eq!(after_break.section_id.as_deref(), Some("section:3"));
    assert_eq!(
        document.paragraphs[7].marker.as_deref(),
        Some("【배경기술】")
    );
    assert_eq!(
        document.images[0].paragraph_id.as_deref(),
        Some("paragraph:6")
    );
    assert_eq!(document.images[0].cell_id, None);
}

#[test]
fn description_does_not_insert_blank_paragraphs_after_pct_centered_headings() {
    let xml = r#"<KIPO><PCTPatentCAFDOC><description><technical-field>
<p><u>관련 출원</u></p><p>첫 본문</p>
<p><u>발명의 분야</u></p><p>둘째 본문</p>
<p><u>정부 지원</u></p><p>셋째 본문</p>
</technical-field></description></PCTPatentCAFDOC></KIPO>"#;
    let file = write_xml(xml.as_bytes());
    let result = inspect_kipo_xml_path(file.path()).unwrap();
    let texts: Vec<_> = result
        .document
        .paragraphs
        .iter()
        .map(|paragraph| paragraph.text.as_str())
        .collect();

    assert_eq!(
        texts,
        [
            "【발명의 설명】",
            "【기술분야】",
            "관련 출원",
            "첫 본문",
            "발명의 분야",
            "둘째 본문",
            "정부 지원",
            "셋째 본문",
        ]
    );
    assert!(!texts.iter().any(|text| text.is_empty()));
}

#[test]
fn table_normalizes_cals_spans_images_caption_and_following_tail() {
    let xml = r#"<KIPO><PCTPatentCAFDOC><description><description-of-embodiments>
<p num="0001"><tables num="7"><table hlt-indent="800"><title>구성품 시험 결과</title>
<tgroup cols="3"><colspec colname="col1" colwidth="3000"/><colspec colname="col2" colwidth="3500"/><colspec colname="col3" colwidth="3760"/><tbody>
<row><entry colname="col1" morerows="1"><b>화합물</b></entry><entry namest="col2" nameend="col3"><b>분석</b></entry></row>
<row><entry colname="col2" align="center">t-값</entry><entry colname="col3"><img file="cell.jpg" wi="42" he="24"/>0.01</entry></row>
<row><entry colname="col1">A</entry><entry colname="col2">B</entry><entry colname="col3">C</entry></row>
<row><entry colname="col1">D</entry><entry colname="col2">E</entry><entry colname="col3">F</entry></row>
<row><entry colname="col1">G</entry><entry colname="col2">H</entry><entry colname="col3">I</entry></row>
<row><entry colname="col1">J</entry><entry colname="col2">K</entry><entry colname="col3">L</entry></row>
<row><entry colname="col1">M</entry><entry colname="col2">N</entry><entry colname="col3">O</entry></row>
<row><entry colname="col1">P</entry><entry colname="col2">Q</entry><entry colname="col3">R</entry></row>
</tbody></tgroup></table></tables></p>
<p num="0002">표 뒤 문단</p>
</description-of-embodiments></description></PCTPatentCAFDOC></KIPO>"#;
    let file = write_xml(xml.as_bytes());
    let result = inspect_kipo_xml_path(file.path()).unwrap();
    let document = &result.document;

    assert_eq!(
        document
            .sections
            .iter()
            .map(|section| section.title.as_str())
            .collect::<Vec<_>>(),
        [
            "【발명의 설명】",
            "【발명을 실시하기 위한 구체적인 내용】",
            "【표 1】",
        ]
    );
    assert_eq!(document.summary.table_count, 1);
    assert_eq!(document.summary.row_count, 8);
    assert_eq!(document.summary.cell_count, 23);
    assert_eq!(document.summary.image_count, 1);

    let table = &document.tables[0];
    assert_eq!(table.id, "table:1");
    assert_eq!(table.max_columns, 3);
    assert_eq!(table.row_count, 8);
    assert_eq!(table.cell_ids.len(), 23);
    let table_paragraph = document
        .paragraphs
        .iter()
        .find(|paragraph| paragraph.table_id.as_deref() == Some("table:1"))
        .unwrap();
    assert_eq!(table.paragraph_id, table_paragraph.id);
    assert_eq!(table_paragraph.align, "justified");
    assert_eq!(table_paragraph.indent_raw, 800);
    assert_eq!(table_paragraph.tail_text, "표 뒤 문단");
    assert_eq!(table_paragraph.text, "표 뒤 문단");

    let caption_index = document
        .paragraphs
        .iter()
        .position(|paragraph| paragraph.text == "구성품 시험 결과")
        .unwrap();
    let caption = &document.paragraphs[caption_index];
    assert_eq!(caption.align, "justified");
    assert_eq!(caption.indent_raw, 800);
    assert_eq!(caption.section_id.as_deref(), Some("section:3"));
    assert_eq!(
        document.paragraphs[caption_index + 1].id,
        table_paragraph.id
    );

    let first = &document.cells[0];
    assert_eq!(first.id, "cell:1:1:1");
    assert_eq!(first.text, "화합물");
    assert_eq!(first.start_raw, None);
    assert_eq!(first.width_raw, None);
    assert_eq!(first.hwidth_raw, Some(3000));
    assert_eq!(first.column_begin, Some(1));
    assert_eq!(first.row_begin, Some(1));
    assert_eq!(first.rowspan, 2);
    assert_eq!(first.colspan, 1);
    assert!(!first.merge_continuation);
    assert_eq!(first.align, "left");
    assert!(
        matches!(&first.inlines[0], HmlInline::Text { text, formats, .. } if text == "화합물" && formats == &[CharacterFormat::Bold])
    );

    let wide = &document.cells[1];
    assert_eq!(wide.hwidth_raw, Some(7260));
    assert_eq!(wide.column_begin, Some(2));
    assert_eq!(wide.colspan, 2);

    let continuation = &document.cells[2];
    assert_eq!(continuation.id, "cell:1:2:1");
    assert_eq!(continuation.text, "");
    assert_eq!(continuation.hwidth_raw, Some(3000));
    assert_eq!(continuation.rowspan, 1);
    assert_eq!(continuation.colspan, 1);
    assert!(continuation.merge_continuation);
    assert_eq!(continuation.align, "left");

    let centered = &document.cells[3];
    assert_eq!(centered.text, "t-값");
    assert_eq!(centered.align, "center");
    let image_cell = &document.cells[4];
    assert_eq!(image_cell.text, "0.01");
    assert_eq!(image_cell.image_ids, ["image:0"]);
    assert!(
        matches!(&image_cell.inlines[0], HmlInline::Image { image_id } if image_id == "image:0")
    );
    assert!(
        matches!(&image_cell.inlines[1], HmlInline::Text { text, formats, .. } if text == "0.01" && formats.is_empty())
    );

    let image = &document.images[0];
    assert_eq!(image.name, "cell.jpg");
    assert_eq!(image.width, Some(149));
    assert_eq!(image.height, Some(85));
    assert_eq!(image.secondary_width, Some(112));
    assert_eq!(image.secondary_height, Some(64));
    assert_eq!(
        image.paragraph_id.as_deref(),
        Some(table_paragraph.id.as_str())
    );
    assert_eq!(image.cell_id.as_deref(), Some("cell:1:2:3"));
}

#[test]
fn table_rejects_unknown_colnames_and_non_numeric_geometry() {
    for table in [
        r#"<table><tgroup cols="2"><colspec colname="col1" colwidth="5000"/><colspec colname="col2" colwidth="5260"/><tbody><row><entry colname="missing">X</entry></row></tbody></tgroup></table>"#,
        r#"<table><tgroup cols="1"><colspec colname="col1" colwidth="wide"/><tbody><row><entry colname="col1">X</entry></row></tbody></tgroup></table>"#,
    ] {
        let xml = format!(
            "<KIPO><PCTPatentCAFDOC><description><description-of-embodiments><p><tables>{table}</tables></p></description-of-embodiments></description></PCTPatentCAFDOC></KIPO>"
        );
        let file = write_xml(xml.as_bytes());
        let error = inspect_kipo_xml_path(file.path()).unwrap_err();
        assert_eq!(error.code, ErrorCode::GeometryInvalid);
        assert_eq!(error.phase, "kipo_xml_table");
    }
}

#[test]
fn terminal_sections_preserve_claim_figure_numbers_breaks_formats_and_images() {
    let xml = r#"<KIPO><PCTPatentCAFDOC>
<description><invention-title>종단 시험</invention-title></description>
<claims>
  <claim num="5"><claim-text>첫째<br/><i>둘째</i></claim-text></claim>
  <claim num="8a"><claim-text><img file="claim.jpg" wi="20" he="10"/>이미지 청구항</claim-text></claim>
</claims>
<abstract><summary><p>요약 문단</p></summary><abstract-figure><figref num="8"/></abstract-figure></abstract>
<drawings>
  <figure num="8a"><img file="fig8a.tif" wi="100" he="80"/></figure>
  <figure num="10"><img file="fig10.tif" wi="90" he="70"/></figure>
</drawings>
</PCTPatentCAFDOC></KIPO>"#;
    let file = write_xml(xml.as_bytes());
    let result = inspect_kipo_xml_path(file.path()).unwrap();
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
            "【청구범위】",
            "【청구항 5】",
            "【청구항 8a】",
            "【요약서】",
            "【요약】",
            "【대표도】",
            "【도면】",
            "【도 8a】",
            "【도 10】",
        ]
    );
    assert_eq!(document.summary.section_count, 11);
    assert_eq!(document.summary.top_level_paragraph_count, 19);
    assert_eq!(document.summary.content_paragraph_count, 8);
    assert_eq!(document.summary.image_count, 3);

    let claim5 = document
        .sections
        .iter()
        .find(|section| section.title == "【청구항 5】")
        .unwrap();
    let claim5_paragraphs: Vec<_> = claim5
        .paragraph_ids
        .iter()
        .filter_map(|id| {
            document
                .paragraphs
                .iter()
                .find(|paragraph| &paragraph.id == id)
        })
        .collect();
    assert_eq!(claim5_paragraphs[1].text, "첫째");
    assert_eq!(claim5_paragraphs[2].text, "둘째");
    assert!(
        matches!(&claim5_paragraphs[2].inlines[0], HmlInline::Text { text, formats, .. } if text == "둘째" && formats == &[CharacterFormat::Italic])
    );

    let claim8a = document
        .sections
        .iter()
        .find(|section| section.title == "【청구항 8a】")
        .unwrap();
    assert_eq!(claim8a.kind, "claim");
    let claim_image = &document.images[0];
    assert_eq!(claim_image.name, "claim.jpg");
    assert_eq!(claim_image.width, Some(71));
    assert_eq!(claim_image.height, Some(35));
    assert_eq!(claim_image.cell_id, None);

    let representative = document
        .paragraphs
        .iter()
        .find(|paragraph| paragraph.text == "도 8")
        .unwrap();
    assert_eq!(representative.section_id.as_deref(), Some("section:8"));
    assert_eq!(representative.indent_raw, 1000);

    assert_eq!(document.sections[9].kind, "figure");
    assert_eq!(document.sections[10].kind, "figure");
    assert_eq!(document.images[1].name, "fig8a.tif");
    assert_eq!(document.images[2].name, "fig10.tif");
    assert_eq!(
        document.images[1].paragraph_id.as_deref(),
        document.sections[9]
            .paragraph_ids
            .get(1)
            .map(String::as_str)
    );
}

#[test]
#[ignore = "실물 work/pct_a 필요(공개 저장소에 없음 · 있으면 --ignored 로)"]
fn real_counterpart_xml_matches_the_python_normalization_counts_and_geometry() {
    let path = support::repo_root().join("work/pct_a/pct_a.xml");
    let result = inspect_kipo_xml_path(&path).unwrap();
    let document = &result.document;

    assert_eq!(result.source, "pct_a.xml");
    assert_eq!(
        result.source_sha256,
        "F0F3F2C17ECFE3F90CA78D905E4ADCB2E792D988A1FF43CCF018306072F14D3C"
    );
    assert_eq!(result.dialect, KipoXmlDialect::PctPatentCafdoc);
    assert_eq!(digest(&result.invention_title), "31c7039b7d2a1da6");
    assert_eq!(document.summary.section_count, 60);
    assert_eq!(document.summary.top_level_paragraph_count, 1148);
    assert_eq!(document.summary.content_paragraph_count, 1088);
    assert_eq!(document.summary.table_count, 2);
    assert_eq!(document.summary.row_count, 19);
    assert_eq!(document.summary.cell_count, 87);
    assert_eq!(document.summary.image_count, 281);
    assert_eq!(
        hml_semantic_sha256(document),
        "C979B658264C2F4251F69348B04B6F4B1B17A69E35B95D964DE2062DDD5F2C2D"
    );
    assert_eq!(document.sections[0].title, "【발명의 설명】");
    assert_eq!(document.sections[8].title, "【표 1】");
    assert_eq!(document.sections[10].title, "【청구범위】");
    assert_eq!(document.sections[11].title, "【청구항 1】");
    assert_eq!(document.sections[57].title, "【도 1】");

    let first_table = &document.tables[0];
    assert_eq!(first_table.max_columns, 5);
    assert_eq!(first_table.row_count, 16);
    let first_cell = &document.cells[0];
    assert_eq!(first_cell.text, "화합물");
    assert_eq!(first_cell.hwidth_raw, Some(3696));
    assert_eq!(first_cell.rowspan, 2);
    assert_eq!(first_cell.colspan, 1);
    assert!(
        matches!(&first_cell.inlines[0], HmlInline::Text { text, formats, .. } if text == "화합물" && formats == &[CharacterFormat::Bold])
    );
}

#[test]
#[ignore = "실물 work/pct_a 필요(공개 저장소에 없음 · 있으면 --ignored 로)"]
fn real_docx_derived_pct_xml_matches_the_python_normalization_counts() {
    let path = support::repo_root().join("work/pct_a/diagnostics/pct_a_docx/pct_a_docx.xml");
    let result = inspect_kipo_xml_path(&path).unwrap();
    let document = &result.document;

    assert_eq!(
        result.source_sha256,
        "EDED96E13CE6D28F0C1BBC4A156CEB3EB55822036152C33A18A546A4929C1D06"
    );
    assert_eq!(digest(&result.invention_title), "72515a07794c077e");
    assert_eq!(document.summary.section_count, 150);
    assert_eq!(document.summary.top_level_paragraph_count, 556);
    assert_eq!(document.summary.content_paragraph_count, 406);
    assert_eq!(document.summary.table_count, 1);
    assert_eq!(document.summary.row_count, 8);
    assert_eq!(document.summary.cell_count, 16);
    assert_eq!(document.summary.image_count, 69);
    assert_eq!(
        hml_semantic_sha256(document),
        "05B8B4918FE623164BB0C74C2E584395FCE3DA9AC71A2615322F746550DB774B"
    );
    assert_eq!(document.sections[0].title, "【발명의 설명】");
    assert_eq!(document.sections[2].title, "【기술분야】");
    assert_eq!(document.sections[8].title, "【청구항 1】");
    assert_eq!(document.sections[80].title, "【도면】");
    assert_eq!(document.sections[81].title, "【도 1】");

    let table = &document.tables[0];
    assert_eq!(table.max_columns, 2);
    assert_eq!(table.row_count, 8);
    assert_eq!(document.cells[0].text, "구성품");
    assert_eq!(document.cells[0].hwidth_raw, Some(4286));
    assert!(
        matches!(&document.cells[0].inlines[0], HmlInline::Text { text, formats, .. } if text == "구성품" && formats == &[CharacterFormat::Bold])
    );
}

#[test]
fn semantic_fingerprint_ignores_input_provenance_but_detects_content_changes() {
    let xml = r#"<KIPO><PCTPatentCAFDOC><description><invention-title>지문 시험</invention-title><technical-field><p>본문</p></technical-field></description></PCTPatentCAFDOC></KIPO>"#;
    let file = write_xml(xml.as_bytes());
    let result = inspect_kipo_xml_path(file.path()).unwrap();
    let baseline = hml_semantic_sha256(&result.document);
    assert_eq!(baseline.len(), 64);
    assert!(
        baseline
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_lowercase())
    );

    let mut provenance_only = result.document.clone();
    provenance_only.source = "renamed.xml".to_string();
    provenance_only.source_sha256 = "0".repeat(64);
    provenance_only.encoding = "cp949".to_string();
    assert_eq!(hml_semantic_sha256(&provenance_only), baseline);

    let mut changed = provenance_only;
    changed.paragraphs[2].text.push('!');
    assert_ne!(hml_semantic_sha256(&changed), baseline);
}

/// 실물 문서의 글은 저장소에 싣지 않는다 — SHA-256 앞 16자로 대조한다.
#[allow(dead_code)]
fn digest(text: &str) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(text.as_bytes()))[..16].to_string()
}

/// 글 앞 `chars` 글자가 그 SHA-256(앞 16자)인가.
#[allow(dead_code)]
fn opens_with(text: &str, chars: usize, expected: &str) -> bool {
    digest(&text.chars().take(chars).collect::<String>()) == expected
}

/// 실물 본문 첫 문단(기술분야)과 둘째 문단(배경)의 첫머리 — 글 대신 지문.
#[allow(dead_code)]
const TECH_FIELD: &str = "4230137345539271";
#[allow(dead_code)]
const BACKGROUND: &str = "1a352eee416dc74b";
