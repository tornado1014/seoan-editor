use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    CellRecord, CharacterFormat, CoreError, ErrorCode, FormatParagraph, ImageOccurrence,
    InspectDocument, LineRecord, ParagraphRecord, TableRecord, VerifyOptions, VerifyResult,
    WriteOptions, WriteSummary, document_revision,
    error::{io_error, safe_source},
    format_inspect::{RuntimeCharacter, RuntimeFormatState, inspect_format_state_from_source},
    inspect::inspect_edit_layout,
    save::{InflatedPatch, open_inflated_source, publish_inflated_as_new, rewrite_inflated_spool},
};

const MAX_OPERATIONS: usize = 500;
const MAX_RUNS: usize = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParagraphAlign {
    Left,
    Center,
    Justified,
}

impl ParagraphAlign {
    fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
            Self::Justified => "justified",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunStyleEdit {
    pub start: u32,
    pub end: u32,
    pub formats: Vec<CharacterFormat>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParagraphFormatEdit {
    pub paragraph_id: String,
    pub expected_text_sha256: String,
    pub align: Option<ParagraphAlign>,
    pub runs: Vec<RunStyleEdit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormatEditRequest {
    #[serde(with = "crate::serde_u64::as_string")]
    pub expected_revision: u64,
    pub operations: Vec<ParagraphFormatEdit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatEditDecisionStatus {
    Ready,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatEditSkipReason {
    ParagraphNotFound,
    DuplicateParagraphId,
    PreviewTruncated,
    SourceMismatch,
    EmptyRun,
    RunOutOfRange,
    OverlappingRuns,
    DuplicateFormat,
    UnverifiedSourceFormat,
    ImageMarkerIntersection,
    StyleUnavailable,
    ShapeUnavailable,
    TableParagraphRequiresCellId,
    NoChange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatEditDecision {
    pub paragraph_id: String,
    pub status: FormatEditDecisionStatus,
    pub reason: Option<FormatEditSkipReason>,
    pub source_text_sha256: Option<String>,
    pub align: Option<ParagraphAlign>,
    pub run_count: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatEditPlan {
    pub source: String,
    #[serde(with = "crate::serde_u64::as_string")]
    pub base_revision: u64,
    pub operation_count: u16,
    pub total_runs: u32,
    pub ready: u16,
    pub skipped: u16,
    pub decisions: Vec<FormatEditDecision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatEditStructureResult {
    pub header_equal: bool,
    pub body_size_delta_equal: bool,
    pub counts_equal: bool,
    pub fixed_blocks_equal: bool,
    pub target_formats_equal: bool,
    pub non_target_formats_equal: bool,
    pub paragraph_text_equal: bool,
    pub paragraph_shapes_equal: bool,
    pub tables_equal: bool,
    pub images_equal: bool,
    pub wipo_equal: bool,
    pub char_spans_valid: bool,
    pub body_changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatEditSaveResult {
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
    pub edit_plan: FormatEditPlan,
    pub structure: FormatEditStructureResult,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedFormatRun {
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) code: u32,
    pub(crate) formats: Vec<CharacterFormat>,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedParagraphFormat {
    pub(crate) paragraph_id: String,
    pub(crate) runs: Vec<PlannedFormatRun>,
    pub(crate) shape_index: Option<u32>,
}

pub(crate) struct FormatEditTransaction {
    pub(crate) plan: FormatEditPlan,
    pub(crate) operations: Vec<PlannedParagraphFormat>,
    pub(crate) source_formats: RuntimeFormatState,
}

pub fn plan_format_edits(
    path: &Path,
    request: &FormatEditRequest,
) -> Result<FormatEditPlan, CoreError> {
    let mut source = open_inflated_source(path, &VerifyOptions::default())?;
    Ok(build_format_edit_transaction(&mut source, request)?.plan)
}

pub fn save_format_edits_as_new(
    source_path: &Path,
    output: &Path,
    request: &FormatEditRequest,
    options: WriteOptions,
) -> Result<FormatEditSaveResult, CoreError> {
    let mut source = open_inflated_source(source_path, &options.verify)?;
    let transaction = build_format_edit_transaction(&mut source, request)?;
    if transaction.operations.is_empty() {
        return Err(no_ready_error(&source.source_name, &transaction.plan));
    }
    let (_, input_line_shapes) = read_line_shapes(
        &mut source.spool,
        source.document.summary.inflated_bytes,
        &source.source_name,
    )?;
    let mut patches = build_character_patches(&mut source, &transaction.operations)?;
    if patches.is_empty() {
        return Err(no_ready_error(&source.source_name, &transaction.plan));
    }
    let expected_inflated_delta = patch_delta(&patches)?;
    let rewrite = rewrite_inflated_spool(&mut source, &mut patches)?;
    if rewrite.input_bytes != source.document.summary.inflated_bytes
        || rewrite.delta_bytes != expected_inflated_delta
    {
        return Err(format_structure_error(
            &source.source_name,
            "format_edit_rewrite_delta",
        ));
    }
    let (rewritten_layout, output_line_shapes) =
        read_line_shapes(&mut source.spool, rewrite.output_bytes, &source.source_name)?;
    if rewritten_layout.total_lines != source.document.summary.line_count
        || rewritten_layout.char_spans.len() as u64 != source.document.summary.span_count
    {
        return Err(format_structure_error(
            &source.source_name,
            "format_char_span_invariant",
        ));
    }
    let line_shapes_equal = expected_line_shapes_equal(
        &source.document,
        &transaction.operations,
        &input_line_shapes,
        &output_line_shapes,
    );
    let output_formats = inspect_format_state_from_source(&mut source)?;
    let rewritten_body_sha256 = sha256_file(&mut source.spool, &source.source_name)?;

    let mut accepted_structure = None;
    let published = publish_inflated_as_new(
        &mut source,
        output,
        &options.verify,
        |input, output_document, output_body_sha256| {
            let structure = compare_format_structure(
                input,
                output_document,
                output_body_sha256,
                &rewritten_body_sha256,
                expected_inflated_delta,
                &transaction.source_formats,
                &output_formats,
                &transaction.operations,
                line_shapes_equal,
            );
            if format_structure_passes(&structure) {
                accepted_structure = Some(structure);
                Ok(())
            } else {
                Err(format_parity_error(&input.source_name, &structure))
            }
        },
    )?;
    let structure = accepted_structure.ok_or_else(|| {
        CoreError::new(ErrorCode::VerificationFailed, "format_edit_parity", false)
    })?;
    let next_revision = document_revision(&published.output_sha256)?;

    Ok(FormatEditSaveResult {
        input_source: source.source_name,
        output_source: published.output_source,
        input_file_size: source.verification.file_size,
        output_file_size: published.output_file_size,
        base_revision: transaction.plan.base_revision,
        next_revision,
        input_sha256: source.verification.input_sha256,
        output_sha256: published.output_sha256,
        expected_inflated_delta,
        writer: published.writer,
        verification: published.verification,
        edit_plan: transaction.plan,
        structure,
    })
}

pub(crate) fn build_format_edit_transaction(
    source: &mut crate::save::InflatedSource,
    request: &FormatEditRequest,
) -> Result<FormatEditTransaction, CoreError> {
    let total_runs = validate_request_bounds(&source.source_name, request)?;
    let base_revision = document_revision(&source.verification.input_sha256)?;
    if request.expected_revision != base_revision {
        let mut error = CoreError::new(ErrorCode::RevisionConflict, "format_edit_revision", false);
        error.source = safe_source(&source.source_name);
        error.expected = Some(base_revision.to_string());
        error.actual = Some(request.expected_revision.to_string());
        return Err(error);
    }

    let state = inspect_format_state_from_source(source)?;
    let format_paragraphs: BTreeMap<&str, &FormatParagraph> = state
        .inventory
        .paragraphs
        .iter()
        .map(|paragraph| (paragraph.paragraph_id.as_str(), paragraph))
        .collect();
    let inspect_paragraphs: BTreeMap<&str, &ParagraphRecord> = source
        .document
        .paragraphs
        .iter()
        .map(|paragraph| (paragraph.id.as_str(), paragraph))
        .collect();
    let unresolved_paragraphs: BTreeSet<&str> = state
        .inventory
        .unresolved
        .iter()
        .map(|item| item.paragraph_id.as_str())
        .collect();
    let lines_by_id: BTreeMap<&str, &LineRecord> = source
        .document
        .lines
        .iter()
        .map(|line| (line.id.as_str(), line))
        .collect();
    let table_paragraphs: BTreeSet<&str> = source
        .document
        .cells
        .iter()
        .flat_map(|cell| &cell.line_ids)
        .filter_map(|line_id| lines_by_id.get(line_id.as_str()))
        .map(|line| line.paragraph_id.as_str())
        .collect();

    let mut seen = BTreeSet::new();
    let mut decisions = Vec::with_capacity(request.operations.len());
    let mut planned = Vec::new();
    for operation in &request.operations {
        if !seen.insert(operation.paragraph_id.as_str()) {
            decisions.push(skipped(
                operation,
                FormatEditSkipReason::DuplicateParagraphId,
                None,
            ));
            continue;
        }
        let (Some(paragraph), Some(inspected), Some(characters)) = (
            format_paragraphs.get(operation.paragraph_id.as_str()),
            inspect_paragraphs.get(operation.paragraph_id.as_str()),
            state.characters.get(operation.paragraph_id.as_str()),
        ) else {
            decisions.push(skipped(
                operation,
                FormatEditSkipReason::ParagraphNotFound,
                None,
            ));
            continue;
        };
        let source_text_sha256 = text_sha256(&paragraph.text);
        if inspected.text_truncated {
            decisions.push(skipped(
                operation,
                FormatEditSkipReason::PreviewTruncated,
                Some(source_text_sha256),
            ));
            continue;
        }
        if operation.expected_text_sha256 != source_text_sha256 {
            decisions.push(skipped(
                operation,
                FormatEditSkipReason::SourceMismatch,
                Some(source_text_sha256),
            ));
            continue;
        }
        if table_paragraphs.contains(operation.paragraph_id.as_str()) {
            decisions.push(skipped(
                operation,
                FormatEditSkipReason::TableParagraphRequiresCellId,
                Some(source_text_sha256),
            ));
            continue;
        }
        if unresolved_paragraphs.contains(operation.paragraph_id.as_str())
            || characters.iter().any(|character| !character.verified)
        {
            decisions.push(skipped(
                operation,
                FormatEditSkipReason::UnverifiedSourceFormat,
                Some(source_text_sha256),
            ));
            continue;
        }

        let mut ranges = operation.runs.clone();
        if let Some(reason) = validate_runs(&mut ranges, characters.len()) {
            decisions.push(skipped(operation, reason, Some(source_text_sha256)));
            continue;
        }
        let plain_code = modal_plain_code(characters);
        let mut planned_runs = Vec::with_capacity(ranges.len());
        let mut run_error = None;
        for run in ranges {
            let mut formats = run.formats;
            formats.sort_unstable();
            let mask = format_mask(&formats);
            if characters[run.start as usize..run.end as usize]
                .iter()
                .any(|character| character.image)
            {
                run_error = Some(FormatEditSkipReason::ImageMarkerIntersection);
                break;
            }
            let code = if mask == 0 {
                plain_code.or_else(|| state.style_codes.get(&0).copied())
            } else {
                state.style_codes.get(&mask).copied()
            };
            let Some(code) = code else {
                run_error = Some(FormatEditSkipReason::StyleUnavailable);
                break;
            };
            planned_runs.push(PlannedFormatRun {
                start: run.start,
                end: run.end,
                code,
                formats,
            });
        }
        if let Some(reason) = run_error {
            decisions.push(skipped(operation, reason, Some(source_text_sha256)));
            continue;
        }

        let shape_index = if let Some(align) = operation.align {
            if inspected.align == align.as_str() {
                None
            } else {
                let shape = source.document.shapes.iter().find(|shape| {
                    shape.align == align.as_str()
                        && shape.indent_raw == inspected.indent_raw
                        && shape.lspace == inspected.lspace
                });
                let Some(shape) = shape else {
                    decisions.push(skipped(
                        operation,
                        FormatEditSkipReason::ShapeUnavailable,
                        Some(source_text_sha256),
                    ));
                    continue;
                };
                Some(shape.index)
            }
        } else {
            None
        };

        let formats_change = planned_runs.iter().any(|run| {
            characters[run.start as usize..run.end as usize]
                .iter()
                .any(|character| character.code != Some(run.code))
        });
        if !formats_change && shape_index.is_none() {
            decisions.push(skipped(
                operation,
                FormatEditSkipReason::NoChange,
                Some(source_text_sha256),
            ));
            continue;
        }
        decisions.push(FormatEditDecision {
            paragraph_id: operation.paragraph_id.clone(),
            status: FormatEditDecisionStatus::Ready,
            reason: None,
            source_text_sha256: Some(source_text_sha256),
            align: operation.align,
            run_count: operation.runs.len() as u16,
        });
        planned.push(PlannedParagraphFormat {
            paragraph_id: operation.paragraph_id.clone(),
            runs: planned_runs,
            shape_index,
        });
    }

    let ready = decisions
        .iter()
        .filter(|decision| decision.status == FormatEditDecisionStatus::Ready)
        .count() as u16;
    Ok(FormatEditTransaction {
        plan: FormatEditPlan {
            source: source.source_name.clone(),
            base_revision,
            operation_count: request.operations.len() as u16,
            total_runs: total_runs as u32,
            ready,
            skipped: request.operations.len() as u16 - ready,
            decisions,
        },
        operations: planned,
        source_formats: state,
    })
}

fn build_character_patches(
    source: &mut crate::save::InflatedSource,
    operations: &[PlannedParagraphFormat],
) -> Result<Vec<InflatedPatch>, CoreError> {
    let layout = inspect_edit_layout(&mut source.spool, source.document.summary.inflated_bytes)?;
    let paragraphs: BTreeMap<String, ParagraphRecord> = source
        .document
        .paragraphs
        .iter()
        .cloned()
        .map(|paragraph| (paragraph.id.clone(), paragraph))
        .collect();
    let lines: BTreeMap<String, LineRecord> = source
        .document
        .lines
        .iter()
        .cloned()
        .map(|line| (line.id.clone(), line))
        .collect();
    let mut patches = Vec::new();
    for operation in operations {
        let paragraph = paragraphs
            .get(operation.paragraph_id.as_str())
            .ok_or_else(|| format_structure_error(&source.source_name, "format_edit_paragraph"))?;
        let mut character_index = 0_u32;
        let mut run_index = 0_usize;
        for (paragraph_line, line_id) in paragraph.line_ids.iter().enumerate() {
            let line = lines
                .get(line_id.as_str())
                .ok_or_else(|| format_structure_error(&source.source_name, "format_edit_line"))?;
            let span = layout.char_spans.get(line.index as usize).ok_or_else(|| {
                format_structure_error(&source.source_name, "format_edit_char_span")
            })?;
            let raw = read_range(
                &mut source.spool,
                line.source_range.start_byte,
                line.source_range.end_byte - line.source_range.start_byte,
                &source.source_name,
            )?;
            let mut codes = if span.codes.len() == 1 {
                vec![span.codes[0]; raw.len()]
            } else {
                if span.codes.len() < raw.len() {
                    return Err(format_structure_error(
                        &source.source_name,
                        "format_edit_char_span",
                    ));
                }
                span.codes[..raw.len()].to_vec()
            };
            let mut byte_index = 0_usize;
            let mut line_changed = false;
            while byte_index < raw.len() {
                let width = if raw[byte_index] >= 0x81 { 2 } else { 1 };
                if byte_index + width > raw.len() {
                    return Err(format_structure_error(
                        &source.source_name,
                        "format_edit_cp949",
                    ));
                }
                if raw[byte_index] >= 0x20 {
                    while run_index < operation.runs.len()
                        && character_index >= operation.runs[run_index].end
                    {
                        run_index += 1;
                    }
                    if let Some(run) = operation.runs.get(run_index)
                        && character_index >= run.start
                        && character_index < run.end
                    {
                        for code in &mut codes[byte_index..byte_index + width] {
                            if *code != run.code {
                                *code = run.code;
                                line_changed = true;
                            }
                        }
                    }
                    character_index = character_index.checked_add(1).ok_or_else(|| {
                        format_structure_error(&source.source_name, "format_edit_character_index")
                    })?;
                }
                byte_index += width;
            }
            if line_changed {
                let terminate = paragraph_line + 1 == paragraph.line_ids.len();
                patches.push(InflatedPatch {
                    start_byte: span.source_range.start_byte,
                    end_byte: span.source_range.end_byte,
                    replacement: encode_expanded_codes(&codes, terminate)?,
                });
            }
        }
        if operation
            .runs
            .last()
            .is_some_and(|run| character_index < run.end)
        {
            return Err(format_structure_error(
                &source.source_name,
                "format_edit_character_index",
            ));
        }
        if let Some(shape_index) = operation.shape_index {
            for line_id in &paragraph.line_ids {
                let line = lines.get(line_id.as_str()).ok_or_else(|| {
                    format_structure_error(&source.source_name, "format_edit_line")
                })?;
                let start_byte = layout.line_base + 12 * line.index;
                let current = read_u32_at(&mut source.spool, start_byte, &source.source_name)?;
                if current != shape_index {
                    patches.push(InflatedPatch {
                        start_byte,
                        end_byte: start_byte + 4,
                        replacement: shape_index.to_le_bytes().to_vec(),
                    });
                }
            }
        }
    }
    Ok(patches)
}

fn encode_expanded_codes(codes: &[u32], terminate: bool) -> Result<Vec<u8>, CoreError> {
    let count = codes
        .len()
        .checked_add(usize::from(terminate))
        .ok_or_else(|| {
            CoreError::new(ErrorCode::ResourceTooLarge, "format_edit_char_span", false)
        })?;
    let count = u32::try_from(count)
        .map_err(|_| CoreError::new(ErrorCode::ResourceTooLarge, "format_edit_char_span", false))?;
    let mut output = Vec::with_capacity(4 + 4 * count as usize);
    output.extend_from_slice(&count.to_le_bytes());
    for code in codes {
        output.extend_from_slice(&code.to_le_bytes());
    }
    if terminate {
        output.extend_from_slice(&0_u32.to_le_bytes());
    }
    Ok(output)
}

fn patch_delta(patches: &[InflatedPatch]) -> Result<i64, CoreError> {
    let mut delta = 0_i128;
    for patch in patches {
        delta += patch.replacement.len() as i128
            - i128::from(patch.end_byte.saturating_sub(patch.start_byte));
    }
    i64::try_from(delta).map_err(|_| {
        CoreError::new(
            ErrorCode::ResourceTooLarge,
            "format_edit_patch_layout",
            false,
        )
    })
}

#[allow(clippy::too_many_arguments)]
fn compare_format_structure(
    source: &crate::save::InflatedSource,
    output_document: &InspectDocument,
    output_body_sha256: &str,
    rewritten_body_sha256: &str,
    expected_delta: i64,
    source_formats: &RuntimeFormatState,
    output_formats: &RuntimeFormatState,
    operations: &[PlannedParagraphFormat],
    line_shapes_equal: bool,
) -> FormatEditStructureResult {
    let input_summary = &source.document.summary;
    let output_summary = &output_document.summary;
    let expected_inflated = i128::from(input_summary.inflated_bytes) + i128::from(expected_delta);
    let counts_equal = source.document.sections == output_document.sections
        && input_summary.section_count == output_summary.section_count
        && input_summary.paragraph_count == output_summary.paragraph_count
        && input_summary.line_count == output_summary.line_count
        && input_summary.span_count == output_summary.span_count
        && input_summary.table_count == output_summary.table_count
        && input_summary.cell_count == output_summary.cell_count
        && input_summary.image_count == output_summary.image_count
        && line_semantics_equal(&source.document.lines, &output_document.lines);
    let paragraph_text_equal = format_text_equal(source_formats, output_formats)
        && paragraph_text_semantics_equal(&source.document, output_document);
    let paragraph_shapes_equal = line_shapes_equal
        && paragraph_shape_semantics_equal(&source.document, output_document, operations);
    let (target_formats_equal, non_target_formats_equal) =
        format_semantics_equal(source_formats, output_formats, operations);

    FormatEditStructureResult {
        header_equal: true,
        body_size_delta_equal: expected_inflated == i128::from(output_summary.inflated_bytes),
        counts_equal,
        fixed_blocks_equal: input_summary.fixed_blocks == output_summary.fixed_blocks,
        target_formats_equal,
        non_target_formats_equal,
        paragraph_text_equal,
        paragraph_shapes_equal,
        tables_equal: table_semantics_equal(
            &source.document.tables,
            &source.document.cells,
            &output_document.tables,
            &output_document.cells,
        ),
        images_equal: image_semantics_equal(&source.document.images, &output_document.images),
        wipo_equal: input_summary.paragraph_wipoid_nonzero
            == output_summary.paragraph_wipoid_nonzero
            && input_summary.paragraph_wipoid_distinct == output_summary.paragraph_wipoid_distinct,
        char_spans_valid: output_body_sha256 == rewritten_body_sha256,
        body_changed: source.body_sha256 != output_body_sha256,
    }
}

fn format_semantics_equal(
    source: &RuntimeFormatState,
    output: &RuntimeFormatState,
    operations: &[PlannedParagraphFormat],
) -> (bool, bool) {
    if source.characters.len() != output.characters.len()
        || !source.inventory.unresolved.is_empty()
        || !output.inventory.unresolved.is_empty()
    {
        return (false, false);
    }
    let targets: BTreeMap<&str, &[PlannedFormatRun]> = operations
        .iter()
        .map(|operation| (operation.paragraph_id.as_str(), operation.runs.as_slice()))
        .collect();
    let mut target_equal = true;
    let mut non_target_equal = true;
    for (paragraph_id, source_characters) in &source.characters {
        let Some(output_characters) = output.characters.get(paragraph_id) else {
            return (false, false);
        };
        if source_characters.len() != output_characters.len() {
            return (false, false);
        }
        let runs = targets.get(paragraph_id.as_str()).copied().unwrap_or(&[]);
        for (index, (source_character, output_character)) in
            source_characters.iter().zip(output_characters).enumerate()
        {
            let target = runs
                .iter()
                .find(|run| run.start as usize <= index && index < run.end as usize);
            if let Some(run) = target {
                target_equal &= output_character.verified
                    && !output_character.image
                    && output_character.code == Some(run.code)
                    && output_character.attr == Some(format_mask(&run.formats));
            } else {
                non_target_equal &= source_character == output_character;
            }
        }
    }
    (target_equal, non_target_equal)
}

fn format_text_equal(source: &RuntimeFormatState, output: &RuntimeFormatState) -> bool {
    source.inventory.paragraphs.len() == output.inventory.paragraphs.len()
        && source
            .inventory
            .paragraphs
            .iter()
            .zip(&output.inventory.paragraphs)
            .all(|(source, output)| {
                source.paragraph_id == output.paragraph_id && source.text == output.text
            })
}

fn paragraph_text_semantics_equal(input: &InspectDocument, output: &InspectDocument) -> bool {
    input.paragraphs.len() == output.paragraphs.len()
        && input
            .paragraphs
            .iter()
            .zip(&output.paragraphs)
            .all(|(input, output)| {
                input.id == output.id
                    && input.part == output.part
                    && input.section_id == output.section_id
                    && input.text == output.text
                    && input.text_truncated == output.text_truncated
                    && input.line_ids == output.line_ids
                    && input.wipo_id == output.wipo_id
            })
}

fn paragraph_shape_semantics_equal(
    input_document: &InspectDocument,
    output_document: &InspectDocument,
    operations: &[PlannedParagraphFormat],
) -> bool {
    let target_shapes: BTreeMap<&str, u32> = operations
        .iter()
        .filter_map(|operation| {
            operation
                .shape_index
                .map(|shape| (operation.paragraph_id.as_str(), shape))
        })
        .collect();
    input_document.paragraphs.len() == output_document.paragraphs.len()
        && input_document
            .paragraphs
            .iter()
            .zip(&output_document.paragraphs)
            .all(|(input_paragraph, output_paragraph)| {
                if input_paragraph.id != output_paragraph.id {
                    return false;
                }
                let Some(shape_index) = target_shapes.get(input_paragraph.id.as_str()) else {
                    return input_paragraph.shape_id == output_paragraph.shape_id
                        && input_paragraph.indent_raw == output_paragraph.indent_raw
                        && input_paragraph.lspace == output_paragraph.lspace
                        && input_paragraph.align == output_paragraph.align;
                };
                let Some(expected) = input_document
                    .shapes
                    .iter()
                    .find(|shape| shape.index == *shape_index)
                else {
                    return false;
                };
                output_paragraph.shape_id == Some(format!("shape:{shape_index}"))
                    && output_paragraph.indent_raw == expected.indent_raw
                    && output_paragraph.lspace == expected.lspace
                    && output_paragraph.align == expected.align
            })
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
        && input_cells.iter().zip(output_cells).all(|(input, output)| {
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
        })
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

fn format_structure_passes(structure: &FormatEditStructureResult) -> bool {
    structure.header_equal
        && structure.body_size_delta_equal
        && structure.counts_equal
        && structure.fixed_blocks_equal
        && structure.target_formats_equal
        && structure.non_target_formats_equal
        && structure.paragraph_text_equal
        && structure.paragraph_shapes_equal
        && structure.tables_equal
        && structure.images_equal
        && structure.wipo_equal
        && structure.char_spans_valid
        && structure.body_changed
}

fn format_parity_error(source: &str, structure: &FormatEditStructureResult) -> CoreError {
    let mut error = CoreError::new(ErrorCode::VerificationFailed, "format_edit_parity", false);
    error.source = safe_source(source);
    for (name, value) in [
        ("header_equal", structure.header_equal),
        ("body_size_delta_equal", structure.body_size_delta_equal),
        ("counts_equal", structure.counts_equal),
        ("fixed_blocks_equal", structure.fixed_blocks_equal),
        ("target_formats_equal", structure.target_formats_equal),
        (
            "non_target_formats_equal",
            structure.non_target_formats_equal,
        ),
        ("paragraph_text_equal", structure.paragraph_text_equal),
        ("paragraph_shapes_equal", structure.paragraph_shapes_equal),
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

fn no_ready_error(source: &str, plan: &FormatEditPlan) -> CoreError {
    let mut error = CoreError::new(ErrorCode::ConversionFailed, "format_edit_apply", false);
    error.source = safe_source(source);
    error
        .details
        .insert("ready".to_string(), plan.ready.to_string());
    error
        .details
        .insert("skipped".to_string(), plan.skipped.to_string());
    error
}

fn read_line_shapes(
    file: &mut File,
    inflated: u64,
    source: &str,
) -> Result<(crate::inspect::EditLayout, Vec<u32>), CoreError> {
    let layout = inspect_edit_layout(file, inflated)?;
    let mut shapes = Vec::with_capacity(layout.total_lines as usize);
    for line in 0..layout.total_lines {
        shapes.push(read_u32_at(file, layout.line_base + 12 * line, source)?);
    }
    Ok((layout, shapes))
}

fn expected_line_shapes_equal(
    document: &InspectDocument,
    operations: &[PlannedParagraphFormat],
    input: &[u32],
    output: &[u32],
) -> bool {
    if input.len() != output.len() || input.len() != document.lines.len() {
        return false;
    }
    let paragraphs: BTreeMap<&str, &ParagraphRecord> = document
        .paragraphs
        .iter()
        .map(|paragraph| (paragraph.id.as_str(), paragraph))
        .collect();
    let lines: BTreeMap<&str, &LineRecord> = document
        .lines
        .iter()
        .map(|line| (line.id.as_str(), line))
        .collect();
    let mut expected = input.to_vec();
    for operation in operations {
        let Some(shape_index) = operation.shape_index else {
            continue;
        };
        let Some(paragraph) = paragraphs.get(operation.paragraph_id.as_str()) else {
            return false;
        };
        for line_id in &paragraph.line_ids {
            let Some(line) = lines.get(line_id.as_str()) else {
                return false;
            };
            let Some(slot) = expected.get_mut(line.index as usize) else {
                return false;
            };
            *slot = shape_index;
        }
    }
    expected == output
}

fn read_u32_at(file: &mut File, offset: u64, source: &str) -> Result<u32, CoreError> {
    Ok(u32::from_le_bytes(
        read_range(file, offset, 4, source)?
            .try_into()
            .expect("four-byte read"),
    ))
}

fn read_range(
    file: &mut File,
    start: u64,
    length: u64,
    source: &str,
) -> Result<Vec<u8>, CoreError> {
    let length = usize::try_from(length)
        .map_err(|_| CoreError::new(ErrorCode::ResourceTooLarge, "format_edit_line", false))?;
    file.seek(SeekFrom::Start(start))
        .map_err(|error| io_error(&error, "format_edit_line", source))?;
    let mut output = vec![0_u8; length];
    file.read_exact(&mut output)
        .map_err(|error| io_error(&error, "format_edit_line", source))?;
    Ok(output)
}

fn sha256_file(file: &mut File, source: &str) -> Result<String, CoreError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| io_error(&error, "format_edit_hash", source))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| io_error(&error, "format_edit_hash", source))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|error| io_error(&error, "format_edit_hash", source))?;
    Ok(format!("{:X}", hasher.finalize()))
}

fn format_structure_error(source: &str, phase: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::StructureInvalid, phase, false);
    error.source = safe_source(source);
    error
}

fn validate_request_bounds(source: &str, request: &FormatEditRequest) -> Result<usize, CoreError> {
    let total_runs = request
        .operations
        .iter()
        .try_fold(0_usize, |sum, operation| {
            sum.checked_add(operation.runs.len())
        });
    if request.operations.len() > MAX_OPERATIONS || total_runs.is_none_or(|count| count > MAX_RUNS)
    {
        let mut error =
            CoreError::new(ErrorCode::ResourceTooLarge, "format_edit_operations", false);
        error.source = safe_source(source);
        return Err(error);
    }
    Ok(total_runs.expect("validated run count"))
}

fn validate_runs(
    runs: &mut [RunStyleEdit],
    character_count: usize,
) -> Option<FormatEditSkipReason> {
    for run in runs.iter() {
        if run.start >= run.end {
            return Some(FormatEditSkipReason::EmptyRun);
        }
        if run.end as usize > character_count {
            return Some(FormatEditSkipReason::RunOutOfRange);
        }
        let unique: BTreeSet<_> = run.formats.iter().copied().collect();
        if unique.len() != run.formats.len() {
            return Some(FormatEditSkipReason::DuplicateFormat);
        }
    }
    runs.sort_by_key(|run| (run.start, run.end));
    if runs.windows(2).any(|pair| pair[0].end > pair[1].start) {
        return Some(FormatEditSkipReason::OverlappingRuns);
    }
    None
}

fn modal_plain_code(characters: &[RuntimeCharacter]) -> Option<u32> {
    let mut counts: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
    for (position, character) in characters.iter().enumerate() {
        if character.verified && !character.image && character.attr == Some(0) {
            let code = character.code?;
            let entry = counts.entry(code).or_insert((0, position));
            entry.0 += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_code, (count, first))| (*count, usize::MAX - *first))
        .map(|(code, _)| code)
}

fn format_mask(formats: &[CharacterFormat]) -> u32 {
    formats.iter().fold(0, |mask, format| {
        mask | match format {
            CharacterFormat::Underline => 1,
            CharacterFormat::Bold => 2,
            CharacterFormat::Italic => 4,
            CharacterFormat::Superscript => 16,
            CharacterFormat::Subscript => 32,
        }
    })
}

fn skipped(
    operation: &ParagraphFormatEdit,
    reason: FormatEditSkipReason,
    source_text_sha256: Option<String>,
) -> FormatEditDecision {
    FormatEditDecision {
        paragraph_id: operation.paragraph_id.clone(),
        status: FormatEditDecisionStatus::Skipped,
        reason: Some(reason),
        source_text_sha256,
        align: operation.align,
        run_count: operation.runs.len() as u16,
    }
}

fn text_sha256(text: &str) -> String {
    format!("{:X}", Sha256::digest(text.as_bytes()))
}
