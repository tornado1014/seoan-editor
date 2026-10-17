use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    CellRecord, CharacterFormat, CoreError, ErrorCode, ImageOccurrence, InspectDocument,
    LineRecord, ParagraphRecord, RunStyleEdit, TableRecord, VerifyOptions, VerifyResult,
    WriteOptions, WriteSummary, document_revision,
    error::{io_error, safe_source},
    format_inspect::{
        RuntimeCharacter, RuntimeFormatState, build_cell_format_inventory,
        inspect_format_state_from_source,
    },
    inspect::inspect_edit_layout,
    save::{InflatedPatch, open_inflated_source, publish_inflated_as_new, rewrite_inflated_spool},
};

const MAX_OPERATIONS: usize = 500;
const MAX_RUNS: usize = 5_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellFormatEdit {
    pub cell_id: String,
    pub expected_text_sha256: String,
    pub runs: Vec<RunStyleEdit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellFormatEditRequest {
    #[serde(with = "crate::serde_u64::as_string")]
    pub expected_revision: u64,
    pub operations: Vec<CellFormatEdit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellFormatEditDecisionStatus {
    Ready,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellFormatEditSkipReason {
    CellNotFound,
    DuplicateCellId,
    SourceMismatch,
    EmptyRun,
    RunOutOfRange,
    OverlappingRuns,
    DuplicateFormat,
    UnverifiedSourceFormat,
    ImageMarkerIntersection,
    StyleUnavailable,
    NoChange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellFormatEditDecision {
    pub cell_id: String,
    pub status: CellFormatEditDecisionStatus,
    pub reason: Option<CellFormatEditSkipReason>,
    pub source_text_sha256: Option<String>,
    pub run_count: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellFormatEditPlan {
    pub source: String,
    #[serde(with = "crate::serde_u64::as_string")]
    pub base_revision: u64,
    pub operation_count: u16,
    pub total_runs: u32,
    pub ready: u16,
    pub skipped: u16,
    pub decisions: Vec<CellFormatEditDecision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellFormatEditStructureResult {
    pub header_equal: bool,
    pub body_size_delta_equal: bool,
    pub counts_equal: bool,
    pub fixed_blocks_equal: bool,
    pub target_formats_equal: bool,
    pub non_target_formats_equal: bool,
    pub cell_text_equal: bool,
    pub paragraph_text_equal: bool,
    pub paragraph_shapes_equal: bool,
    pub tables_equal: bool,
    pub images_equal: bool,
    pub wipo_equal: bool,
    pub char_spans_valid: bool,
    pub body_changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellFormatEditSaveResult {
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
    pub edit_plan: CellFormatEditPlan,
    pub structure: CellFormatEditStructureResult,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedCellFormat {
    pub(crate) cell_id: String,
    pub(crate) runs: Vec<PlannedCellFormatRun>,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedCellFormatRun {
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) code: u32,
    pub(crate) formats: Vec<CharacterFormat>,
}

pub(crate) struct CellFormatEditTransaction {
    pub(crate) plan: CellFormatEditPlan,
    pub(crate) operations: Vec<PlannedCellFormat>,
    pub(crate) source_formats: RuntimeFormatState,
}

pub fn plan_cell_format_edits(
    path: &Path,
    request: &CellFormatEditRequest,
) -> Result<CellFormatEditPlan, CoreError> {
    let mut source = open_inflated_source(path, &VerifyOptions::default())?;
    Ok(build_cell_format_edit_transaction(&mut source, request)?.plan)
}

pub fn save_cell_format_edits_as_new(
    source_path: &Path,
    output: &Path,
    request: &CellFormatEditRequest,
    options: WriteOptions,
) -> Result<CellFormatEditSaveResult, CoreError> {
    let mut source = open_inflated_source(source_path, &options.verify)?;
    let transaction = build_cell_format_edit_transaction(&mut source, request)?;
    if transaction.operations.is_empty() {
        return Err(no_ready_error(&source.source_name, &transaction.plan));
    }
    let mut patches = build_cell_character_patches(&mut source, &transaction.operations)?;
    if patches.is_empty() {
        return Err(no_ready_error(&source.source_name, &transaction.plan));
    }
    let expected_inflated_delta = patch_delta(&patches)?;
    let rewrite = rewrite_inflated_spool(&mut source, &mut patches)?;
    if rewrite.input_bytes != source.document.summary.inflated_bytes
        || rewrite.delta_bytes != expected_inflated_delta
    {
        return Err(structure_error(
            &source.source_name,
            "cell_format_edit_rewrite_delta",
        ));
    }
    let rewritten_layout =
        inspect_edit_layout(&mut source.spool, rewrite.output_bytes).map_err(|_| {
            structure_error(&source.source_name, "cell_format_edit_char_span_invariant")
        })?;
    if rewritten_layout.total_lines != source.document.summary.line_count
        || rewritten_layout.char_spans.len() as u64 != source.document.summary.span_count
    {
        return Err(structure_error(
            &source.source_name,
            "cell_format_edit_char_span_invariant",
        ));
    }
    let output_formats = inspect_format_state_from_source(&mut source)?;
    let rewritten_body_sha256 = sha256_file(&mut source.spool, &source.source_name)?;

    let mut accepted_structure = None;
    let published = publish_inflated_as_new(
        &mut source,
        output,
        &options.verify,
        |input, output_document, output_body_sha256| {
            let structure = compare_cell_format_structure(
                input,
                output_document,
                output_body_sha256,
                &rewritten_body_sha256,
                expected_inflated_delta,
                &transaction.source_formats,
                &output_formats,
                &transaction.operations,
            );
            if cell_format_structure_passes(&structure) {
                accepted_structure = Some(structure);
                Ok(())
            } else {
                Err(parity_error(&input.source_name, &structure))
            }
        },
    )?;
    let structure = accepted_structure.ok_or_else(|| {
        CoreError::new(
            ErrorCode::VerificationFailed,
            "cell_format_edit_parity",
            false,
        )
    })?;
    let next_revision = document_revision(&published.output_sha256)?;

    Ok(CellFormatEditSaveResult {
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

pub(crate) fn build_cell_format_edit_transaction(
    source: &mut crate::save::InflatedSource,
    request: &CellFormatEditRequest,
) -> Result<CellFormatEditTransaction, CoreError> {
    let total_runs = validate_request_bounds(&source.source_name, request)?;
    let base_revision = document_revision(&source.verification.input_sha256)?;
    if request.expected_revision != base_revision {
        let mut error = CoreError::new(
            ErrorCode::RevisionConflict,
            "cell_format_edit_revision",
            false,
        );
        error.source = safe_source(&source.source_name);
        error.expected = Some(base_revision.to_string());
        error.actual = Some(request.expected_revision.to_string());
        return Err(error);
    }

    let cells = source.document.cells.clone();
    let state = inspect_format_state_from_source(source)?;
    let inventory = build_cell_format_inventory(&cells, &state);
    let inventory_by_id: BTreeMap<&str, _> = inventory
        .cells
        .iter()
        .map(|cell| (cell.cell_id.as_str(), cell))
        .collect();
    let cells_by_id: BTreeMap<&str, _> =
        cells.iter().map(|cell| (cell.id.as_str(), cell)).collect();
    let unresolved_cells: BTreeSet<&str> = inventory
        .unresolved
        .iter()
        .map(|item| item.cell_id.as_str())
        .collect();

    let mut seen = BTreeSet::new();
    let mut decisions = Vec::with_capacity(request.operations.len());
    let mut planned = Vec::new();
    for operation in &request.operations {
        if !seen.insert(operation.cell_id.as_str()) {
            decisions.push(skipped(
                operation,
                CellFormatEditSkipReason::DuplicateCellId,
                None,
            ));
            continue;
        }
        let (Some(cell), Some(inventory_cell)) = (
            cells_by_id.get(operation.cell_id.as_str()),
            inventory_by_id.get(operation.cell_id.as_str()),
        ) else {
            decisions.push(skipped(
                operation,
                CellFormatEditSkipReason::CellNotFound,
                None,
            ));
            continue;
        };
        let source_text_sha256 = text_sha256(&inventory_cell.text);
        if operation.expected_text_sha256 != source_text_sha256 {
            decisions.push(skipped(
                operation,
                CellFormatEditSkipReason::SourceMismatch,
                Some(source_text_sha256),
            ));
            continue;
        }
        let Some(characters) = cell_characters(cell, &state) else {
            decisions.push(skipped(
                operation,
                CellFormatEditSkipReason::UnverifiedSourceFormat,
                Some(source_text_sha256),
            ));
            continue;
        };
        if unresolved_cells.contains(operation.cell_id.as_str())
            || characters.iter().any(|character| !character.verified)
        {
            decisions.push(skipped(
                operation,
                CellFormatEditSkipReason::UnverifiedSourceFormat,
                Some(source_text_sha256),
            ));
            continue;
        }

        let mut ranges = operation.runs.clone();
        if let Some(reason) = validate_runs(&mut ranges, characters.len()) {
            decisions.push(skipped(operation, reason, Some(source_text_sha256)));
            continue;
        }
        let plain_code = modal_plain_code(&characters);
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
                run_error = Some(CellFormatEditSkipReason::ImageMarkerIntersection);
                break;
            }
            let code = if mask == 0 {
                plain_code.or_else(|| state.style_codes.get(&0).copied())
            } else {
                state.style_codes.get(&mask).copied()
            };
            let Some(code) = code else {
                run_error = Some(CellFormatEditSkipReason::StyleUnavailable);
                break;
            };
            planned_runs.push(PlannedCellFormatRun {
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
        let formats_change = planned_runs.iter().any(|run| {
            characters[run.start as usize..run.end as usize]
                .iter()
                .any(|character| character.code != Some(run.code))
        });
        if !formats_change {
            decisions.push(skipped(
                operation,
                CellFormatEditSkipReason::NoChange,
                Some(source_text_sha256),
            ));
            continue;
        }

        decisions.push(CellFormatEditDecision {
            cell_id: operation.cell_id.clone(),
            status: CellFormatEditDecisionStatus::Ready,
            reason: None,
            source_text_sha256: Some(source_text_sha256),
            run_count: operation.runs.len() as u16,
        });
        planned.push(PlannedCellFormat {
            cell_id: operation.cell_id.clone(),
            runs: planned_runs,
        });
    }

    let ready = decisions
        .iter()
        .filter(|decision| decision.status == CellFormatEditDecisionStatus::Ready)
        .count() as u16;
    Ok(CellFormatEditTransaction {
        plan: CellFormatEditPlan {
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

fn build_cell_character_patches(
    source: &mut crate::save::InflatedSource,
    operations: &[PlannedCellFormat],
) -> Result<Vec<InflatedPatch>, CoreError> {
    let layout = inspect_edit_layout(&mut source.spool, source.document.summary.inflated_bytes)?;
    let cells: BTreeMap<&str, &CellRecord> = source
        .document
        .cells
        .iter()
        .map(|cell| (cell.id.as_str(), cell))
        .collect();
    let lines: BTreeMap<&str, &LineRecord> = source
        .document
        .lines
        .iter()
        .map(|line| (line.id.as_str(), line))
        .collect();
    let paragraphs: BTreeMap<&str, &ParagraphRecord> = source
        .document
        .paragraphs
        .iter()
        .map(|paragraph| (paragraph.id.as_str(), paragraph))
        .collect();
    let mut line_owners: BTreeMap<&str, usize> = BTreeMap::new();
    for cell in source.document.cells.iter() {
        for line_id in &cell.line_ids {
            *line_owners.entry(line_id.as_str()).or_default() += 1;
        }
    }

    let mut patches = Vec::new();
    for operation in operations {
        let cell = cells
            .get(operation.cell_id.as_str())
            .ok_or_else(|| structure_error(&source.source_name, "cell_format_edit_cell"))?;
        let mut character_index = 0_u32;
        let mut run_index = 0_usize;
        for line_id in &cell.line_ids {
            if line_owners.get(line_id.as_str()) != Some(&1) {
                return Err(structure_error(
                    &source.source_name,
                    "cell_format_edit_line_owner",
                ));
            }
            let line = lines
                .get(line_id.as_str())
                .ok_or_else(|| structure_error(&source.source_name, "cell_format_edit_line"))?;
            let paragraph = paragraphs.get(line.paragraph_id.as_str()).ok_or_else(|| {
                structure_error(&source.source_name, "cell_format_edit_paragraph")
            })?;
            let span = layout.char_spans.get(line.index as usize).ok_or_else(|| {
                structure_error(&source.source_name, "cell_format_edit_char_span")
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
                    return Err(structure_error(
                        &source.source_name,
                        "cell_format_edit_char_span",
                    ));
                }
                span.codes[..raw.len()].to_vec()
            };
            let mut byte_index = 0_usize;
            let mut line_changed = false;
            while byte_index < raw.len() {
                let width = if raw[byte_index] >= 0x81 { 2 } else { 1 };
                if byte_index + width > raw.len() {
                    return Err(structure_error(
                        &source.source_name,
                        "cell_format_edit_cp949",
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
                        structure_error(&source.source_name, "cell_format_edit_character_index")
                    })?;
                }
                byte_index += width;
            }
            if line_changed {
                let terminate = paragraph.line_ids.last() == Some(line_id);
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
            return Err(structure_error(
                &source.source_name,
                "cell_format_edit_character_index",
            ));
        }
    }
    Ok(patches)
}

fn encode_expanded_codes(codes: &[u32], terminate: bool) -> Result<Vec<u8>, CoreError> {
    let count = codes
        .len()
        .checked_add(usize::from(terminate))
        .ok_or_else(|| {
            CoreError::new(
                ErrorCode::ResourceTooLarge,
                "cell_format_edit_char_span",
                false,
            )
        })?;
    let count = u32::try_from(count).map_err(|_| {
        CoreError::new(
            ErrorCode::ResourceTooLarge,
            "cell_format_edit_char_span",
            false,
        )
    })?;
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
            "cell_format_edit_patch_layout",
            false,
        )
    })
}

#[allow(clippy::too_many_arguments)]
fn compare_cell_format_structure(
    source: &crate::save::InflatedSource,
    output_document: &InspectDocument,
    output_body_sha256: &str,
    rewritten_body_sha256: &str,
    expected_delta: i64,
    source_formats: &RuntimeFormatState,
    output_formats: &RuntimeFormatState,
    operations: &[PlannedCellFormat],
) -> CellFormatEditStructureResult {
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
    let source_cells = build_cell_format_inventory(&source.document.cells, source_formats);
    let output_cells = build_cell_format_inventory(&source.document.cells, output_formats);
    let (target_formats_equal, non_target_formats_equal) = cell_format_semantics_equal(
        &source.document.cells,
        source_formats,
        output_formats,
        operations,
    );

    CellFormatEditStructureResult {
        header_equal: true,
        body_size_delta_equal: expected_inflated == i128::from(output_summary.inflated_bytes),
        counts_equal,
        fixed_blocks_equal: input_summary.fixed_blocks == output_summary.fixed_blocks,
        target_formats_equal,
        non_target_formats_equal,
        cell_text_equal: source_cells.unresolved.is_empty()
            && output_cells.unresolved.is_empty()
            && source_cells
                .cells
                .iter()
                .map(|cell| (&cell.cell_id, &cell.text))
                .eq(output_cells
                    .cells
                    .iter()
                    .map(|cell| (&cell.cell_id, &cell.text))),
        paragraph_text_equal: paragraph_text_equal(source_formats, output_formats)
            && paragraph_semantics_equal(&source.document, output_document),
        paragraph_shapes_equal: paragraph_shapes_equal(&source.document, output_document),
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

fn cell_format_semantics_equal(
    cells: &[CellRecord],
    source: &RuntimeFormatState,
    output: &RuntimeFormatState,
    operations: &[PlannedCellFormat],
) -> (bool, bool) {
    if !source.inventory.unresolved.is_empty() || !output.inventory.unresolved.is_empty() {
        return (false, false);
    }
    let targets: BTreeMap<&str, &[PlannedCellFormatRun]> = operations
        .iter()
        .map(|operation| (operation.cell_id.as_str(), operation.runs.as_slice()))
        .collect();
    let mut target_equal = true;
    let mut non_target_equal = true;
    for cell in cells {
        let Some(source_characters) = cell_characters(cell, source) else {
            return (false, false);
        };
        let Some(output_characters) = cell_characters(cell, output) else {
            return (false, false);
        };
        if source_characters.len() != output_characters.len() {
            return (false, false);
        }
        let runs = targets.get(cell.id.as_str()).copied().unwrap_or(&[]);
        for (index, (source_character, output_character)) in source_characters
            .iter()
            .zip(output_characters.iter())
            .enumerate()
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

fn paragraph_text_equal(source: &RuntimeFormatState, output: &RuntimeFormatState) -> bool {
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

fn paragraph_semantics_equal(input: &InspectDocument, output: &InspectDocument) -> bool {
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

fn paragraph_shapes_equal(input: &InspectDocument, output: &InspectDocument) -> bool {
    input.shapes == output.shapes
        && input.paragraphs.len() == output.paragraphs.len()
        && input
            .paragraphs
            .iter()
            .zip(&output.paragraphs)
            .all(|(input, output)| {
                input.id == output.id
                    && input.shape_id == output.shape_id
                    && input.indent_raw == output.indent_raw
                    && input.lspace == output.lspace
                    && input.align == output.align
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

fn cell_format_structure_passes(structure: &CellFormatEditStructureResult) -> bool {
    structure.header_equal
        && structure.body_size_delta_equal
        && structure.counts_equal
        && structure.fixed_blocks_equal
        && structure.target_formats_equal
        && structure.non_target_formats_equal
        && structure.cell_text_equal
        && structure.paragraph_text_equal
        && structure.paragraph_shapes_equal
        && structure.tables_equal
        && structure.images_equal
        && structure.wipo_equal
        && structure.char_spans_valid
        && structure.body_changed
}

fn parity_error(source: &str, structure: &CellFormatEditStructureResult) -> CoreError {
    let mut error = CoreError::new(
        ErrorCode::VerificationFailed,
        "cell_format_edit_parity",
        false,
    );
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
        ("cell_text_equal", structure.cell_text_equal),
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

fn no_ready_error(source: &str, plan: &CellFormatEditPlan) -> CoreError {
    let mut error = CoreError::new(ErrorCode::ConversionFailed, "cell_format_edit_apply", false);
    error.source = safe_source(source);
    error
        .details
        .insert("ready".to_string(), plan.ready.to_string());
    error
        .details
        .insert("skipped".to_string(), plan.skipped.to_string());
    error
}

fn read_range(
    file: &mut File,
    start: u64,
    length: u64,
    source: &str,
) -> Result<Vec<u8>, CoreError> {
    let length = usize::try_from(length)
        .map_err(|_| CoreError::new(ErrorCode::ResourceTooLarge, "cell_format_edit_line", false))?;
    file.seek(SeekFrom::Start(start))
        .map_err(|error| io_error(&error, "cell_format_edit_line", source))?;
    let mut output = vec![0_u8; length];
    file.read_exact(&mut output)
        .map_err(|error| io_error(&error, "cell_format_edit_line", source))?;
    Ok(output)
}

fn sha256_file(file: &mut File, source: &str) -> Result<String, CoreError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| io_error(&error, "cell_format_edit_hash", source))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| io_error(&error, "cell_format_edit_hash", source))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|error| io_error(&error, "cell_format_edit_hash", source))?;
    Ok(format!("{:X}", hasher.finalize()))
}

fn structure_error(source: &str, phase: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::StructureInvalid, phase, false);
    error.source = safe_source(source);
    error
}

fn cell_characters(
    cell: &crate::CellRecord,
    state: &RuntimeFormatState,
) -> Option<Vec<RuntimeCharacter>> {
    let mut characters = Vec::new();
    for line_id in &cell.line_ids {
        characters.extend_from_slice(state.line_characters.get(line_id)?);
    }
    Some(characters)
}

fn validate_request_bounds(
    source: &str,
    request: &CellFormatEditRequest,
) -> Result<usize, CoreError> {
    let total_runs = request
        .operations
        .iter()
        .try_fold(0_usize, |sum, operation| {
            sum.checked_add(operation.runs.len())
        });
    if request.operations.len() > MAX_OPERATIONS || total_runs.is_none_or(|count| count > MAX_RUNS)
    {
        let mut error = CoreError::new(
            ErrorCode::ResourceTooLarge,
            "cell_format_edit_operations",
            false,
        );
        error.source = safe_source(source);
        return Err(error);
    }
    Ok(total_runs.expect("validated run count"))
}

fn validate_runs(
    runs: &mut [RunStyleEdit],
    character_count: usize,
) -> Option<CellFormatEditSkipReason> {
    for run in runs.iter() {
        if run.start >= run.end {
            return Some(CellFormatEditSkipReason::EmptyRun);
        }
        if run.end as usize > character_count {
            return Some(CellFormatEditSkipReason::RunOutOfRange);
        }
        let unique: BTreeSet<_> = run.formats.iter().copied().collect();
        if unique.len() != run.formats.len() {
            return Some(CellFormatEditSkipReason::DuplicateFormat);
        }
    }
    runs.sort_by_key(|run| (run.start, run.end));
    if runs.windows(2).any(|pair| pair[0].end > pair[1].start) {
        return Some(CellFormatEditSkipReason::OverlappingRuns);
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

pub(crate) fn format_mask(formats: &[CharacterFormat]) -> u32 {
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
    operation: &CellFormatEdit,
    reason: CellFormatEditSkipReason,
    source_text_sha256: Option<String>,
) -> CellFormatEditDecision {
    CellFormatEditDecision {
        cell_id: operation.cell_id.clone(),
        status: CellFormatEditDecisionStatus::Skipped,
        reason: Some(reason),
        source_text_sha256,
        run_count: operation.runs.len() as u16,
    }
}

fn text_sha256(text: &str) -> String {
    format!("{:X}", Sha256::digest(text.as_bytes()))
}
