mod support;

use std::{fs, io::Read, path::Path};

use encoding_rs::EUC_KR;
use flate2::read::DeflateDecoder;
use hlt_core::{
    EditSkipReason, ErrorCode, HLT_HEADER_LEN, InspectDocument, ParagraphTextEdit,
    VariableTextEditRequest, VerifyOptions, WriteOptions, document_revision, inspect_path,
    plan_variable_text_edits, read_layout, save_variable_text_as_new, verify_path,
};
use sha2::{Digest, Sha256};

#[test]
fn multiline_edit_preserves_every_mapped_break_and_rebuilds_each_character_span() {
    let source = support::pct_multiline_fixture();
    let source_sha_before = file_sha256(source.path());
    let input = inspect_path(source.path(), Default::default()).unwrap();
    let target = input
        .paragraphs
        .iter()
        .find(|paragraph| paragraph.id == "paragraph:4")
        .unwrap();
    let old_text = target.text.clone();
    assert!(old_text.starts_with("구성품xt-값xx재질x"));
    assert_eq!(target.line_ids.len(), 25);
    let input_body = decoded_body(source.path());
    let input_lines = paragraph_line_texts(&input, &input_body, "paragraph:4");
    assert_eq!(input_lines.len(), 25);
    drop(input_body);

    let replacement = format!("새로운 {old_text}");
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("multiline-variable.HLT");
    let request = request(source.path(), "paragraph:4", &old_text, &replacement);

    let result =
        save_variable_text_as_new(source.path(), &output, &request, WriteOptions::default())
            .unwrap();

    let document = inspect_path(&output, Default::default()).unwrap();
    let paragraph = document
        .paragraphs
        .iter()
        .find(|paragraph| paragraph.id == "paragraph:4")
        .unwrap();
    assert_eq!(paragraph.text, replacement);
    assert_eq!(paragraph.line_ids, target.line_ids);
    let output_body = decoded_body(&output);
    let output_lines = paragraph_line_texts(&document, &output_body, "paragraph:4");
    assert_eq!(output_lines[0], format!("새로운 {}", input_lines[0]));
    assert_eq!(&output_lines[1..], &input_lines[1..]);

    let spans = span_codes_through(
        &output_body,
        paragraph
            .line_ids
            .iter()
            .map(|id| line_index(&document, id))
            .max()
            .unwrap(),
    );
    for (offset, line_id) in paragraph.line_ids.iter().enumerate() {
        let line = document
            .lines
            .iter()
            .find(|line| &line.id == line_id)
            .unwrap();
        let raw = &output_body
            [line.source_range.start_byte as usize..line.source_range.end_byte as usize];
        let codes = &spans[line.index as usize];
        let expected_codes = raw.len() + usize::from(offset + 1 == paragraph.line_ids.len());
        assert_eq!(codes.len(), expected_codes);
        assert_cp949_pairs_share_codes(raw, codes);
        if offset + 1 == paragraph.line_ids.len() {
            assert_eq!(codes.last(), Some(&0));
        }
    }
    assert_eq!(document.summary.line_count, input.summary.line_count);
    assert_eq!(document.summary.span_count, input.summary.span_count);
    assert!(result.structure.target_paragraphs_equal);
    assert!(result.structure.non_target_semantics_equal);
    assert!(result.structure.fixed_blocks_equal);
    assert!(result.structure.tables_equal);
    assert!(result.structure.images_equal);
    assert!(result.structure.wipo_equal);
    assert!(result.structure.char_spans_valid);
    assert_eq!(file_sha256(source.path()), source_sha_before);
    assert!(
        verify_path(&output, VerifyOptions::default())
            .unwrap()
            .window_ok
    );
}

#[test]
fn grows_and_shrinks_one_line_while_rebuilding_the_character_span() {
    let cases = [("구성품 심층 분석 결과", 89_i64), ("구성품 분석", 39_i64)];
    for (index, (replacement, expected_delta)) in cases.into_iter().enumerate() {
        let source = support::pct_table_fixture();
        let source_sha_before = file_sha256(source.path());
        let input = inspect_path(source.path(), Default::default()).unwrap();
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join(format!("variable-{index}.HLT"));
        let request = request(
            source.path(),
            "paragraph:3",
            "구성품 시험 결과",
            replacement,
        );

        let result =
            save_variable_text_as_new(source.path(), &output, &request, WriteOptions::default())
                .unwrap();

        let document = inspect_path(&output, Default::default()).unwrap();
        assert_eq!(
            document
                .paragraphs
                .iter()
                .find(|paragraph| paragraph.id == "paragraph:3")
                .unwrap()
                .text,
            replacement
        );
        assert_eq!(result.expected_inflated_delta, expected_delta);
        assert_eq!(
            document.summary.inflated_bytes as i128,
            input.summary.inflated_bytes as i128 + i128::from(expected_delta)
        );
        assert_eq!(document.summary.section_count, 3);
        assert_eq!(document.summary.paragraph_count, 6);
        assert_eq!(document.summary.line_count, 29);
        assert_eq!(document.summary.span_count, 29);
        assert_eq!(document.summary.table_count, 1);
        assert_eq!(document.summary.cell_count, 16);
        assert_eq!(document.summary.fixed_blocks, input.summary.fixed_blocks);
        assert!(result.structure.header_equal);
        assert!(result.structure.inflated_delta_equal);
        assert!(result.structure.counts_equal);
        assert!(result.structure.fixed_blocks_equal);
        assert!(result.structure.target_paragraphs_equal);
        assert!(result.structure.non_target_semantics_equal);
        assert!(result.structure.tables_equal);
        assert!(result.structure.images_equal);
        assert!(result.structure.wipo_equal);
        assert!(result.structure.char_spans_valid);
        assert!(result.structure.body_changed);
        assert_eq!(file_sha256(source.path()), source_sha_before);
        assert!(
            verify_path(&output, VerifyOptions::default())
                .unwrap()
                .window_ok
        );
    }
}

#[test]
fn variable_planner_keeps_control_lines_and_marker_changes_fail_closed() {
    let source = support::pct_table_fixture();
    let request = VariableTextEditRequest {
        expected_revision: revision(source.path()),
        operations: vec![
            ParagraphTextEdit {
                paragraph_id: "paragraph:4".to_string(),
                expected_text_sha256: text_sha256(
                    "구성품\u{13}t-값\u{13}\u{12}재질\u{13}< 1.5e-12 (p < 또는 = 0.001)\u{13}\u{12}두께\u{13}0.0314159 (p < 또는 = 0.05)\u{13}\u{12}표면 색\u{13}4.120e-10 (p < 또는 = 0.001)\u{13}\u{12}길이\u{13}0.0002718 (p < 또는 = 0.001)\u{13}\u{12}무게\u{13}0.0016180 (p < 또는 = 0.01)\u{13}\u{12}표면 처리 재질\u{13}0.0007071 (p < 또는 = 0.001)\u{13}\u{12}무게별 정렬\u{13}0.0141421 (p < 또는 = 0.05)\u{13}\u{12}표 아래 본문이 정상적으로 이어지는지 확인하는 문단이다.",
                ),
                replacement: "unused replacement".to_string(),
            },
            ParagraphTextEdit {
                paragraph_id: "paragraph:3".to_string(),
                expected_text_sha256: text_sha256("구성품 시험 결과"),
                replacement: "구성품_분석 결과".to_string(),
            },
        ],
    };

    let plan = plan_variable_text_edits(source.path(), &request).unwrap();

    assert_eq!(plan.ready, 0);
    assert_eq!(plan.skipped, 2);
    assert_eq!(
        plan.decisions[0].reason,
        Some(EditSkipReason::HiddenControlsUnsupported)
    );
    assert_eq!(
        plan.decisions[1].reason,
        Some(EditSkipReason::ImageMarkerChanged)
    );
}

#[test]
fn variable_save_rejects_stale_zero_ready_same_and_existing_outputs() {
    let source = support::pct_table_fixture();
    let directory = tempfile::tempdir().unwrap();
    let absent = directory.path().join("absent.HLT");
    let mut stale = request(
        source.path(),
        "paragraph:3",
        "구성품 시험 결과",
        "구성품 심층 분석 결과",
    );
    stale.expected_revision ^= 1;
    let error = plan_variable_text_edits(source.path(), &stale).unwrap_err();
    assert_eq!(error.code, ErrorCode::RevisionConflict);

    let no_ready = request(
        source.path(),
        "paragraph:missing",
        "구성품 시험 결과",
        "구성품 심층 분석 결과",
    );
    let error =
        save_variable_text_as_new(source.path(), &absent, &no_ready, WriteOptions::default())
            .unwrap_err();
    assert_eq!(error.code, ErrorCode::ConversionFailed);
    assert!(!absent.exists());

    let valid = request(
        source.path(),
        "paragraph:3",
        "구성품 시험 결과",
        "구성품 심층 분석 결과",
    );
    let error = save_variable_text_as_new(
        source.path(),
        source.path(),
        &valid,
        WriteOptions::default(),
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::OutputSameAsInput);

    let existing = directory.path().join("existing.HLT");
    std::fs::write(&existing, b"protected").unwrap();
    let error =
        save_variable_text_as_new(source.path(), &existing, &valid, WriteOptions::default())
            .unwrap_err();
    assert_eq!(error.code, ErrorCode::OutputExists);
    assert_eq!(std::fs::read(existing).unwrap(), b"protected");
}

#[test]
fn variable_planner_rejects_caller_breaks_controls_and_non_cp949_text() {
    let source = support::pct_table_fixture();
    for replacement in [
        "구성품\r\n분석 결과",
        "구성품\r분석 결과",
        "구성품\n분석 결과",
        "구성품\0분석 결과",
        "구성품\u{0012}분석 결과",
        "구성품\u{0013}분석 결과",
        "구성품\u{0015}분석 결과",
    ] {
        let plan = plan_variable_text_edits(
            source.path(),
            &request(
                source.path(),
                "paragraph:3",
                "구성품 시험 결과",
                replacement,
            ),
        )
        .unwrap();
        assert_eq!(
            plan.decisions[0].reason,
            Some(EditSkipReason::HiddenControlsUnsupported),
            "{replacement:?}"
        );
    }

    let plan = plan_variable_text_edits(
        source.path(),
        &request(
            source.path(),
            "paragraph:3",
            "구성품 시험 결과",
            "구성품 🧪 분석 결과",
        ),
    )
    .unwrap();
    assert_eq!(
        plan.decisions[0].reason,
        Some(EditSkipReason::Cp949Unrepresentable)
    );
}

fn request(
    source: &Path,
    paragraph_id: &str,
    old_text: &str,
    replacement: &str,
) -> VariableTextEditRequest {
    VariableTextEditRequest {
        expected_revision: revision(source),
        operations: vec![ParagraphTextEdit {
            paragraph_id: paragraph_id.to_string(),
            expected_text_sha256: text_sha256(old_text),
            replacement: replacement.to_string(),
        }],
    }
}

fn revision(source: &Path) -> u64 {
    let verified = verify_path(source, VerifyOptions::default()).unwrap();
    document_revision(&verified.input_sha256).unwrap()
}

fn text_sha256(text: &str) -> String {
    format!("{:X}", Sha256::digest(text.as_bytes()))
}

fn file_sha256(path: &Path) -> String {
    format!("{:X}", Sha256::digest(std::fs::read(path).unwrap()))
}

fn decoded_body(path: &Path) -> Vec<u8> {
    let bytes = fs::read(path).unwrap();
    let mut cursor = std::io::Cursor::new(&bytes);
    let layout = read_layout(&mut cursor, "fixture.HLT").unwrap();
    assert_eq!(layout.stream_start, HLT_HEADER_LEN);
    let mut decoder =
        DeflateDecoder::new(&bytes[layout.stream_start as usize..layout.stream_end as usize]);
    let mut body = Vec::new();
    decoder.read_to_end(&mut body).unwrap();
    body
}

fn paragraph_line_texts(document: &InspectDocument, body: &[u8], id: &str) -> Vec<String> {
    let paragraph = document
        .paragraphs
        .iter()
        .find(|paragraph| paragraph.id == id)
        .unwrap();
    paragraph
        .line_ids
        .iter()
        .map(|line_id| {
            let line = document
                .lines
                .iter()
                .find(|line| &line.id == line_id)
                .unwrap();
            let raw =
                &body[line.source_range.start_byte as usize..line.source_range.end_byte as usize];
            EUC_KR
                .decode_without_bom_handling_and_without_replacement(raw)
                .unwrap()
                .into_owned()
        })
        .collect()
}

fn line_index(document: &InspectDocument, id: &str) -> usize {
    document
        .lines
        .iter()
        .find(|line| line.id == id)
        .unwrap()
        .index as usize
}

fn span_codes_through(body: &[u8], last_line: usize) -> Vec<Vec<u32>> {
    const CHAR_MARK: &[u8] = b"~`!@#$%^&*()-+|=-TeCfMt\r\n";
    let marker = body
        .windows(CHAR_MARK.len())
        .position(|window| window == CHAR_MARK)
        .unwrap();
    let mut cursor = marker + CHAR_MARK.len();
    let mut spans = Vec::with_capacity(last_line + 1);
    for _ in 0..=last_line {
        let count = u32::from_le_bytes(body[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;
        let mut codes = Vec::with_capacity(count);
        for _ in 0..count {
            codes.push(u32::from_le_bytes(
                body[cursor..cursor + 4].try_into().unwrap(),
            ));
            cursor += 4;
        }
        spans.push(codes);
    }
    spans
}

fn assert_cp949_pairs_share_codes(raw: &[u8], codes: &[u32]) {
    let mut position = 0;
    while position < raw.len() {
        let width = if raw[position] >= 0x81 { 2 } else { 1 };
        if width == 2 {
            assert_eq!(codes[position], codes[position + 1]);
        }
        position += width;
    }
}
