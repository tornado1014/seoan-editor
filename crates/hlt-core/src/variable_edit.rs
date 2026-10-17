use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use encoding_rs::EUC_KR;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    CellRecord, CoreError, EditDecision, EditDecisionStatus, EditPlan, EditSkipReason, ErrorCode,
    ExactTextEditRequest, ImageOccurrence, InspectDocument, LineRecord, ParagraphRecord,
    ParagraphTextEdit, TableRecord, VerifyResult, WriteOptions, WriteSummary,
    carry_character_codes, document_revision,
    edit::validate_request_bounds,
    error::{io_error, safe_source},
    inspect::{CHAR_MARK, EditCharSpan, inspect_edit_layout},
    restore_automatic_line_breaks,
    save::{
        InflatedPatch, InflatedSource, open_inflated_source, publish_inflated_as_new,
        rewrite_inflated_spool,
    },
};

pub type VariableTextEditRequest = ExactTextEditRequest;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariableEditStructureResult {
    pub header_equal: bool,
    pub inflated_delta_equal: bool,
    pub counts_equal: bool,
    pub fixed_blocks_equal: bool,
    pub target_paragraphs_equal: bool,
    pub non_target_semantics_equal: bool,
    pub tables_equal: bool,
    pub images_equal: bool,
    pub wipo_equal: bool,
    pub char_spans_valid: bool,
    pub body_changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariableEditSaveResult {
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
    #[serde(with = "crate::serde_u64::i64_as_string")]
    pub expected_inflated_delta: i64,
    pub writer: WriteSummary,
    pub verification: VerifyResult,
    pub edit_plan: EditPlan,
    pub structure: VariableEditStructureResult,
}

struct VariableEditTransaction {
    plan: EditPlan,
    patches: Vec<InflatedPatch>,
    replacements: BTreeMap<String, String>,
    expected_inflated_delta: i64,
}

pub fn plan_variable_text_edits(
    source: &Path,
    request: &VariableTextEditRequest,
) -> Result<EditPlan, CoreError> {
    let mut source = open_inflated_source(source, &Default::default())?;
    Ok(build_variable_transaction(&mut source, request)?.plan)
}

pub fn save_variable_text_as_new(
    source: &Path,
    output: &Path,
    request: &VariableTextEditRequest,
    options: WriteOptions,
) -> Result<VariableEditSaveResult, CoreError> {
    let mut source = open_inflated_source(source, &options.verify)?;
    let mut transaction = build_variable_transaction(&mut source, request)?;
    if transaction.patches.is_empty() {
        return Err(no_ready_error(&source.source_name, &transaction.plan));
    }
    let rewrite = rewrite_inflated_spool(&mut source, &mut transaction.patches)?;
    if rewrite.input_bytes != source.document.summary.inflated_bytes
        || rewrite.delta_bytes != transaction.expected_inflated_delta
    {
        return Err(variable_structure_error(
            &source.source_name,
            "edit_rewrite_delta",
        ));
    }
    let rewritten_layout = inspect_edit_layout(&mut source.spool, rewrite.output_bytes)?;
    if rewritten_layout.total_lines != source.document.summary.line_count
        || rewritten_layout.char_spans.len() as u64 != source.document.summary.span_count
    {
        return Err(variable_structure_error(
            &source.source_name,
            "char_span_invariant",
        ));
    }
    let rewritten_body_sha256 = sha256_file(&mut source.spool, &source.source_name)?;

    let mut accepted_structure = None;
    let published = publish_inflated_as_new(
        &mut source,
        output,
        &options.verify,
        |input, output_document, output_body_sha256| {
            let structure = compare_variable_structure(
                input,
                output_document,
                output_body_sha256,
                &rewritten_body_sha256,
                transaction.expected_inflated_delta,
                &transaction.replacements,
            );
            if variable_structure_passes(&structure) {
                accepted_structure = Some(structure);
                Ok(())
            } else {
                Err(variable_parity_error(&input.source_name, &structure))
            }
        },
    )?;
    let structure = accepted_structure.ok_or_else(|| {
        CoreError::new(ErrorCode::VerificationFailed, "variable_edit_parity", false)
    })?;
    let next_revision = document_revision(&published.output_sha256)?;

    Ok(VariableEditSaveResult {
        input_source: source.source_name,
        output_source: published.output_source,
        input_file_size: source.verification.file_size,
        output_file_size: published.output_file_size,
        base_revision: transaction.plan.base_revision,
        next_revision,
        input_sha256: source.verification.input_sha256,
        output_sha256: published.output_sha256,
        expected_inflated_delta: transaction.expected_inflated_delta,
        writer: published.writer,
        verification: published.verification,
        edit_plan: transaction.plan,
        structure,
    })
}

fn build_variable_transaction(
    source: &mut InflatedSource,
    request: &VariableTextEditRequest,
) -> Result<VariableEditTransaction, CoreError> {
    validate_request_bounds(&source.source_path, request)?;
    let base_revision = document_revision(&source.verification.input_sha256)?;
    if request.expected_revision != base_revision {
        let mut error = CoreError::new(ErrorCode::RevisionConflict, "edit_revision", false);
        error.source = safe_source(&source.source_name);
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
    let section_anchors: BTreeSet<&str> = source
        .document
        .sections
        .iter()
        .filter_map(|section| section.anchor_paragraph_id.as_deref())
        .collect();
    let layout = inspect_edit_layout(&mut source.spool, source.document.summary.inflated_bytes)?;
    if !(layout.body_start < layout.body_end
        && layout.char_base == layout.body_end + CHAR_MARK.len() as u64
        && layout.char_base < layout.line_base
        && layout.total_lines == source.document.summary.line_count
        && layout.char_spans.len() as u64 == source.document.summary.span_count)
    {
        return Err(variable_structure_error(&source.source_name, "edit_layout"));
    }

    let mut decisions = Vec::with_capacity(request.operations.len());
    let mut patches = Vec::with_capacity(request.operations.len() * 2);
    let mut replacements = BTreeMap::new();
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
        let source_text_sha256 = text_sha256(&paragraph.text);
        if paragraph.text_truncated {
            decisions.push(skipped(
                operation,
                EditSkipReason::PreviewTruncated,
                Some(source_text_sha256),
                replacement_sha256,
                None,
                None,
            ));
            continue;
        }
        if paragraph.line_ids.is_empty() {
            decisions.push(skipped(
                operation,
                EditSkipReason::CharSpanInvalid,
                Some(source_text_sha256),
                replacement_sha256,
                None,
                None,
            ));
            continue;
        }
        if section_anchors.contains(operation.paragraph_id.as_str()) {
            decisions.push(skipped(
                operation,
                EditSkipReason::SectionAnchorUnsupported,
                Some(source_text_sha256),
                replacement_sha256,
                None,
                None,
            ));
            continue;
        }
        let paragraph_lines: Vec<&LineRecord> = paragraph
            .line_ids
            .iter()
            .map(|id| {
                lines
                    .get(id.as_str())
                    .ok_or_else(|| variable_structure_error(&source.source_name, "edit_line"))
            })
            .collect::<Result<_, _>>()?;
        let raw_lines: Vec<Vec<u8>> = paragraph_lines
            .iter()
            .map(|line| {
                read_range(
                    &mut source.spool,
                    line.source_range.start_byte,
                    line.source_range.end_byte - line.source_range.start_byte,
                    &source.source_name,
                )
            })
            .collect::<Result<_, _>>()?;
        let raw_with_breaks = join_raw_lines(&raw_lines);
        let source_bytes = Some(raw_with_breaks.len() as u64);
        if raw_lines.iter().any(|raw| contains_structure_controls(raw)) {
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
        let Some(decoded_with_breaks) =
            EUC_KR.decode_without_bom_handling_and_without_replacement(&raw_with_breaks)
        else {
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
        let decoded_plain = decoded_with_breaks.replace("\r\n", "");
        if decoded_plain != paragraph.text || operation.expected_text_sha256 != source_text_sha256 {
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
        if operation.replacement.chars().any(|value| {
            matches!(
                value,
                '\r' | '\n' | '\0' | '\u{0015}' | '\u{0012}' | '\u{0013}'
            )
        }) {
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
        let replacement_with_breaks = if paragraph_lines.len() == 1 {
            operation.replacement.clone()
        } else {
            restore_automatic_line_breaks(&decoded_with_breaks, &operation.replacement)?
        };
        let replacement_lines: Vec<&str> = replacement_with_breaks.split("\r\n").collect();
        if replacement_lines.len() != paragraph_lines.len() {
            decisions.push(skipped(
                operation,
                EditSkipReason::AutomaticBreakMismatch,
                Some(source_text_sha256),
                replacement_sha256,
                source_bytes,
                None,
            ));
            continue;
        }
        let (replacement, _, had_errors) = EUC_KR.encode(&replacement_with_breaks);
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
        let replacement = replacement.into_owned();
        let replacement_bytes = Some(replacement.len() as u64);
        if marker_count(&raw_with_breaks) != marker_count(&replacement) {
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
        if raw_with_breaks == replacement {
            decisions.push(skipped(
                operation,
                EditSkipReason::NoChange,
                Some(source_text_sha256),
                replacement_sha256,
                source_bytes,
                replacement_bytes,
            ));
            continue;
        }
        let mut spans = Vec::with_capacity(paragraph_lines.len());
        let mut old_codes = Vec::with_capacity(decoded_plain.chars().count());
        for (index, (line, raw)) in paragraph_lines.iter().zip(&raw_lines).enumerate() {
            let Some(span) = layout.char_spans.get(line.index as usize) else {
                decisions.push(skipped(
                    operation,
                    EditSkipReason::CharSpanInvalid,
                    Some(source_text_sha256.clone()),
                    replacement_sha256.clone(),
                    source_bytes,
                    replacement_bytes,
                ));
                spans.clear();
                break;
            };
            old_codes.extend(character_codes(
                raw,
                span,
                index + 1 == paragraph_lines.len(),
            )?);
            spans.push(span);
        }
        if spans.len() != paragraph_lines.len() {
            continue;
        }
        let old_characters: Vec<char> = decoded_plain.chars().collect();
        let Some(plain_code) = modal_plain_code(&old_characters, &old_codes) else {
            decisions.push(skipped(
                operation,
                EditSkipReason::PlainCodeUnavailable,
                Some(source_text_sha256),
                replacement_sha256,
                source_bytes,
                replacement_bytes,
            ));
            continue;
        };
        let carried = carry_character_codes(
            &decoded_plain,
            &old_codes,
            &operation.replacement,
            plain_code,
        )?;
        let body_start = paragraph_lines
            .first()
            .expect("non-empty paragraph lines")
            .source_range
            .start_byte;
        let body_end = paragraph_lines
            .last()
            .expect("non-empty paragraph lines")
            .source_range
            .end_byte;
        patches.push(InflatedPatch {
            start_byte: body_start,
            end_byte: body_end,
            replacement,
        });
        let mut carried_start = 0;
        for (index, (line_text, span)) in replacement_lines.iter().zip(spans).enumerate() {
            let carried_end = carried_start + line_text.chars().count();
            let span_replacement = encode_expanded_span(
                line_text,
                &carried[carried_start..carried_end],
                index + 1 == paragraph_lines.len(),
            )?;
            patches.push(InflatedPatch {
                start_byte: span.source_range.start_byte,
                end_byte: span.source_range.end_byte,
                replacement: span_replacement,
            });
            carried_start = carried_end;
        }
        if carried_start != carried.len() {
            return Err(variable_structure_error(&source.source_name, "line_breaks"));
        }
        replacements.insert(
            operation.paragraph_id.clone(),
            operation.replacement.clone(),
        );
        decisions.push(EditDecision {
            paragraph_id: operation.paragraph_id.clone(),
            status: EditDecisionStatus::Ready,
            reason: None,
            source_text_sha256: Some(source_text_sha256),
            replacement_sha256,
            source_bytes,
            replacement_bytes,
        });
    }

    let expected_inflated_delta = patch_delta(&patches)?;
    let ready = decisions
        .iter()
        .filter(|decision| decision.status == EditDecisionStatus::Ready)
        .count() as u16;
    Ok(VariableEditTransaction {
        plan: EditPlan {
            source: source.source_name.clone(),
            base_revision,
            operation_count: request.operations.len() as u16,
            ready,
            skipped: request.operations.len() as u16 - ready,
            decisions,
        },
        patches,
        replacements,
        expected_inflated_delta,
    })
}

fn character_codes(raw: &[u8], span: &EditCharSpan, is_last: bool) -> Result<Vec<u32>, CoreError> {
    let Some(text) = EUC_KR.decode_without_bom_handling_and_without_replacement(raw) else {
        return Err(CoreError::new(
            ErrorCode::Cp949Invalid,
            "char_span_invariant",
            false,
        ));
    };
    if span.codes.len() == 1 {
        return Ok(vec![span.codes[0]; text.chars().count()]);
    }
    let expected = raw.len() + usize::from(is_last);
    if span.codes.len() != expected || (is_last && span.codes.last() != Some(&0)) {
        return Err(CoreError::new(
            ErrorCode::StructureInvalid,
            "char_span_invariant",
            false,
        ));
    }
    let mut output = Vec::with_capacity(text.chars().count());
    let mut position = 0_usize;
    while position < raw.len() {
        let width = if raw[position] >= 0x81 { 2 } else { 1 };
        if position + width > raw.len()
            || (width == 2 && span.codes[position] != span.codes[position + 1])
        {
            return Err(CoreError::new(
                ErrorCode::StructureInvalid,
                "char_span_invariant",
                false,
            ));
        }
        output.push(span.codes[position]);
        position += width;
    }
    Ok(output)
}

fn modal_plain_code(characters: &[char], codes: &[u32]) -> Option<u32> {
    let mut counts: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
    for (index, (character, code)) in characters.iter().zip(codes).enumerate() {
        if matches!(*character, '_' | '\u{2063}') {
            continue;
        }
        let entry = counts.entry(*code).or_insert((0, index));
        entry.0 += 1;
    }
    counts
        .into_iter()
        .max_by(|left, right| {
            left.1
                .0
                .cmp(&right.1.0)
                .then_with(|| right.1.1.cmp(&left.1.1))
        })
        .map(|(code, _)| code)
}

fn encode_expanded_span(
    text: &str,
    character_codes: &[u32],
    terminate: bool,
) -> Result<Vec<u8>, CoreError> {
    if text.chars().count() != character_codes.len() {
        return Err(CoreError::new(
            ErrorCode::ConversionFailed,
            "carry_codes",
            false,
        ));
    }
    let mut codes = Vec::new();
    for (character, code) in text.chars().zip(character_codes) {
        let value = character.to_string();
        let (encoded, _, had_errors) = EUC_KR.encode(&value);
        if had_errors || encoded.is_empty() || encoded.len() > 2 {
            return Err(CoreError::new(
                ErrorCode::Cp949Invalid,
                "char_span_encode",
                false,
            ));
        }
        codes.extend(std::iter::repeat_n(*code, encoded.len()));
    }
    if terminate {
        codes.push(0);
    }
    let count = u32::try_from(codes.len())
        .map_err(|_| CoreError::new(ErrorCode::ResourceTooLarge, "char_span_encode", false))?;
    let mut output = Vec::with_capacity(4 + 4 * codes.len());
    output.extend_from_slice(&count.to_le_bytes());
    for code in codes {
        output.extend_from_slice(&code.to_le_bytes());
    }
    Ok(output)
}

fn join_raw_lines(lines: &[Vec<u8>]) -> Vec<u8> {
    let content_bytes = lines.iter().map(Vec::len).sum::<usize>();
    let mut output = Vec::with_capacity(
        content_bytes.saturating_add(lines.len().saturating_sub(1).saturating_mul(2)),
    );
    for (index, line) in lines.iter().enumerate() {
        if index != 0 {
            output.extend_from_slice(b"\r\n");
        }
        output.extend_from_slice(line);
    }
    output
}

fn patch_delta(patches: &[InflatedPatch]) -> Result<i64, CoreError> {
    let mut delta = 0_i128;
    for patch in patches {
        delta += patch.replacement.len() as i128
            - i128::from(patch.end_byte.saturating_sub(patch.start_byte));
    }
    i64::try_from(delta)
        .map_err(|_| CoreError::new(ErrorCode::ResourceTooLarge, "edit_patch_layout", false))
}

fn compare_variable_structure(
    source: &InflatedSource,
    output: &InspectDocument,
    output_body_sha256: &str,
    rewritten_body_sha256: &str,
    expected_delta: i64,
    replacements: &BTreeMap<String, String>,
) -> VariableEditStructureResult {
    let input_summary = &source.document.summary;
    let output_summary = &output.summary;
    let expected_inflated = i128::from(input_summary.inflated_bytes) + i128::from(expected_delta);
    let counts_equal = source.document.sections == output.sections
        && input_summary.section_count == output_summary.section_count
        && input_summary.paragraph_count == output_summary.paragraph_count
        && input_summary.line_count == output_summary.line_count
        && input_summary.span_count == output_summary.span_count
        && input_summary.table_count == output_summary.table_count
        && input_summary.cell_count == output_summary.cell_count
        && input_summary.image_count == output_summary.image_count
        && line_semantics_equal(&source.document.lines, &output.lines);
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
    let target_paragraphs_equal = replacements.iter().all(|(id, replacement)| {
        input_paragraphs
            .get(id.as_str())
            .zip(output_paragraphs.get(id.as_str()))
            .is_some_and(|(input, output)| {
                paragraph_semantics_equal(input, output, replacement.as_str())
            })
    });
    let non_target_semantics_equal = input_paragraphs.len() == output_paragraphs.len()
        && input_paragraphs.iter().all(|(id, input)| {
            replacements.contains_key(*id)
                || output_paragraphs
                    .get(id)
                    .is_some_and(|output| paragraph_semantics_equal(input, output, &input.text))
        });

    VariableEditStructureResult {
        header_equal: true,
        inflated_delta_equal: expected_inflated == i128::from(output_summary.inflated_bytes),
        counts_equal,
        fixed_blocks_equal: input_summary.fixed_blocks == output_summary.fixed_blocks,
        target_paragraphs_equal,
        non_target_semantics_equal,
        tables_equal: table_semantics_equal(
            &source.document.tables,
            &source.document.cells,
            &output.tables,
            &output.cells,
        ),
        images_equal: image_semantics_equal(&source.document.images, &output.images),
        wipo_equal: input_summary.paragraph_wipoid_nonzero
            == output_summary.paragraph_wipoid_nonzero
            && input_summary.paragraph_wipoid_distinct == output_summary.paragraph_wipoid_distinct,
        char_spans_valid: output_body_sha256 == rewritten_body_sha256,
        body_changed: source.body_sha256 != output_body_sha256,
    }
}

fn paragraph_semantics_equal(
    input: &ParagraphRecord,
    output: &ParagraphRecord,
    expected_text: &str,
) -> bool {
    input.id == output.id
        && input.part == output.part
        && input.section_id == output.section_id
        && output.text == expected_text
        && input.text_truncated == output.text_truncated
        && input.shape_id == output.shape_id
        && input.indent_raw == output.indent_raw
        && input.lspace == output.lspace
        && input.align == output.align
        && input.line_ids == output.line_ids
        && input.wipo_id == output.wipo_id
}

fn line_semantics_equal(input: &[LineRecord], output: &[LineRecord]) -> bool {
    input.len() == output.len()
        && input.iter().zip(output).all(|(input, output)| {
            input.id == output.id
                && input.paragraph_id == output.paragraph_id
                && input.index == output.index
        })
}

fn table_semantics_equal(
    input_tables: &[TableRecord],
    input_cells: &[CellRecord],
    output_tables: &[TableRecord],
    output_cells: &[CellRecord],
) -> bool {
    input_tables.len() == output_tables.len()
        && input_tables
            .iter()
            .zip(output_tables)
            .all(|(input, output)| {
                input.id == output.id
                    && input.occurrence == output.occurrence
                    && input.anchor_section_id == output.anchor_section_id
                    && input.anchor_paragraph_id == output.anchor_paragraph_id
                    && input.row_count == output.row_count
                    && input.column_count == output.column_count
                    && input.cell_ids == output.cell_ids
            })
        && input_cells.len() == output_cells.len()
        && input_cells
            .iter()
            .zip(output_cells)
            .all(|(input, output)| cell_semantics_equal(input, output))
}

fn cell_semantics_equal(input: &CellRecord, output: &CellRecord) -> bool {
    input.id == output.id
        && input.table_id == output.table_id
        && input.row == output.row
        && input.occurrence == output.occurrence
        && input.start == output.start
        && input.width == output.width
        && input.rowspan == output.rowspan
        && input.colspan == output.colspan
        && input.line_ids == output.line_ids
        && input.image_ids == output.image_ids
}

fn image_semantics_equal(input: &[ImageOccurrence], output: &[ImageOccurrence]) -> bool {
    input.len() == output.len()
        && input.iter().zip(output).all(|(input, output)| {
            input.id == output.id
                && input.occurrence == output.occurrence
                && input.object_code == output.object_code
                && input.payload_sha256 == output.payload_sha256
                && input.paragraph_id == output.paragraph_id
                && input.line_id == output.line_id
                && input.table_id == output.table_id
                && input.cell_id == output.cell_id
        })
}

fn variable_structure_passes(structure: &VariableEditStructureResult) -> bool {
    structure.header_equal
        && structure.inflated_delta_equal
        && structure.counts_equal
        && structure.fixed_blocks_equal
        && structure.target_paragraphs_equal
        && structure.non_target_semantics_equal
        && structure.tables_equal
        && structure.images_equal
        && structure.wipo_equal
        && structure.char_spans_valid
        && structure.body_changed
}

fn variable_parity_error(source: &str, structure: &VariableEditStructureResult) -> CoreError {
    let mut error = CoreError::new(ErrorCode::VerificationFailed, "variable_edit_parity", false);
    error.source = safe_source(source);
    for (name, value) in [
        ("header_equal", structure.header_equal),
        ("inflated_delta_equal", structure.inflated_delta_equal),
        ("counts_equal", structure.counts_equal),
        ("fixed_blocks_equal", structure.fixed_blocks_equal),
        ("target_paragraphs_equal", structure.target_paragraphs_equal),
        (
            "non_target_semantics_equal",
            structure.non_target_semantics_equal,
        ),
        ("tables_equal", structure.tables_equal),
        ("images_equal", structure.images_equal),
        ("wipo_equal", structure.wipo_equal),
        ("char_spans_valid", structure.char_spans_valid),
        ("body_changed", structure.body_changed),
    ] {
        error.details.insert(name.to_string(), value.to_string());
    }
    error
}

fn no_ready_error(source: &str, plan: &EditPlan) -> CoreError {
    let mut error = CoreError::new(ErrorCode::ConversionFailed, "variable_edit_apply", false);
    error.source = safe_source(source);
    error
        .details
        .insert("ready".to_string(), plan.ready.to_string());
    error
        .details
        .insert("skipped".to_string(), plan.skipped.to_string());
    error
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

fn read_range(
    file: &mut File,
    start: u64,
    length: u64,
    source: &str,
) -> Result<Vec<u8>, CoreError> {
    let length = usize::try_from(length)
        .map_err(|_| CoreError::new(ErrorCode::ResourceTooLarge, "variable_edit_line", false))?;
    file.seek(SeekFrom::Start(start))
        .map_err(|error| io_error(&error, "variable_edit_line", source))?;
    let mut output = vec![0_u8; length];
    file.read_exact(&mut output)
        .map_err(|error| io_error(&error, "variable_edit_line", source))?;
    Ok(output)
}

fn sha256_file(file: &mut File, source: &str) -> Result<String, CoreError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| io_error(&error, "variable_edit_hash", source))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| io_error(&error, "variable_edit_hash", source))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|error| io_error(&error, "variable_edit_hash", source))?;
    Ok(format!("{:X}", hasher.finalize()))
}

fn marker_count(bytes: &[u8]) -> usize {
    bytes.iter().filter(|value| **value == b'_').count()
}

fn contains_structure_controls(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .any(|value| matches!(*value, b'\r' | b'\n' | 0 | 0x12 | 0x13 | 0x15))
}

fn text_sha256(text: &str) -> String {
    format!("{:X}", Sha256::digest(text.as_bytes()))
}

fn variable_structure_error(source: &str, phase: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::StructureInvalid, phase, false);
    error.source = safe_source(source);
    error
}
