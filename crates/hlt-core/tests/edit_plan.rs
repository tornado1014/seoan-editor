mod support;

use hlt_core::{
    EditDecisionStatus, EditSkipReason, ExactTextEditRequest, ParagraphTextEdit, VerifyOptions,
    document_revision, plan_exact_text_edits, verify_path,
};
use sha2::{Digest, Sha256};

#[test]
fn plans_one_same_length_cp949_edit_by_revision_id_and_text_hash() {
    let source = support::pct_table_fixture();
    let request = request(
        source.path(),
        vec![operation(
            "paragraph:3",
            "구성품 시험 결과",
            "구성품 분석 결과",
        )],
    );

    let plan = plan_exact_text_edits(source.path(), &request).unwrap();

    assert_eq!(plan.operation_count, 1);
    assert_eq!(plan.ready, 1);
    assert_eq!(plan.skipped, 0);
    assert_eq!(plan.base_revision, request.expected_revision);
    assert_eq!(plan.decisions[0].paragraph_id, "paragraph:3");
    assert_eq!(plan.decisions[0].status, EditDecisionStatus::Ready);
    assert_eq!(plan.decisions[0].reason, None);
    assert_eq!(plan.decisions[0].source_bytes, Some(16));
    assert_eq!(plan.decisions[0].replacement_bytes, Some(16));
    let serialized = serde_json::to_string(&plan).unwrap();
    assert!(!serialized.contains("구성품"));
    assert!(!serialized.contains("연구"));
    assert!(!serialized.contains("분석"));
    assert!(!serialized.contains("start_byte"));
}

#[test]
fn revision_mismatch_fails_the_whole_request() {
    let source = support::pct_table_fixture();
    let mut request = request(
        source.path(),
        vec![operation(
            "paragraph:3",
            "구성품 시험 결과",
            "구성품 분석 결과",
        )],
    );
    request.expected_revision ^= 1;

    let error = plan_exact_text_edits(source.path(), &request).unwrap_err();

    assert_eq!(error.code.to_string(), "revision_conflict");
    assert_eq!(error.phase, "edit_revision");
}

#[test]
fn skips_wrong_hash_missing_duplicate_cp949_length_and_multiline_operations() {
    let source = support::pct_table_fixture();
    let request = request(
        source.path(),
        vec![
            ParagraphTextEdit {
                paragraph_id: "paragraph:3".to_string(),
                expected_text_sha256: text_sha256("다른 원문"),
                replacement: "구성품 분석 결과".to_string(),
            },
            operation("paragraph:missing", "구성품 시험 결과", "구성품 분석 결과"),
            operation("paragraph:3", "구성품 시험 결과", "구성품 분석 결과"),
            operation("paragraph:2", "【표 1】", "😀😀😀😀😀"),
            operation("paragraph:0", "【발명의 설명】", "【발명의 상세한 설명】"),
            operation(
                "paragraph:4",
                "unused because multiline is checked first",
                "unused replacement",
            ),
        ],
    );

    let plan = plan_exact_text_edits(source.path(), &request).unwrap();
    let reasons: Vec<_> = plan
        .decisions
        .iter()
        .map(|decision| decision.reason)
        .collect();

    assert_eq!(plan.ready, 0);
    assert_eq!(plan.skipped, 6);
    assert_eq!(
        reasons,
        [
            Some(EditSkipReason::SourceMismatch),
            Some(EditSkipReason::ParagraphNotFound),
            Some(EditSkipReason::DuplicateParagraphId),
            Some(EditSkipReason::Cp949Unrepresentable),
            Some(EditSkipReason::ByteLengthChanged),
            Some(EditSkipReason::MultilineUnsupported),
        ]
    );
}

#[test]
fn rejects_more_than_500_operations_or_four_mib_of_replacement_text() {
    let source = support::pct_table_fixture();
    let one = operation("paragraph:3", "구성품 시험 결과", "구성품 분석 결과");
    let too_many = ExactTextEditRequest {
        expected_revision: 0,
        operations: vec![one; 501],
    };
    let too_large = ExactTextEditRequest {
        expected_revision: 0,
        operations: vec![ParagraphTextEdit {
            paragraph_id: "paragraph:3".to_string(),
            expected_text_sha256: text_sha256("구성품 시험 결과"),
            replacement: "A".repeat(4 * 1024 * 1024 + 1),
        }],
    };

    for request in [too_many, too_large] {
        let error = plan_exact_text_edits(source.path(), &request).unwrap_err();
        assert_eq!(error.code.to_string(), "resource_too_large");
        assert_eq!(error.phase, "edit_operations");
    }
}

fn request(source: &std::path::Path, operations: Vec<ParagraphTextEdit>) -> ExactTextEditRequest {
    let verified = verify_path(source, VerifyOptions::default()).unwrap();
    ExactTextEditRequest {
        expected_revision: document_revision(&verified.input_sha256).unwrap(),
        operations,
    }
}

fn operation(paragraph_id: &str, source: &str, replacement: &str) -> ParagraphTextEdit {
    ParagraphTextEdit {
        paragraph_id: paragraph_id.to_string(),
        expected_text_sha256: text_sha256(source),
        replacement: replacement.to_string(),
    }
}

fn text_sha256(text: &str) -> String {
    format!("{:X}", Sha256::digest(text.as_bytes()))
}
