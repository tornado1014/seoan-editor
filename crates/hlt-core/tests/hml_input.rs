mod support;

use std::io::Write;

use encoding_rs::EUC_KR;
use hlt_core::{CharacterFormat, ErrorCode, HmlInline, inspect_hml_path};

const HML_BODY: &str = r#"<!DOCTYPE HWPML SYSTEM>
<HWPML VER="HWPML1.1" CODE=KS>
<HEAD><information><summation><title>시험 &amp; 문서</title></summation>
<fontlist><korfont n=1><font id=0>신명조</font></korfont></fontlist></information></HEAD>
<BODY>
<p styleid=0 pagebreak=1><pstyle indent=0 lspace=160 align=center><text>【발명의 설명】</text></p>
<p styleid=0><pstyle indent=1000 lspace=280 align=justified><text>앞 <b>굵게<i>둘다</i></b></text><ctrlcode id=image><data name="pct00001.jpg" type=2 width=149 height=85 w2=112 h2=64 align=bottom></ctrlcode><text> 뒤</text></p>
<p styleid=0><pstyle indent=1000 lspace=280 align=justified><ctrlcode id=box><box type=table maxcol=2><tr>
<CELL CSTART=0 CWIDTH=4000 COLBEGIN=1 COLSPAN=1 ROWBEGIN=1 ROWSPAN=2 ALIGN="LEFT"><p styleid=0><pstyle indent=0 lspace=130 align=left><text><b>화합물</b></text></P></cell>
<CELL CSTART=4000 CWIDTH=4260 COLBEGIN=2 COLSPAN=1 ROWBEGIN=1 ROWSPAN=1 ALIGN="CENTER"><p styleid=0><pstyle indent=0 lspace=130 align=center><text>t-값</text></P></cell>
</tr><tr>
<CELL CSTART=0 CWIDTH=4000 COLBEGIN=1 COLSPAN=1 ROWBEGIN=2 ROWSPAN=1 MERGECONT=1 ALIGN="LEFT"><p styleid=0><pstyle indent=0 lspace=130 align=left><text></text></P></cell>
<CELL CSTART=4000 CWIDTH=4260 COLBEGIN=2 COLSPAN=1 ROWBEGIN=2 ROWSPAN=1 ALIGN="LEFT"><p styleid=0><pstyle indent=0 lspace=130 align=left><ctrlcode id=image><data name="pct00002.jpg" type=2 width=20 height=10 align=bottom></ctrlcode><text>0.01</text></P></cell>
</tr></box></ctrlcode><tail indent=1000 lspace=280 align=justified><text>표 뒤 문단</text></tail></p>
</BODY></HWPML>"#;

fn write_hml(text: &str, encoding: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::Builder::new().suffix(".hml").tempfile().unwrap();
    match encoding {
        "cp949" => {
            let (bytes, _, had_errors) = EUC_KR.encode(text);
            assert!(!had_errors);
            file.write_all(&bytes).unwrap();
        }
        "utf-8" => file.write_all(text.as_bytes()).unwrap(),
        _ => unreachable!(),
    }
    file.flush().unwrap();
    file
}

#[test]
fn reads_cp949_hml_without_flattening_inline_order_or_table_cells() {
    let file = write_hml(HML_BODY, "cp949");
    let document = inspect_hml_path(file.path()).unwrap();

    assert_eq!(document.encoding, "cp949");
    assert_eq!(document.title, "시험 & 문서");
    assert_eq!(document.fonts, ["신명조"]);
    assert_eq!(document.summary.section_count, 1);
    assert_eq!(document.summary.top_level_paragraph_count, 3);
    assert_eq!(document.summary.content_paragraph_count, 2);
    assert_eq!(document.summary.table_count, 1);
    assert_eq!(document.summary.row_count, 2);
    assert_eq!(document.summary.cell_count, 4);
    assert_eq!(document.summary.image_count, 2);

    assert_eq!(document.sections[0].id, "section:1");
    assert_eq!(document.sections[0].title, "【발명의 설명】");
    assert_eq!(document.sections[0].kind, "section");
    assert_eq!(document.sections[0].level, 0);

    let paragraph = &document.paragraphs[1];
    assert_eq!(paragraph.id, "paragraph:2");
    assert_eq!(paragraph.text, "앞 굵게둘다 뒤");
    assert_eq!(paragraph.align, "justified");
    assert_eq!(paragraph.indent_raw, 1000);
    assert_eq!(paragraph.lspace, 280);
    assert_eq!(paragraph.inlines.len(), 5);
    assert!(
        matches!(&paragraph.inlines[0], HmlInline::Text { text, formats, .. } if text == "앞 " && formats.is_empty())
    );
    assert!(
        matches!(&paragraph.inlines[1], HmlInline::Text { text, formats, .. } if text == "굵게" && formats == &[CharacterFormat::Bold])
    );
    assert!(
        matches!(&paragraph.inlines[2], HmlInline::Text { text, formats, .. } if text == "둘다" && formats == &[CharacterFormat::Bold, CharacterFormat::Italic])
    );
    assert!(
        matches!(&paragraph.inlines[3], HmlInline::Image { image_id } if image_id == "image:0")
    );
    assert!(
        matches!(&paragraph.inlines[4], HmlInline::Text { text, formats, .. } if text == " 뒤" && formats.is_empty())
    );

    let table = &document.tables[0];
    assert_eq!(table.id, "table:1");
    assert_eq!(table.paragraph_id, "paragraph:3");
    assert_eq!(table.max_columns, 2);
    assert_eq!(table.row_count, 2);
    assert_eq!(
        table.cell_ids,
        ["cell:1:1:1", "cell:1:1:2", "cell:1:2:1", "cell:1:2:2"]
    );

    let first = &document.cells[0];
    assert_eq!(first.text, "화합물");
    assert_eq!(first.start_raw, Some(0));
    assert_eq!(first.width_raw, Some(4000));
    assert_eq!(first.rowspan, 2);
    assert_eq!(first.colspan, 1);
    assert_eq!(first.align, "left");
    assert!(
        matches!(&first.inlines[0], HmlInline::Text { text, formats, .. } if text == "화합물" && formats == &[CharacterFormat::Bold])
    );

    let continuation = &document.cells[2];
    assert!(continuation.merge_continuation);
    assert_eq!(continuation.text, "");

    assert_eq!(
        document.images[0].paragraph_id.as_deref(),
        Some("paragraph:2")
    );
    assert_eq!(document.images[0].cell_id, None);
    assert_eq!(document.images[0].width, Some(149));
    assert_eq!(document.images[0].secondary_width, Some(112));
    assert_eq!(
        document.images[1].paragraph_id.as_deref(),
        Some("paragraph:3")
    );
    assert_eq!(document.images[1].cell_id.as_deref(), Some("cell:1:2:2"));

    assert_eq!(document.paragraphs[2].tail_text, "표 뒤 문단");
}

#[test]
fn honors_an_explicit_utf8_declaration_and_numeric_entities() {
    let hml = HML_BODY
        .replace(
            "<!DOCTYPE HWPML SYSTEM>",
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE HWPML SYSTEM>",
        )
        .replace("시험 &amp; 문서", "시험 &#x26; &#47928;&#49436;");
    let file = write_hml(&hml, "utf-8");
    let document = inspect_hml_path(file.path()).unwrap();

    assert_eq!(document.encoding, "utf-8");
    assert_eq!(document.title, "시험 & 문서");
}

#[test]
fn body_title_tags_do_not_replace_the_head_summation_title() {
    let hml = HML_BODY.replace("<BODY>", "<BODY><title>본문 안의 별도 제목</title>");
    let file = write_hml(&hml, "cp949");
    let document = inspect_hml_path(file.path()).unwrap();

    assert_eq!(document.title, "시험 & 문서");
}

#[test]
fn rejects_text_that_is_not_an_hwpml_document() {
    let file = write_hml("<BODY><p><text>문단</text></p></BODY>", "cp949");
    let error = inspect_hml_path(file.path()).unwrap_err();

    assert_eq!(error.code, ErrorCode::StructureInvalid);
    assert_eq!(error.phase, "hml_parse");
}

#[test]
fn rejects_undecodable_hml_without_replacement_characters() {
    let mut file = tempfile::Builder::new().suffix(".hml").tempfile().unwrap();
    file.write_all(b"<HWPML CODE=KS><BODY><p><text>\xff\xff</text></p></BODY></HWPML>")
        .unwrap();
    file.flush().unwrap();

    let error = inspect_hml_path(file.path()).unwrap_err();
    assert_eq!(error.code, ErrorCode::Cp949Invalid);
    assert_eq!(error.phase, "hml_decode");
}

#[test]
fn real_counterpart_hml_matches_the_python_generator() {
    if !support::has_local_files(&["work/pct_a/pct_a_v5.hml"]) {
        return;
    }
    let path = support::repo_root().join("work/pct_a/pct_a_v5.hml");
    let document = inspect_hml_path(&path).unwrap();

    assert_eq!(document.source, "pct_a_v5.hml");
    assert_eq!(
        document.source_sha256,
        "EE8953295A94C0C99FF30194FE8CD05EDB256ADCF6B79E62852DFE9EAE261E99"
    );
    assert_eq!(document.encoding, "cp949");
    assert_eq!(document.summary.section_count, 60);
    assert_eq!(document.summary.top_level_paragraph_count, 1149);
    assert_eq!(document.summary.content_paragraph_count, 1089);
    assert_eq!(document.summary.table_count, 2);
    assert_eq!(document.summary.row_count, 19);
    assert_eq!(document.summary.cell_count, 87);
    assert_eq!(document.summary.image_count, 281);
    assert_eq!(document.sections[0].title, "【발명의 설명】");
    assert_eq!(document.sections[1].title, "【발명의 명칭】");
    assert_eq!(document.sections[8].title, "【표 1】");
    assert_eq!(document.sections[10].title, "【청구범위】");

    let table = &document.tables[0];
    assert_eq!(table.max_columns, 5);
    assert_eq!(table.row_count, 16);
    let first = &document.cells[0];
    assert_eq!(first.text, "화합물");
    assert_eq!(first.start_raw, Some(0));
    assert_eq!(first.width_raw, Some(3696));
    assert_eq!(first.rowspan, 2);
    assert_eq!(first.colspan, 1);
    assert!(
        matches!(&first.inlines[0], HmlInline::Text { text, formats, .. } if text == "화합물" && formats == &[CharacterFormat::Bold])
    );
}

/// 출원서 서식 HML 인데 들여쓰기 · 양쪽 정렬이 빠졌으면 불러올 때 본문에 표준 모양을 입힌다(2026-11-23).
#[test]
fn bare_kipo_form_hml_gets_standard_indent_and_justify_on_import() {
    let text = r#"<!DOCTYPE HWPML SYSTEM>
<HWPML VER="HWPML1.1" CODE=KS>
<HEAD><information><fontlist><korfont n=1><font id=0>신명조</font></korfont></fontlist></information></HEAD>
<BODY>
<p styleid=0><pstyle indent=0 lspace=160 align=center><text>【발명의 설명】</text></p>
<p styleid=0><pstyle indent=0 lspace=280 align=left><text>【기술분야】</text></p>
<p styleid=0><pstyle indent=0 lspace=280 align=left><text>맨 문단</text></p>
<p styleid=0><pstyle indent=0 lspace=160 align=center><text>가운데 문단</text></p>
</BODY></HWPML>"#;
    let file = write_hml(text, "utf-8");
    let (document, _, _) = hlt_core::load_generation_input(file.path()).unwrap();
    let shape = |wanted: &str| {
        document
            .paragraphs
            .iter()
            .find(|p| p.text == wanted)
            .map(|p| (p.align.clone(), p.indent_raw))
            .unwrap()
    };
    assert_eq!(
        shape("맨 문단"),
        ("justified".to_string(), hlt_core::KIPO_BODY_INDENT_RAW)
    );
    assert_eq!(shape("가운데 문단"), ("center".to_string(), 0));
    assert_eq!(shape("【기술분야】"), ("left".to_string(), 0));
}

/// 한글 원본(.hwp · .hwpx)은 읽지 못한다 — 이유를 달아 거절한다(화면 · CLI 가 HML/워드로 저장하라고 알린다).
#[test]
fn hwp_and_hwpx_inputs_are_refused_with_a_save_as_reason() {
    for ext in ["hwp", "HWPX"] {
        let file = tempfile::Builder::new()
            .suffix(&format!(".{ext}"))
            .tempfile()
            .unwrap();
        let error = hlt_core::load_generation_source(file.path()).unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsupportedFormat);
        assert_eq!(
            error.details.get("reason").map(String::as_str),
            Some("hwp_save_as_hml_or_docx")
        );
    }
}
