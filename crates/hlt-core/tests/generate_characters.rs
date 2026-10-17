use hlt_core::{
    CharacterFormat, GeneratedFormatCode, GeneratedFormatSpan, GeneratedImageAnchor, GeneratedLine,
    GeneratedLineEnding, HltBodyPlan, generate_hlt_character_spans, plan_hlt_characters,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fn body(lines: Vec<GeneratedLine>, images: Vec<GeneratedImageAnchor>) -> HltBodyPlan {
    HltBodyPlan {
        section_table: vec![],
        body: vec![],
        lines,
        images,
        cell_ranges: vec![],
    }
}

fn line(
    text: &str,
    ending: GeneratedLineEnding,
    formats: Vec<GeneratedFormatSpan>,
) -> GeneratedLine {
    GeneratedLine {
        paragraph_id: "paragraph:1".into(),
        cell_id: None,
        text: text.into(),
        shape_index: 1,
        wipo_id: Some(1),
        formats,
        ending,
    }
}

#[test]
fn assigns_builtin_codes_and_duplicates_codes_for_cp949_bytes() {
    let formats = vec![
        span(0, 1, &[CharacterFormat::Bold]),
        span(1, 2, &[CharacterFormat::Italic]),
        span(2, 3, &[CharacterFormat::Underline]),
        span(3, 4, &[CharacterFormat::Subscript]),
        span(4, 5, &[CharacterFormat::Superscript]),
        span(5, 6, &[CharacterFormat::Bold, CharacterFormat::Underline]),
    ];
    let plan = plan_hlt_characters(&body(
        vec![line("가A나B다C", GeneratedLineEnding::Paragraph, formats)],
        vec![],
    ))
    .unwrap();

    assert_eq!(plan.object_count, 24);
    assert_eq!(
        plan.format_codes,
        [
            GeneratedFormatCode::new(vec![CharacterFormat::Bold], None, 2, 2, 12),
            GeneratedFormatCode::new(vec![CharacterFormat::Italic], None, 3, 4, 12),
            GeneratedFormatCode::new(vec![CharacterFormat::Underline], None, 4, 1, 12),
            GeneratedFormatCode::new(vec![CharacterFormat::Subscript], None, 5, 32, 12),
            GeneratedFormatCode::new(vec![CharacterFormat::Superscript], None, 6, 16, 12),
            GeneratedFormatCode::new(
                vec![CharacterFormat::Bold, CharacterFormat::Underline],
                None,
                7,
                3,
                12,
            ),
        ]
    );
    assert_eq!(plan.object_table.len(), 2_405);
    assert_eq!(plan.object_table[0], 0xbf);
    assert_eq!(
        u32::from_le_bytes(plan.object_table[1..5].try_into().unwrap()),
        24
    );
    assert_eq!(
        hex(&Sha256::digest(&plan.object_table)),
        "5E8245037CAAA86838D9C23A2E4BC3CEE07628F50754B07BDAFB289F70F0832C"
    );
    assert_eq!(words(&plan.spans), [10, 2, 2, 3, 4, 4, 5, 6, 6, 7, 0]);
}

#[test]
fn expands_cell_spans_with_cell_terminator_but_compresses_plain_control_lines() {
    let mut cell = line(
        "가",
        GeneratedLineEnding::Cell,
        vec![span(0, 1, &[CharacterFormat::Bold])],
    );
    cell.cell_id = Some("cell:1:1:1".into());
    cell.wipo_id = None;
    let mut row = line("\u{12}", GeneratedLineEnding::Row, vec![]);
    row.cell_id = Some("cell:1:1:1".into());
    row.wipo_id = None;
    let plain = line("A", GeneratedLineEnding::Continuation, vec![]);
    let plan = plan_hlt_characters(&body(vec![cell, row, plain], vec![])).unwrap();

    assert_eq!(words(&plan.spans), [3, 2, 2, 0, 1, 0, 1, 0]);
}

#[test]
fn replaces_only_the_image_character_byte_with_an_appended_object_code() {
    let source = body(
        vec![line("가_A", GeneratedLineEnding::Paragraph, vec![])],
        vec![GeneratedImageAnchor {
            image_id: "image:0".into(),
            line_index: 0,
            character_index: 1,
        }],
    );
    let plan = plan_hlt_characters(&source).unwrap();
    let image_codes = BTreeMap::from([("image:0".to_string(), 24)]);
    let spans = generate_hlt_character_spans(&source, &plan, &image_codes).unwrap();

    assert_eq!(words(&spans), [5, 0, 0, 24, 0, 0]);
}

fn span(start: u32, end: u32, formats: &[CharacterFormat]) -> GeneratedFormatSpan {
    GeneratedFormatSpan {
        start,
        end,
        formats: formats.to_vec(),
        size: None,
    }
}

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|value| format!("{value:02X}")).collect()
}

#[test]
fn allocates_font_sizes_from_the_source_run_and_defaults_to_twelve() {
    // 네이티브 `pct_a_source.HLT` 실측 문자모양: 본문 평문 attr=0/font=12,
    // 본문 굵게 attr=2/font=12, 표 셀 평문 attr=0/font=10, 표 셀 굵게 attr=2/font=10.
    // 즉 크기는 서식 종류가 아니라 원본 런의 크기에서 온다.
    let plain_ten = sized_span(0, 1, &[], Some(10));
    let bold_ten = sized_span(1, 2, &[CharacterFormat::Bold], Some(10));
    let bold_default = span(2, 3, &[CharacterFormat::Bold]);
    let plan = plan_hlt_characters(&body(
        vec![line(
            "가나다",
            GeneratedLineEnding::Paragraph,
            vec![plain_ten, bold_ten, bold_default],
        )],
        vec![],
    ))
    .unwrap();

    let bold = plan
        .format_codes
        .iter()
        .find(|entry| entry.formats == [CharacterFormat::Bold] && entry.size.is_none())
        .unwrap();
    assert_eq!((bold.attr, bold.font), (2, 12));
    let sized_plain = plan
        .format_codes
        .iter()
        .find(|entry| entry.formats.is_empty() && entry.size == Some(10))
        .unwrap();
    assert_eq!((sized_plain.attr, sized_plain.font), (0, 10));
    let sized_bold = plan
        .format_codes
        .iter()
        .find(|entry| entry.formats == [CharacterFormat::Bold] && entry.size == Some(10))
        .unwrap();
    assert_eq!((sized_bold.attr, sized_bold.font), (2, 10));

    assert_eq!(
        words(&plan.spans),
        [
            7,
            sized_plain.code,
            sized_plain.code,
            sized_bold.code,
            sized_bold.code,
            2,
            2,
            0
        ]
    );
}

fn sized_span(
    start: u32,
    end: u32,
    formats: &[CharacterFormat],
    size: Option<u32>,
) -> GeneratedFormatSpan {
    GeneratedFormatSpan {
        start,
        end,
        formats: formats.to_vec(),
        size,
    }
}
