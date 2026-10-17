mod support;

use hlt_core::{
    CharacterFormat, ErrorCode, FormatEditDecisionStatus, FormatEditRequest, FormatEditSkipReason,
    InspectOptions, ParagraphAlign, ParagraphFormatEdit, RunStyleEdit, VerifyOptions, WriteOptions,
    document_revision, inspect_format_inventory, inspect_path, plan_format_edits,
    save_format_edits_as_new, verify_path,
};
use sha2::{Digest, Sha256};

#[test]
fn planner_accepts_a_bold_range_and_matching_existing_center_shape() {
    let source = support::formatted_style_fixture();
    let request = request(
        source.path(),
        vec![operation(
            "paragraph:1",
            "Abold both H2O",
            Some(ParagraphAlign::Center),
            vec![run(0, 1, vec![CharacterFormat::Bold])],
        )],
    );

    let plan = plan_format_edits(source.path(), &request).unwrap();

    assert_eq!(plan.base_revision, request.expected_revision);
    assert_eq!(plan.operation_count, 1);
    assert_eq!(plan.total_runs, 1);
    assert_eq!(plan.ready, 1);
    assert_eq!(plan.skipped, 0);
    assert_eq!(plan.decisions[0].status, FormatEditDecisionStatus::Ready);
    assert_eq!(plan.decisions[0].reason, None);
    let serialized = serde_json::to_string(&plan).unwrap();
    assert!(!serialized.contains("Abold"));
    assert!(!serialized.contains("start_byte"));
    assert!(!serialized.contains("raw_code"));
}

#[test]
fn planner_rejects_a_stale_revision() {
    let source = support::formatted_style_fixture();
    let mut request = request(
        source.path(),
        vec![operation(
            "paragraph:1",
            "Abold both H2O",
            None,
            vec![run(0, 1, vec![CharacterFormat::Bold])],
        )],
    );
    request.expected_revision ^= 1;

    let error = plan_format_edits(source.path(), &request).unwrap_err();

    assert_eq!(error.code, ErrorCode::RevisionConflict);
    assert_eq!(error.phase, "format_edit_revision");
}

#[test]
fn planner_skips_invalid_targets_ranges_and_unavailable_runtime_styles() {
    let source = support::formatted_style_fixture();
    let cases = [
        (
            operation(
                "paragraph:missing",
                "Abold both H2O",
                None,
                vec![run(0, 1, vec![CharacterFormat::Bold])],
            ),
            FormatEditSkipReason::ParagraphNotFound,
        ),
        (
            ParagraphFormatEdit {
                paragraph_id: "paragraph:1".to_string(),
                expected_text_sha256: text_sha256("wrong"),
                align: None,
                runs: vec![run(0, 1, vec![CharacterFormat::Bold])],
            },
            FormatEditSkipReason::SourceMismatch,
        ),
        (
            operation(
                "paragraph:1",
                "Abold both H2O",
                None,
                vec![run(1, 1, vec![CharacterFormat::Bold])],
            ),
            FormatEditSkipReason::EmptyRun,
        ),
        (
            operation(
                "paragraph:1",
                "Abold both H2O",
                None,
                vec![run(13, 15, vec![CharacterFormat::Bold])],
            ),
            FormatEditSkipReason::RunOutOfRange,
        ),
        (
            operation(
                "paragraph:1",
                "Abold both H2O",
                None,
                vec![
                    run(0, 2, vec![CharacterFormat::Bold]),
                    run(1, 3, vec![CharacterFormat::Italic]),
                ],
            ),
            FormatEditSkipReason::OverlappingRuns,
        ),
        (
            operation(
                "paragraph:1",
                "Abold both H2O",
                None,
                vec![run(
                    0,
                    1,
                    vec![CharacterFormat::Bold, CharacterFormat::Bold],
                )],
            ),
            FormatEditSkipReason::DuplicateFormat,
        ),
        (
            operation(
                "paragraph:1",
                "Abold both H2O",
                None,
                vec![run(
                    0,
                    1,
                    vec![CharacterFormat::Bold, CharacterFormat::Subscript],
                )],
            ),
            FormatEditSkipReason::StyleUnavailable,
        ),
        (
            operation(
                "paragraph:4",
                "_",
                None,
                vec![run(0, 1, vec![CharacterFormat::Bold])],
            ),
            FormatEditSkipReason::ImageMarkerIntersection,
        ),
        (
            operation("paragraph:5", "", Some(ParagraphAlign::Left), Vec::new()),
            FormatEditSkipReason::ShapeUnavailable,
        ),
    ];

    for (operation, expected) in cases {
        let plan =
            plan_format_edits(source.path(), &request(source.path(), vec![operation])).unwrap();
        assert_eq!(plan.ready, 0);
        assert_eq!(plan.skipped, 1);
        assert_eq!(plan.decisions[0].reason, Some(expected));
    }
}

#[test]
fn planner_skips_duplicate_ids_and_paragraphs_with_unverified_used_attributes() {
    let source = support::formatted_style_fixture();
    let valid = operation(
        "paragraph:1",
        "Abold both H2O",
        None,
        vec![run(0, 1, vec![CharacterFormat::Bold])],
    );
    let plan = plan_format_edits(
        source.path(),
        &request(source.path(), vec![valid.clone(), valid]),
    )
    .unwrap();
    assert_eq!(plan.ready, 1);
    assert_eq!(plan.skipped, 1);
    assert_eq!(
        plan.decisions[1].reason,
        Some(FormatEditSkipReason::DuplicateParagraphId)
    );

    let unknown = support::formatted_style_fixture_with_attr(0, 8);
    let plan = plan_format_edits(
        unknown.path(),
        &request(
            unknown.path(),
            vec![operation(
                "paragraph:1",
                "Abold both H2O",
                Some(ParagraphAlign::Center),
                Vec::new(),
            )],
        ),
    )
    .unwrap();
    assert_eq!(
        plan.decisions[0].reason,
        Some(FormatEditSkipReason::UnverifiedSourceFormat)
    );
}

#[test]
fn planner_rejects_more_than_500_operations_or_5000_runs() {
    let source = support::formatted_style_fixture();
    let operation = operation(
        "paragraph:1",
        "Abold both H2O",
        None,
        vec![run(0, 1, vec![CharacterFormat::Bold])],
    );
    let too_many_operations = FormatEditRequest {
        expected_revision: 0,
        operations: vec![operation.clone(); 501],
    };
    let too_many_runs = FormatEditRequest {
        expected_revision: 0,
        operations: vec![ParagraphFormatEdit {
            runs: vec![run(0, 1, vec![CharacterFormat::Bold]); 5_001],
            ..operation
        }],
    };

    for request in [too_many_operations, too_many_runs] {
        let error = plan_format_edits(source.path(), &request).unwrap_err();
        assert_eq!(error.code, ErrorCode::ResourceTooLarge);
        assert_eq!(error.phase, "format_edit_operations");
    }
}

#[test]
fn planner_requires_a_cell_id_for_any_paragraph_containing_table_lines() {
    let source = support::pct_table_fixture();
    let document = inspect_path(source.path(), InspectOptions::default()).unwrap();
    let first_cell = &document.cells[0];
    let first_line = document
        .lines
        .iter()
        .find(|line| line.id == first_cell.line_ids[0])
        .unwrap();
    let inventory = inspect_format_inventory(source.path()).unwrap();
    let paragraph = inventory
        .paragraphs
        .iter()
        .find(|paragraph| paragraph.paragraph_id == first_line.paragraph_id)
        .unwrap();
    let plan = plan_format_edits(
        source.path(),
        &request(
            source.path(),
            vec![operation(
                &paragraph.paragraph_id,
                &paragraph.text,
                None,
                vec![run(0, 1, vec![CharacterFormat::Bold])],
            )],
        ),
    )
    .unwrap();

    assert_eq!(plan.ready, 0);
    assert_eq!(plan.skipped, 1);
    assert_eq!(
        plan.decisions[0].reason,
        Some(FormatEditSkipReason::TableParagraphRequiresCellId)
    );
}

#[test]
fn save_rebuilds_only_touched_character_spans_and_preserves_document_semantics() {
    let source = support::formatted_style_fixture_with_attr(8, 34);
    let source_sha = verify_path(source.path(), VerifyOptions::default())
        .unwrap()
        .input_sha256;
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("formatted-style-save.HLT");
    let request = request(
        source.path(),
        vec![operation(
            "paragraph:1",
            "Abold both H2O",
            None,
            vec![
                run(0, 1, vec![CharacterFormat::Underline]),
                run(1, 6, Vec::new()),
                run(11, 12, vec![CharacterFormat::Bold]),
                run(
                    12,
                    13,
                    vec![CharacterFormat::Bold, CharacterFormat::Subscript],
                ),
                run(13, 14, vec![CharacterFormat::Bold]),
            ],
        )],
    );

    let result =
        save_format_edits_as_new(source.path(), &output, &request, WriteOptions::default())
            .unwrap();

    assert_eq!(
        verify_path(source.path(), VerifyOptions::default())
            .unwrap()
            .input_sha256,
        source_sha
    );
    assert_eq!(result.input_sha256, source_sha);
    assert_ne!(result.output_sha256, result.input_sha256);
    assert_eq!(result.expected_inflated_delta, 0);
    assert!(result.structure.header_equal);
    assert!(result.structure.body_size_delta_equal);
    assert!(result.structure.counts_equal);
    assert!(result.structure.fixed_blocks_equal);
    assert!(result.structure.target_formats_equal);
    assert!(result.structure.non_target_formats_equal);
    assert!(result.structure.paragraph_text_equal);
    assert!(result.structure.paragraph_shapes_equal);
    assert!(result.structure.tables_equal);
    assert!(result.structure.images_equal);
    assert!(result.structure.wipo_equal);
    assert!(result.structure.char_spans_valid);
    assert!(result.structure.body_changed);

    let inventory = inspect_format_inventory(&output).unwrap();
    let paragraph = inventory
        .paragraphs
        .iter()
        .find(|paragraph| paragraph.paragraph_id == "paragraph:1")
        .unwrap();
    assert_eq!(paragraph.text, "Abold both H2O");
    assert_eq!(
        paragraph
            .spans
            .iter()
            .map(|span| (span.start, span.end, span.raw_attr))
            .collect::<Vec<_>>(),
        vec![
            (0, 1, 1),
            (6, 10, 6),
            (11, 12, 2),
            (12, 13, 34),
            (13, 14, 2)
        ]
    );
    assert!(inventory.unresolved.is_empty());

    let source_images = inspect_path(
        source.path(),
        InspectOptions {
            include_image_identities: true,
            ..InspectOptions::default()
        },
    )
    .unwrap()
    .images;
    let output_images = inspect_path(
        &output,
        InspectOptions {
            include_image_identities: true,
            ..InspectOptions::default()
        },
    )
    .unwrap()
    .images;
    assert_eq!(source_images, output_images);
}

#[test]
fn save_expands_a_partially_touched_compressed_span() {
    let source = support::formatted_style_fixture();
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("compressed-style-save.HLT");
    let request = request(
        source.path(),
        vec![operation(
            "paragraph:0",
            "【발명의 명칭】",
            None,
            vec![run(0, 1, vec![CharacterFormat::Underline])],
        )],
    );

    let result =
        save_format_edits_as_new(source.path(), &output, &request, WriteOptions::default())
            .unwrap();

    assert_eq!(result.expected_inflated_delta, 60);
    assert!(result.structure.target_formats_equal);
    assert!(result.structure.non_target_formats_equal);
    let inventory = inspect_format_inventory(&output).unwrap();
    let paragraph = inventory
        .paragraphs
        .iter()
        .find(|paragraph| paragraph.paragraph_id == "paragraph:0")
        .unwrap();
    assert_eq!(paragraph.text, "【발명의 명칭】");
    assert_eq!(paragraph.spans[0].start, 0);
    assert_eq!(paragraph.spans[0].end, 1);
    assert_eq!(paragraph.spans[0].raw_attr, 1);
}

#[test]
fn save_reuses_one_existing_shape_for_every_line_of_a_multiline_paragraph() {
    let source = support::formatted_style_fixture();
    let before = inspect_path(source.path(), InspectOptions::default()).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("multiline-center-save.HLT");
    let request = request(
        source.path(),
        vec![operation(
            "paragraph:2",
            "【도면】",
            Some(ParagraphAlign::Center),
            Vec::new(),
        )],
    );

    let result =
        save_format_edits_as_new(source.path(), &output, &request, WriteOptions::default())
            .unwrap();

    assert_eq!(result.expected_inflated_delta, 0);
    assert!(result.structure.paragraph_shapes_equal);
    assert!(result.structure.paragraph_text_equal);
    assert!(result.structure.target_formats_equal);
    assert!(result.structure.non_target_formats_equal);
    assert!(result.structure.fixed_blocks_equal);
    let after = inspect_path(&output, InspectOptions::default()).unwrap();
    let target = after
        .paragraphs
        .iter()
        .find(|paragraph| paragraph.id == "paragraph:2")
        .unwrap();
    // 표본 바이트 실측(2026-08-27): 문단모양 3 이 (들여쓰기 0, 정렬 1=가운데, 줄 높이 280)이다.
    // 모양 11 은 줄 높이가 0 이다. 예전 기대값 11 은 판독기가 줄 높이를 한 레코드 앞에서
    // 읽던 결함의 그림자였다(모양 11 이 모양 10 의 280 을 물고 나왔다).
    assert_eq!(target.shape_id.as_deref(), Some("shape:3"));
    assert_eq!(target.align, "center");
    assert_eq!(target.indent_raw, 0);
    assert_eq!(target.lspace, 280);
    for paragraph_id in ["paragraph:0", "paragraph:1", "paragraph:3", "paragraph:4"] {
        let source_paragraph = before
            .paragraphs
            .iter()
            .find(|paragraph| paragraph.id == paragraph_id)
            .unwrap();
        let output_paragraph = after
            .paragraphs
            .iter()
            .find(|paragraph| paragraph.id == paragraph_id)
            .unwrap();
        assert_eq!(source_paragraph.shape_id, output_paragraph.shape_id);
        assert_eq!(source_paragraph.align, output_paragraph.align);
        assert_eq!(source_paragraph.indent_raw, output_paragraph.indent_raw);
        assert_eq!(source_paragraph.lspace, output_paragraph.lspace);
    }
}

fn request(source: &std::path::Path, operations: Vec<ParagraphFormatEdit>) -> FormatEditRequest {
    let verified = verify_path(source, VerifyOptions::default()).unwrap();
    FormatEditRequest {
        expected_revision: document_revision(&verified.input_sha256).unwrap(),
        operations,
    }
}

fn operation(
    paragraph_id: &str,
    source: &str,
    align: Option<ParagraphAlign>,
    runs: Vec<RunStyleEdit>,
) -> ParagraphFormatEdit {
    ParagraphFormatEdit {
        paragraph_id: paragraph_id.to_string(),
        expected_text_sha256: text_sha256(source),
        align,
        runs,
    }
}

fn run(start: u32, end: u32, formats: Vec<CharacterFormat>) -> RunStyleEdit {
    RunStyleEdit {
        start,
        end,
        formats,
    }
}

fn text_sha256(text: &str) -> String {
    format!("{:X}", Sha256::digest(text.as_bytes()))
}
