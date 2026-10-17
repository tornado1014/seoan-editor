mod support;

use hlt_core::{
    CellFormatEdit, CellFormatEditDecisionStatus, CellFormatEditRequest, CellFormatEditSkipReason,
    CharacterFormat, ErrorCode, InspectOptions, RunStyleEdit, VerifyOptions, WriteOptions,
    document_revision, inspect_cell_format_inventory, inspect_path, plan_cell_format_edits,
    save_cell_format_edits_as_new, verify_path,
};
use sha2::{Digest, Sha256};

#[test]
fn planner_accepts_a_plain_body_cell_to_bold_by_stable_cell_id() {
    let source = support::pct_table_fixture();
    let request = request(
        source.path(),
        vec![operation(
            "cell:0:1:0",
            "재질",
            vec![run(0, 2, vec![CharacterFormat::Bold])],
        )],
    );

    let plan = plan_cell_format_edits(source.path(), &request).unwrap();

    assert_eq!(plan.base_revision, request.expected_revision);
    assert_eq!(plan.operation_count, 1);
    assert_eq!(plan.total_runs, 1);
    assert_eq!(plan.ready, 1);
    assert_eq!(plan.skipped, 0);
    assert_eq!(
        plan.decisions[0].status,
        CellFormatEditDecisionStatus::Ready
    );
    assert_eq!(plan.decisions[0].reason, None);
    assert_eq!(plan.decisions[0].cell_id, "cell:0:1:0");
    assert_eq!(plan.decisions[0].run_count, 1);

    let serialized = serde_json::to_string(&plan).unwrap();
    assert!(!serialized.contains("재질"));
    assert!(!serialized.contains("start_byte"));
    assert!(!serialized.contains("raw_code"));
    assert!(!serialized.contains("replacement"));
}

#[test]
fn planner_rejects_a_stale_revision() {
    let source = support::pct_table_fixture();
    let mut request = request(
        source.path(),
        vec![operation(
            "cell:0:1:0",
            "재질",
            vec![run(0, 2, vec![CharacterFormat::Bold])],
        )],
    );
    request.expected_revision ^= 1;

    let error = plan_cell_format_edits(source.path(), &request).unwrap_err();

    assert_eq!(error.code, ErrorCode::RevisionConflict);
    assert_eq!(error.phase, "cell_format_edit_revision");
}

#[test]
fn planner_skips_missing_mismatched_invalid_and_unchanged_cells() {
    let source = support::pct_table_fixture();
    let cases = [
        (
            operation(
                "cell:missing",
                "재질",
                vec![run(0, 2, vec![CharacterFormat::Bold])],
            ),
            CellFormatEditSkipReason::CellNotFound,
        ),
        (
            CellFormatEdit {
                cell_id: "cell:0:1:0".to_string(),
                expected_text_sha256: text_sha256("wrong"),
                runs: vec![run(0, 2, vec![CharacterFormat::Bold])],
            },
            CellFormatEditSkipReason::SourceMismatch,
        ),
        (
            operation(
                "cell:0:1:0",
                "재질",
                vec![run(1, 1, vec![CharacterFormat::Bold])],
            ),
            CellFormatEditSkipReason::EmptyRun,
        ),
        (
            operation(
                "cell:0:1:0",
                "재질",
                vec![run(1, 3, vec![CharacterFormat::Bold])],
            ),
            CellFormatEditSkipReason::RunOutOfRange,
        ),
        (
            operation(
                "cell:0:1:0",
                "재질",
                vec![
                    run(0, 2, vec![CharacterFormat::Bold]),
                    run(1, 2, Vec::new()),
                ],
            ),
            CellFormatEditSkipReason::OverlappingRuns,
        ),
        (
            operation(
                "cell:0:1:0",
                "재질",
                vec![run(
                    0,
                    2,
                    vec![CharacterFormat::Bold, CharacterFormat::Bold],
                )],
            ),
            CellFormatEditSkipReason::DuplicateFormat,
        ),
        (
            operation(
                "cell:0:1:0",
                "재질",
                vec![run(
                    0,
                    2,
                    vec![
                        CharacterFormat::Underline,
                        CharacterFormat::Bold,
                        CharacterFormat::Italic,
                        CharacterFormat::Superscript,
                        CharacterFormat::Subscript,
                    ],
                )],
            ),
            CellFormatEditSkipReason::StyleUnavailable,
        ),
        (
            operation(
                "cell:0:0:0",
                "구성품",
                vec![run(0, 3, vec![CharacterFormat::Bold])],
            ),
            CellFormatEditSkipReason::NoChange,
        ),
    ];

    for (operation, expected) in cases {
        let plan = plan_cell_format_edits(source.path(), &request(source.path(), vec![operation]))
            .unwrap();
        assert_eq!(plan.ready, 0);
        assert_eq!(plan.skipped, 1);
        assert_eq!(plan.decisions[0].reason, Some(expected));
    }
}

#[test]
fn planner_skips_duplicate_cell_ids() {
    let source = support::pct_table_fixture();
    let valid = operation(
        "cell:0:1:0",
        "재질",
        vec![run(0, 2, vec![CharacterFormat::Bold])],
    );

    let plan = plan_cell_format_edits(
        source.path(),
        &request(source.path(), vec![valid.clone(), valid]),
    )
    .unwrap();

    assert_eq!(plan.ready, 1);
    assert_eq!(plan.skipped, 1);
    assert_eq!(
        plan.decisions[1].reason,
        Some(CellFormatEditSkipReason::DuplicateCellId)
    );
}

#[test]
fn planner_rejects_more_than_500_cells_or_5000_runs() {
    let source = support::pct_table_fixture();
    let operation = operation(
        "cell:0:1:0",
        "재질",
        vec![run(0, 2, vec![CharacterFormat::Bold])],
    );
    let too_many_operations = CellFormatEditRequest {
        expected_revision: 0,
        operations: vec![operation.clone(); 501],
    };
    let too_many_runs = CellFormatEditRequest {
        expected_revision: 0,
        operations: vec![CellFormatEdit {
            runs: vec![run(0, 1, vec![CharacterFormat::Bold]); 5_001],
            ..operation
        }],
    };

    for request in [too_many_operations, too_many_runs] {
        let error = plan_cell_format_edits(source.path(), &request).unwrap_err();
        assert_eq!(error.code, ErrorCode::ResourceTooLarge);
        assert_eq!(error.phase, "cell_format_edit_operations");
    }
}

#[test]
fn inventory_text_hash_is_the_cell_planner_precondition() {
    let source = support::pct_table_fixture();
    let inventory = inspect_cell_format_inventory(source.path()).unwrap();
    let body = inventory
        .cells
        .iter()
        .find(|cell| cell.cell_id == "cell:0:1:0")
        .unwrap();
    let operation = CellFormatEdit {
        cell_id: body.cell_id.clone(),
        expected_text_sha256: text_sha256(&body.text),
        runs: vec![run(0, 2, vec![CharacterFormat::Bold])],
    };

    let plan =
        plan_cell_format_edits(source.path(), &request(source.path(), vec![operation])).unwrap();

    assert_eq!(plan.ready, 1);
}

#[test]
fn save_makes_a_plain_body_cell_bold_and_preserves_the_hlt_structure() {
    let source = support::pct_table_fixture();
    let source_sha = verify_path(source.path(), VerifyOptions::default())
        .unwrap()
        .input_sha256;
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("pct-cell-body-bold.HLT");
    let request = request(
        source.path(),
        vec![operation(
            "cell:0:1:0",
            "재질",
            vec![run(0, 2, vec![CharacterFormat::Bold])],
        )],
    );

    let result =
        save_cell_format_edits_as_new(source.path(), &output, &request, WriteOptions::default())
            .unwrap();

    assert_eq!(
        verify_path(source.path(), VerifyOptions::default())
            .unwrap()
            .input_sha256,
        source_sha
    );
    assert_eq!(result.input_sha256, source_sha);
    assert_ne!(result.output_sha256, result.input_sha256);
    assert!(result.structure.header_equal);
    assert!(result.structure.body_size_delta_equal);
    assert!(result.structure.counts_equal);
    assert!(result.structure.fixed_blocks_equal);
    assert!(result.structure.target_formats_equal);
    assert!(result.structure.non_target_formats_equal);
    assert!(result.structure.cell_text_equal);
    assert!(result.structure.paragraph_text_equal);
    assert!(result.structure.paragraph_shapes_equal);
    assert!(result.structure.tables_equal);
    assert!(result.structure.images_equal);
    assert!(result.structure.wipo_equal);
    assert!(result.structure.char_spans_valid);
    assert!(result.structure.body_changed);

    let inventory = inspect_cell_format_inventory(&output).unwrap();
    let cell = inventory
        .cells
        .iter()
        .find(|cell| cell.cell_id == "cell:0:1:0")
        .unwrap();
    assert_eq!(cell.text, "재질");
    assert_eq!(
        cell.spans
            .iter()
            .map(|span| (span.start, span.end, span.raw_attr))
            .collect::<Vec<_>>(),
        vec![(0, 2, 2)]
    );
}

#[test]
fn save_makes_a_bold_header_plain() {
    let source = support::pct_table_fixture();
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("pct-cell-header-plain.HLT");
    let request = request(
        source.path(),
        vec![operation(
            "cell:0:0:0",
            "구성품",
            vec![run(0, 3, Vec::new())],
        )],
    );

    let result =
        save_cell_format_edits_as_new(source.path(), &output, &request, WriteOptions::default())
            .unwrap();

    assert!(result.structure.target_formats_equal);
    assert!(result.structure.non_target_formats_equal);
    let inventory = inspect_cell_format_inventory(&output).unwrap();
    let cell = inventory
        .cells
        .iter()
        .find(|cell| cell.cell_id == "cell:0:0:0")
        .unwrap();
    assert_eq!(cell.text, "구성품");
    assert!(cell.spans.is_empty());
}

#[test]
fn save_partially_styles_one_cp949_character_without_touching_the_cell_boundary() {
    let source = support::pct_table_fixture();
    let before = inspect_path(source.path(), InspectOptions::default()).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("pct-cell-partial-bold.HLT");
    let request = request(
        source.path(),
        vec![operation(
            "cell:0:1:0",
            "재질",
            vec![run(0, 1, vec![CharacterFormat::Bold])],
        )],
    );

    let result =
        save_cell_format_edits_as_new(source.path(), &output, &request, WriteOptions::default())
            .unwrap();

    assert!(result.expected_inflated_delta >= 0);
    assert!(result.structure.tables_equal);
    let after = inspect_path(&output, InspectOptions::default()).unwrap();
    assert_eq!(before.tables, after.tables);
    assert_eq!(before.cells, after.cells);
    let inventory = inspect_cell_format_inventory(&output).unwrap();
    let cell = inventory
        .cells
        .iter()
        .find(|cell| cell.cell_id == "cell:0:1:0")
        .unwrap();
    assert_eq!(cell.text, "재질");
    assert_eq!(
        cell.spans
            .iter()
            .map(|span| (span.start, span.end, span.raw_attr))
            .collect::<Vec<_>>(),
        vec![(0, 1, 2)]
    );
}

#[test]
fn save_two_cells_in_one_table_paragraph_uses_original_cell_coordinates() {
    let source = support::pct_table_fixture();
    let inventory = inspect_cell_format_inventory(source.path()).unwrap();
    let targets = ["cell:0:1:0", "cell:0:1:1"];
    let operations = targets
        .iter()
        .map(|id| {
            let cell = inventory
                .cells
                .iter()
                .find(|cell| &cell.cell_id == id)
                .unwrap();
            operation(
                id,
                &cell.text,
                vec![run(
                    0,
                    cell.text.chars().count() as u32,
                    vec![CharacterFormat::Bold],
                )],
            )
        })
        .collect();
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("pct-two-cells-bold.HLT");

    let result = save_cell_format_edits_as_new(
        source.path(),
        &output,
        &request(source.path(), operations),
        WriteOptions::default(),
    )
    .unwrap();

    assert_eq!(result.edit_plan.ready, 2);
    assert!(result.structure.target_formats_equal);
    assert!(result.structure.non_target_formats_equal);
    let after = inspect_cell_format_inventory(&output).unwrap();
    for id in targets {
        let cell = after.cells.iter().find(|cell| cell.cell_id == id).unwrap();
        assert_eq!(cell.spans.len(), 1);
        assert_eq!(cell.spans[0].raw_attr, 2);
        assert_eq!(cell.spans[0].end, cell.text.chars().count() as u32);
    }
}

fn request(source: &std::path::Path, operations: Vec<CellFormatEdit>) -> CellFormatEditRequest {
    let verified = verify_path(source, VerifyOptions::default()).unwrap();
    CellFormatEditRequest {
        expected_revision: document_revision(&verified.input_sha256).unwrap(),
        operations,
    }
}

fn operation(cell_id: &str, source: &str, runs: Vec<RunStyleEdit>) -> CellFormatEdit {
    CellFormatEdit {
        cell_id: cell_id.to_string(),
        expected_text_sha256: text_sha256(source),
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
