use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

use encoding_rs::EUC_KR;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    CoreError, ErrorCode, InspectDocument, LineRecord, ParagraphRecord, VerifyOptions,
    VerifyResult, WriteOptions, WriteSummary,
    error::{io_error, safe_source},
    save::{InflatedSource, open_inflated_source, publish_inflated_as_new},
};

const MAX_OPERATIONS: usize = 500;
const MAX_REPLACEMENT_UTF8_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParagraphTextEdit {
    pub paragraph_id: String,
    pub expected_text_sha256: String,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactTextEditRequest {
    #[serde(with = "crate::serde_u64::as_string")]
    pub expected_revision: u64,
    pub operations: Vec<ParagraphTextEdit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditDecisionStatus {
    Ready,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditSkipReason {
    ParagraphNotFound,
    DuplicateParagraphId,
    PreviewTruncated,
    MultilineUnsupported,
    HiddenControlsUnsupported,
    SourceMismatch,
    Cp949Unrepresentable,
    ByteLengthChanged,
    ImageMarkerChanged,
    NoChange,
    PlainCodeUnavailable,
    CharSpanInvalid,
    SectionAnchorUnsupported,
    AutomaticBreakMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditDecision {
    pub paragraph_id: String,
    pub status: EditDecisionStatus,
    pub reason: Option<EditSkipReason>,
    pub source_text_sha256: Option<String>,
    pub replacement_sha256: String,
    #[serde(with = "crate::serde_u64::option")]
    pub source_bytes: Option<u64>,
    #[serde(with = "crate::serde_u64::option")]
    pub replacement_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditPlan {
    pub source: String,
    #[serde(with = "crate::serde_u64::as_string")]
    pub base_revision: u64,
    pub operation_count: u16,
    pub ready: u16,
    pub skipped: u16,
    pub decisions: Vec<EditDecision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditStructureResult {
    pub header_equal: bool,
    pub inflated_size_equal: bool,
    pub summary_equal: bool,
    pub fixed_blocks_equal: bool,
    pub target_paragraphs_equal: bool,
    pub non_target_paragraphs_equal: bool,
    pub body_changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditSaveResult {
    pub input_source: String,
    pub output_source: String,
    #[serde(with = "crate::serde_u64::as_string")]
    pub input_file_size: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub output_file_size: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub base_revision: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub next_revision: u64,
    pub input_sha256: String,
    pub output_sha256: String,
    pub writer: WriteSummary,
    pub verification: VerifyResult,
    pub edit_plan: EditPlan,
    pub structure: EditStructureResult,
}

struct ExactPatch {
    paragraph_id: String,
    start_byte: u64,
    replacement_bytes: Vec<u8>,
    replacement_text: String,
}

struct ExactEditTransaction {
    plan: EditPlan,
    patches: Vec<ExactPatch>,
}

pub fn document_revision(sha256: &str) -> Result<u64, CoreError> {
    if sha256.len() != 64 || !sha256.bytes().all(|value| value.is_ascii_hexdigit()) {
        return Err(CoreError::new(
            ErrorCode::ConversionFailed,
            "edit_revision",
            false,
        ));
    }
    u64::from_str_radix(&sha256[..16], 16)
        .map_err(|_| CoreError::new(ErrorCode::ConversionFailed, "edit_revision", false))
}

pub fn plan_exact_text_edits(
    source: &Path,
    request: &ExactTextEditRequest,
) -> Result<EditPlan, CoreError> {
    let mut source = open_inflated_source(source, &VerifyOptions::default())?;
    Ok(build_exact_edit_transaction(&mut source, request)?.plan)
}

pub fn save_exact_text_as_new(
    source: &Path,
    output: &Path,
    request: &ExactTextEditRequest,
    options: WriteOptions,
) -> Result<EditSaveResult, CoreError> {
    let mut source = open_inflated_source(source, &options.verify)?;
    let transaction = build_exact_edit_transaction(&mut source, request)?;
    if transaction.patches.is_empty() {
        let mut error = CoreError::new(ErrorCode::ConversionFailed, "edit_apply", false);
        error.source = safe_source(&source.source_name);
        error
            .details
            .insert("ready".to_string(), transaction.plan.ready.to_string());
        error
            .details
            .insert("skipped".to_string(), transaction.plan.skipped.to_string());
        return Err(error);
    }

    for patch in &transaction.patches {
        source
            .spool
            .seek(SeekFrom::Start(patch.start_byte))
            .map_err(|error| io_error(&error, "edit_apply", &source.source_name))?;
        source
            .spool
            .write_all(&patch.replacement_bytes)
            .map_err(|error| io_error(&error, "edit_apply", &source.source_name))?;
    }
    source
        .spool
        .flush()
        .and_then(|_| source.spool.sync_all())
        .map_err(|error| io_error(&error, "edit_apply", &source.source_name))?;

    let mut accepted_structure = None;
    let published = publish_inflated_as_new(
        &mut source,
        output,
        &options.verify,
        |input, output_document, output_body_sha256| {
            let structure = compare_edit_structure(
                input,
                output_document,
                output_body_sha256,
                &transaction.patches,
            );
            if structure.header_equal
                && structure.inflated_size_equal
                && structure.summary_equal
                && structure.fixed_blocks_equal
                && structure.target_paragraphs_equal
                && structure.non_target_paragraphs_equal
                && structure.body_changed
            {
                accepted_structure = Some(structure);
                Ok(())
            } else {
                Err(edit_parity_error(&input.source_name, &structure))
            }
        },
    )?;
    let structure = accepted_structure
        .ok_or_else(|| CoreError::new(ErrorCode::VerificationFailed, "edit_parity", false))?;
    let next_revision = document_revision(&published.output_sha256)?;

    Ok(EditSaveResult {
        input_source: source.source_name,
        output_source: published.output_source,
        input_file_size: source.verification.file_size,
        output_file_size: published.output_file_size,
        base_revision: transaction.plan.base_revision,
        next_revision,
        input_sha256: source.verification.input_sha256,
        output_sha256: published.output_sha256,
        writer: published.writer,
        verification: published.verification,
        edit_plan: transaction.plan,
        structure,
    })
}

fn build_exact_edit_transaction(
    source: &mut InflatedSource,
    request: &ExactTextEditRequest,
) -> Result<ExactEditTransaction, CoreError> {
    validate_request_bounds(&source.source_path, request)?;
    let source_name = source.source_name.clone();
    let base_revision = document_revision(&source.verification.input_sha256)?;
    if request.expected_revision != base_revision {
        let mut error = CoreError::new(ErrorCode::RevisionConflict, "edit_revision", false);
        error.source = safe_source(&source_name);
        error.expected = Some(base_revision.to_string());
        error.actual = Some(request.expected_revision.to_string());
        return Err(error);
    }

    let paragraphs: BTreeMap<String, ParagraphRecord> = source
        .document
        .paragraphs
        .iter()
        .map(|paragraph| (paragraph.id.clone(), paragraph.clone()))
        .collect();
    let lines: BTreeMap<String, LineRecord> = source
        .document
        .lines
        .iter()
        .map(|line| (line.id.clone(), line.clone()))
        .collect();

    let mut decisions = Vec::with_capacity(request.operations.len());
    let mut patches = Vec::with_capacity(request.operations.len());
    let mut seen = BTreeSet::new();
    for operation in &request.operations {
        let replacement_sha256 = text_sha256(&operation.replacement);
        if !seen.insert(operation.paragraph_id.as_str()) {
            decisions.push(skipped(
                operation,
                EditSkipReason::DuplicateParagraphId,
                None,
                replacement_sha256,
                None,
                None,
            ));
            continue;
        }
        let Some(paragraph) = paragraphs.get(operation.paragraph_id.as_str()) else {
            decisions.push(skipped(
                operation,
                EditSkipReason::ParagraphNotFound,
                None,
                replacement_sha256,
                None,
                None,
            ));
            continue;
        };
        if paragraph.text_truncated {
            decisions.push(skipped(
                operation,
                EditSkipReason::PreviewTruncated,
                Some(text_sha256(&paragraph.text)),
                replacement_sha256,
                None,
                None,
            ));
            continue;
        }
        if paragraph.line_ids.len() != 1 {
            decisions.push(skipped(
                operation,
                EditSkipReason::MultilineUnsupported,
                Some(text_sha256(&paragraph.text)),
                replacement_sha256,
                None,
                None,
            ));
            continue;
        }
        let line = lines
            .get(paragraph.line_ids[0].as_str())
            .ok_or_else(|| structure_error(&source_name, "edit_line"))?;
        let raw = read_range(
            &mut source.spool,
            line.source_range.start_byte,
            line.source_range.end_byte - line.source_range.start_byte,
            &source_name,
        )?;
        let source_bytes = Some(raw.len() as u64);
        if raw
            .iter()
            .any(|value| matches!(*value, b'\r' | b'\n' | 0 | 0x15))
        {
            decisions.push(skipped(
                operation,
                EditSkipReason::HiddenControlsUnsupported,
                None,
                replacement_sha256,
                source_bytes,
                None,
            ));
            continue;
        }
        let Some(decoded) = EUC_KR.decode_without_bom_handling_and_without_replacement(&raw) else {
            decisions.push(skipped(
                operation,
                EditSkipReason::SourceMismatch,
                None,
                replacement_sha256,
                source_bytes,
                None,
            ));
            continue;
        };
        let source_text_sha256 = text_sha256(&paragraph.text);
        if decoded.as_ref() != paragraph.text
            || operation.expected_text_sha256 != source_text_sha256
        {
            decisions.push(skipped(
                operation,
                EditSkipReason::SourceMismatch,
                Some(source_text_sha256),
                replacement_sha256,
                source_bytes,
                None,
            ));
            continue;
        }
        if operation
            .replacement
            .bytes()
            .any(|value| matches!(value, b'\r' | b'\n' | 0 | 0x15))
        {
            decisions.push(skipped(
                operation,
                EditSkipReason::HiddenControlsUnsupported,
                Some(source_text_sha256),
                replacement_sha256,
                source_bytes,
                None,
            ));
            continue;
        }
        let (replacement, _, had_errors) = EUC_KR.encode(&operation.replacement);
        if had_errors {
            decisions.push(skipped(
                operation,
                EditSkipReason::Cp949Unrepresentable,
                Some(source_text_sha256),
                replacement_sha256,
                source_bytes,
                None,
            ));
            continue;
        }
        let replacement_bytes = Some(replacement.len() as u64);
        if replacement.len() != raw.len() {
            decisions.push(skipped(
                operation,
                EditSkipReason::ByteLengthChanged,
                Some(source_text_sha256),
                replacement_sha256,
                source_bytes,
                replacement_bytes,
            ));
            continue;
        }
        if marker_positions(&raw) != marker_positions(&replacement) {
            decisions.push(skipped(
                operation,
                EditSkipReason::ImageMarkerChanged,
                Some(source_text_sha256),
                replacement_sha256,
                source_bytes,
                replacement_bytes,
            ));
            continue;
        }
        decisions.push(EditDecision {
            paragraph_id: operation.paragraph_id.clone(),
            status: EditDecisionStatus::Ready,
            reason: None,
            source_text_sha256: Some(source_text_sha256),
            replacement_sha256,
            source_bytes,
            replacement_bytes,
        });
        patches.push(ExactPatch {
            paragraph_id: operation.paragraph_id.clone(),
            start_byte: line.source_range.start_byte,
            replacement_bytes: replacement.into_owned(),
            replacement_text: operation.replacement.clone(),
        });
    }

    let ready = decisions
        .iter()
        .filter(|decision| decision.status == EditDecisionStatus::Ready)
        .count() as u16;
    Ok(ExactEditTransaction {
        plan: EditPlan {
            source: source_name,
            base_revision,
            operation_count: request.operations.len() as u16,
            ready,
            skipped: request.operations.len() as u16 - ready,
            decisions,
        },
        patches,
    })
}

fn compare_edit_structure(
    source: &InflatedSource,
    output: &InspectDocument,
    output_body_sha256: &str,
    patches: &[ExactPatch],
) -> EditStructureResult {
    let targets: BTreeMap<&str, &str> = patches
        .iter()
        .map(|patch| (patch.paragraph_id.as_str(), patch.replacement_text.as_str()))
        .collect();
    let input_paragraphs: BTreeMap<&str, &ParagraphRecord> = source
        .document
        .paragraphs
        .iter()
        .map(|paragraph| (paragraph.id.as_str(), paragraph))
        .collect();
    let output_paragraphs: BTreeMap<&str, &ParagraphRecord> = output
        .paragraphs
        .iter()
        .map(|paragraph| (paragraph.id.as_str(), paragraph))
        .collect();
    let target_paragraphs_equal = targets.iter().all(|(id, replacement)| {
        let (Some(input), Some(output)) = (input_paragraphs.get(id), output_paragraphs.get(id))
        else {
            return false;
        };
        let mut expected = (*input).clone();
        expected.text = (*replacement).to_string();
        output == &&expected
    });
    let non_target_paragraphs_equal = input_paragraphs.len() == output_paragraphs.len()
        && input_paragraphs.iter().all(|(id, input)| {
            targets.contains_key(id)
                || output_paragraphs
                    .get(id)
                    .is_some_and(|output| output == input)
        });

    EditStructureResult {
        header_equal: true,
        inflated_size_equal: source.document.summary.inflated_bytes
            == output.summary.inflated_bytes,
        summary_equal: source.document.summary == output.summary,
        fixed_blocks_equal: source.document.summary.fixed_blocks == output.summary.fixed_blocks,
        target_paragraphs_equal,
        non_target_paragraphs_equal,
        body_changed: source.body_sha256 != output_body_sha256,
    }
}

fn edit_parity_error(source: &str, structure: &EditStructureResult) -> CoreError {
    let mut error = CoreError::new(ErrorCode::VerificationFailed, "edit_parity", false);
    error.source = safe_source(source);
    for (name, value) in [
        ("header_equal", structure.header_equal),
        ("inflated_size_equal", structure.inflated_size_equal),
        ("summary_equal", structure.summary_equal),
        ("fixed_blocks_equal", structure.fixed_blocks_equal),
        ("target_paragraphs_equal", structure.target_paragraphs_equal),
        (
            "non_target_paragraphs_equal",
            structure.non_target_paragraphs_equal,
        ),
        ("body_changed", structure.body_changed),
    ] {
        error.details.insert(name.to_string(), value.to_string());
    }
    error
}

pub(crate) fn validate_request_bounds(
    source: &Path,
    request: &ExactTextEditRequest,
) -> Result<(), CoreError> {
    let replacement_bytes = request
        .operations
        .iter()
        .try_fold(0_usize, |total, operation| {
            total.checked_add(operation.replacement.len())
        });
    if request.operations.len() > MAX_OPERATIONS
        || replacement_bytes.is_none_or(|value| value > MAX_REPLACEMENT_UTF8_BYTES)
    {
        let source_name = source
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("input.HLT");
        let mut error = CoreError::new(ErrorCode::ResourceTooLarge, "edit_operations", false);
        error.source = safe_source(source_name);
        error.expected = Some(format!(
            "operations<={MAX_OPERATIONS},replacement_utf8_bytes<={MAX_REPLACEMENT_UTF8_BYTES}"
        ));
        return Err(error);
    }
    Ok(())
}

fn read_range(
    file: &mut File,
    start: u64,
    length: u64,
    source: &str,
) -> Result<Vec<u8>, CoreError> {
    let length = usize::try_from(length)
        .map_err(|_| CoreError::new(ErrorCode::ResourceTooLarge, "edit_line", false))?;
    file.seek(SeekFrom::Start(start))
        .map_err(|error| io_error(&error, "edit_line", source))?;
    let mut output = vec![0_u8; length];
    file.read_exact(&mut output)
        .map_err(|error| io_error(&error, "edit_line", source))?;
    Ok(output)
}

fn skipped(
    operation: &ParagraphTextEdit,
    reason: EditSkipReason,
    source_text_sha256: Option<String>,
    replacement_sha256: String,
    source_bytes: Option<u64>,
    replacement_bytes: Option<u64>,
) -> EditDecision {
    EditDecision {
        paragraph_id: operation.paragraph_id.clone(),
        status: EditDecisionStatus::Skipped,
        reason: Some(reason),
        source_text_sha256,
        replacement_sha256,
        source_bytes,
        replacement_bytes,
    }
}

fn marker_positions(bytes: &[u8]) -> Vec<usize> {
    bytes
        .iter()
        .enumerate()
        .filter_map(|(index, value)| (*value == b'_').then_some(index))
        .collect()
}

fn text_sha256(text: &str) -> String {
    format!("{:X}", Sha256::digest(text.as_bytes()))
}

fn structure_error(source: &str, phase: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::StructureInvalid, phase, false);
    error.source = safe_source(source);
    error
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_marker_positions_not_just_counts_must_match() {
        assert_eq!(marker_positions(b"a_b_c"), vec![1, 3]);
        assert_ne!(marker_positions(b"a_b_c"), marker_positions(b"_ab_c"));
    }
}
