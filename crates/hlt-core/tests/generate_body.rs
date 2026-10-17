use hlt_core::{
    CharacterFormat, ErrorCode, GeneratedLineEnding, GenerationDialect, HmlCell, HmlDocument,
    HmlImage, HmlInline, HmlParagraph, HmlSection, HmlSummary, HmlTable, generate_line_array,
    plan_hlt_body,
};

fn document(content: &str) -> HmlDocument {
    HmlDocument {
        source: "fixture.hml".into(),
        source_sha256: "0".repeat(64),
        encoding: "cp949".into(),
        title: "시험".into(),
        fonts: vec!["신명조".into(); 3],
        summary: HmlSummary {
            section_count: 2,
            top_level_paragraph_count: 4,
            content_paragraph_count: 2,
            table_count: 0,
            row_count: 0,
            cell_count: 0,
            image_count: 1,
        },
        sections: vec![
            HmlSection {
                id: "section:1".into(),
                index: 1,
                title: "【발명의 설명】".into(),
                kind: "section".into(),
                level: 0,
                paragraph_ids: vec!["paragraph:1".into(), "paragraph:2".into()],
            },
            HmlSection {
                id: "section:2".into(),
                index: 2,
                title: "【청구범위】".into(),
                kind: "section".into(),
                level: 0,
                paragraph_ids: vec!["paragraph:3".into(), "paragraph:4".into()],
            },
        ],
        paragraphs: vec![
            paragraph(
                "paragraph:1",
                Some("section:1"),
                "【발명의 설명】",
                Some("【발명의 설명】"),
                "center",
                0,
                vec![HmlInline::Text {
                    text: "【발명의 설명】".into(),
                    formats: vec![],
                    size: None,
                }],
            ),
            paragraph(
                "paragraph:2",
                Some("section:1"),
                content,
                None,
                "justified",
                1000,
                vec![
                    HmlInline::Text {
                        text: "가".into(),
                        formats: vec![],
                        size: None,
                    },
                    HmlInline::Text {
                        text: "A".into(),
                        formats: vec![CharacterFormat::Bold],
                        size: None,
                    },
                    HmlInline::Image {
                        image_id: "image:0".into(),
                    },
                    HmlInline::Text {
                        text: "끝".into(),
                        formats: vec![],
                        size: None,
                    },
                ],
            ),
            paragraph(
                "paragraph:3",
                Some("section:2"),
                "【청구범위】",
                Some("【청구범위】"),
                "center",
                0,
                vec![HmlInline::Text {
                    text: "【청구범위】".into(),
                    formats: vec![],
                    size: None,
                }],
            ),
            paragraph(
                "paragraph:4",
                Some("section:2"),
                "청구",
                None,
                "justified",
                1000,
                vec![HmlInline::Text {
                    text: "청구".into(),
                    formats: vec![],
                    size: None,
                }],
            ),
        ],
        tables: vec![],
        cells: vec![],
        images: vec![HmlImage {
            id: "image:0".into(),
            occurrence: 0,
            name: "x.png".into(),
            width: Some(10),
            height: Some(10),
            secondary_width: Some(8),
            secondary_height: Some(8),
            paragraph_id: Some("paragraph:2".into()),
            cell_id: None,
        }],
    }
}

fn paragraph(
    id: &str,
    section: Option<&str>,
    text: &str,
    marker: Option<&str>,
    align: &str,
    indent: i32,
    inlines: Vec<HmlInline>,
) -> HmlParagraph {
    HmlParagraph {
        id: id.into(),
        section_id: section.map(str::to_string),
        marker: marker.map(str::to_string),
        text: text.into(),
        tail_text: String::new(),
        align: align.into(),
        indent_raw: indent,
        lmargin_raw: 0,
        rmargin_raw: 0,
        lspace: 280,
        page_break: false,
        table_id: None,
        image_ids: if id == "paragraph:2" {
            vec!["image:0".into()]
        } else {
            vec![]
        },
        inlines,
        source_line_bytes: Vec::new(),
        number: None,
    }
}

#[test]
fn plans_sections_body_formats_images_shapes_and_wipo_ids() {
    let plan = plan_hlt_body(&document("가A끝"), GenerationDialect::PatentSpecificationKo).unwrap();
    assert_eq!(plan.section_table.len(), 134);
    assert_eq!(
        u32::from_le_bytes(plan.section_table[51..55].try_into().unwrap()),
        15
    );
    assert_eq!(
        u32::from_le_bytes(plan.section_table[63..67].try_into().unwrap()),
        0
    );
    let expected = "【발명의 설명】\x15\r\n가A_끝\x15\r\n\x0c\r\n【청구범위】\x15\r\n청구\x15\r\n";
    assert_eq!(plan.body, encoding_rs::EUC_KR.encode(expected).0.as_ref());
    assert_eq!(plan.lines.len(), 5);
    assert_eq!(
        plan.lines
            .iter()
            .map(|line| line.shape_index)
            .collect::<Vec<_>>(),
        [11, 5, 11, 11, 5]
    );
    assert_eq!(
        plan.lines
            .iter()
            .map(|line| line.wipo_id)
            .collect::<Vec<_>>(),
        [None, Some(1), None, None, Some(2)]
    );
    assert_eq!(plan.lines[1].formats[0].start, 1);
    assert_eq!(plan.lines[1].formats[0].end, 2);
    assert_eq!(plan.lines[1].formats[0].formats, [CharacterFormat::Bold]);
    assert_eq!(plan.images[0].line_index, 1);
    assert_eq!(plan.images[0].character_index, 2);
    assert_eq!(
        plan.lines
            .iter()
            .map(|line| line.ending)
            .collect::<Vec<_>>(),
        [
            GeneratedLineEnding::Paragraph,
            GeneratedLineEnding::Paragraph,
            GeneratedLineEnding::PageBreak,
            GeneratedLineEnding::Paragraph,
            GeneratedLineEnding::Paragraph,
        ]
    );
}

#[test]
fn wraps_long_korean_text_without_splitting_cp949_and_reuses_one_wipo_id() {
    let long = "가".repeat(40);
    let mut doc = document(&long);
    doc.paragraphs[1].inlines = vec![HmlInline::Text {
        text: long,
        formats: vec![],
        size: None,
    }];
    doc.paragraphs[1].image_ids.clear();
    doc.images.clear();
    doc.summary.image_count = 0;
    let plan = plan_hlt_body(&doc, GenerationDialect::PatentSpecificationKo).unwrap();
    let lines: Vec<_> = plan
        .lines
        .iter()
        .filter(|line| line.paragraph_id == "paragraph:2")
        .collect();
    // 원본 줄나눔이 없으면 통특기 저장 줄나눔(R15): 본문 9,355.6 twip − 첫 줄 들여쓰기 1,000 → 한글 34자.
    // (옛 판은 지어낸 「39칸」 규칙이었다 · domestic 인쇄 첫 줄 들여쓰기 800 = 35자와 같은 식.)
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].text.chars().count(), 34);
    assert_eq!(lines[1].text.chars().count(), 6);
    assert_eq!((lines[0].wipo_id, lines[1].wipo_id), (Some(1), Some(1)));
}

#[test]
fn rejects_non_cp949_and_hidden_structure_controls() {
    for value in ["😀", "금지\u{15}"] {
        let mut doc = document(value);
        doc.paragraphs[1].text = value.into();
        doc.paragraphs[1].inlines = vec![HmlInline::Text {
            text: value.into(),
            formats: vec![],
            size: None,
        }];
        let error = plan_hlt_body(&doc, GenerationDialect::PatentSpecificationKo).unwrap_err();
        assert!(matches!(
            error.code,
            ErrorCode::Cp949Invalid | ErrorCode::StructureInvalid
        ));
    }
}

#[test]
fn emits_table_cells_row_controls_tail_wipo_and_cell_line_ranges() {
    let mut doc = document("unused");
    doc.sections = vec![HmlSection {
        id: "section:1".into(),
        index: 1,
        title: "【표 1】".into(),
        kind: "table".into(),
        level: 2,
        paragraph_ids: vec!["paragraph:1".into(), "paragraph:2".into()],
    }];
    doc.paragraphs = vec![
        paragraph(
            "paragraph:1",
            Some("section:1"),
            "【표 1】",
            Some("【표 1】"),
            "justified",
            0,
            vec![HmlInline::Text {
                text: "【표 1】".into(),
                formats: vec![],
                size: None,
            }],
        ),
        HmlParagraph {
            id: "paragraph:2".into(),
            section_id: Some("section:1".into()),
            marker: None,
            text: "뒤".into(),
            tail_text: "뒤".into(),
            align: "justified".into(),
            indent_raw: 1000,
            lmargin_raw: 0,
            rmargin_raw: 0,
            lspace: 280,
            page_break: false,
            table_id: Some("table:1".into()),
            image_ids: vec![],
            inlines: vec![HmlInline::Text {
                text: "뒤".into(),
                formats: vec![],
                size: None,
            }],
            source_line_bytes: Vec::new(),
            number: None,
        },
    ];
    doc.tables = vec![HmlTable {
        id: "table:1".into(),
        occurrence: 1,
        paragraph_id: "paragraph:2".into(),
        max_columns: 2,
        row_count: 2,
        cell_ids: vec![
            "cell:1:1:1".into(),
            "cell:1:1:2".into(),
            "cell:1:2:1".into(),
            "cell:1:2:2".into(),
        ],
    }];
    doc.cells = vec![
        gen_cell(
            "cell:1:1:1",
            1,
            1,
            "A",
            vec![HmlInline::Text {
                text: "A".into(),
                formats: vec![CharacterFormat::Bold],
                size: None,
            }],
            false,
        ),
        gen_cell(
            "cell:1:1:2",
            1,
            2,
            "B",
            vec![HmlInline::Text {
                text: "B".into(),
                formats: vec![],
                size: None,
            }],
            false,
        ),
        gen_cell("cell:1:2:1", 2, 1, "", vec![], true),
        gen_cell(
            "cell:1:2:2",
            2,
            2,
            "D",
            vec![
                HmlInline::Image {
                    image_id: "image:0".into(),
                },
                HmlInline::Text {
                    text: "D".into(),
                    formats: vec![],
                    size: None,
                },
            ],
            false,
        ),
    ];
    doc.images = vec![HmlImage {
        id: "image:0".into(),
        occurrence: 0,
        name: "x.png".into(),
        width: Some(10),
        height: Some(10),
        secondary_width: Some(8),
        secondary_height: Some(8),
        paragraph_id: Some("paragraph:2".into()),
        cell_id: Some("cell:1:2:2".into()),
    }];
    doc.summary = HmlSummary {
        section_count: 1,
        top_level_paragraph_count: 2,
        content_paragraph_count: 1,
        table_count: 1,
        row_count: 2,
        cell_count: 4,
        image_count: 1,
    };
    let plan = plan_hlt_body(&doc, GenerationDialect::PatentSpecificationKo).unwrap();
    let expected = "【표 1】\x15\r\nA\x13\r\nB\x13\r\n\x12\r\n\x13\r\n_D\x13\r\n\x12\r\n뒤\x15\r\n";
    assert_eq!(plan.body, encoding_rs::EUC_KR.encode(expected).0.as_ref());
    // 표 칸에 든 줄은 줄 높이 130 짜리 모양(국내 9번, left)을 쓴다. 실물 12건
    // 1,160줄 전수에서 예외가 없다. 예전 기대값 7 은 줄 높이 280 이라, 입력이
    // 셀 줄 높이를 안 실어 오는 경로(.docx, KIPO XML)에서 표를 배 넘게 늘렸다.
    // 2026-09-06: 절 표제(앵커) 줄은 들여쓰기 0 + 왼쪽 정렬(국내 7번)이다. 통특기 저장본 4건의 앵커
    // 305개가 전부 그렇고(가운데 정렬 소수 제외), 들여쓰기 800 이던 domestic 정본은 사용자가 결함으로 판정했다.
    assert_eq!(
        plan.lines.iter().map(|l| l.shape_index).collect::<Vec<_>>(),
        [7, 9, 9, 9, 9, 9, 9, 5]
    );
    assert_eq!(
        plan.lines.iter().map(|l| l.wipo_id).collect::<Vec<_>>(),
        [None, None, None, None, None, None, None, Some(1)]
    );
    assert_eq!(
        plan.cell_ranges
            .iter()
            .map(|r| (r.cell_id.as_str(), r.line_start, r.line_end))
            .collect::<Vec<_>>(),
        [
            ("cell:1:1:1", 1, 1),
            ("cell:1:1:2", 2, 2),
            ("cell:1:2:1", 4, 4),
            ("cell:1:2:2", 5, 5)
        ]
    );
    assert_eq!(
        (plan.images[0].line_index, plan.images[0].character_index),
        (5, 0)
    );
    assert_eq!(
        plan.lines
            .iter()
            .map(|line| line.ending)
            .collect::<Vec<_>>(),
        [
            GeneratedLineEnding::Paragraph,
            GeneratedLineEnding::Cell,
            GeneratedLineEnding::Cell,
            GeneratedLineEnding::Row,
            GeneratedLineEnding::Cell,
            GeneratedLineEnding::Cell,
            GeneratedLineEnding::Row,
            GeneratedLineEnding::Paragraph,
        ]
    );
    let array = generate_line_array(&doc, &plan).unwrap();
    let cell_indices: Vec<_> = array
        .chunks_exact(12)
        .map(|record| u32::from_le_bytes(record[4..8].try_into().unwrap()))
        .collect();
    assert_eq!(cell_indices, [0, 1, 2, 2, 3, 4, 4, 0]);
}

fn gen_cell(
    id: &str,
    row: u32,
    occurrence: u32,
    text: &str,
    inlines: Vec<HmlInline>,
    continuation: bool,
) -> HmlCell {
    HmlCell {
        id: id.into(),
        table_id: "table:1".into(),
        row,
        occurrence,
        text: text.into(),
        start_raw: None,
        width_raw: None,
        hwidth_raw: Some(1200),
        column_begin: Some(occurrence),
        row_begin: Some(row),
        rowspan: 1,
        colspan: 1,
        merge_continuation: continuation,
        align: "left".into(),
        indent_raw: 0,
        lspace: 280,
        image_ids: if id.ends_with("2:2") {
            vec!["image:0".into()]
        } else {
            vec![]
        },
        inlines,
    }
}

#[test]
fn real_docx_centered_headings_use_the_native_flat_line_spacing_shape() {
    // 네이티브 저장본은 가운데 정렬, 들여쓰기 0, 줄간격 160 문단모양(내장 PCT 프로필 1번)을
    // 상단 앵커 4종, 표제 3종, 도면 문단 1건에 쓴다. 【청구범위】,【요약서】,【도면】 은
    // 앞의 쪽나눔 줄도 같은 문단모양을 받는다.
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../work/pct_a/diagnostics/pct_a_source.docx");
    let document = hlt_core::inspect_docx_path(&path).unwrap().document;
    let body = plan_hlt_body(&document, GenerationDialect::PctPatent).unwrap();

    let flat: Vec<_> = body
        .lines
        .iter()
        .filter(|line| line.shape_index == 1)
        .map(|line| line.text.as_str())
        .collect();
    assert_eq!(
        flat,
        [
            "【발명의 설명】",
            "관련 출원",
            "발명의 분야",
            "정부 지원",
            "\u{c}",
            "【청구범위】",
            "\u{c}",
            "【요약서】",
            "\u{c}",
            "【도면】",
            "_"
        ]
    );
}
