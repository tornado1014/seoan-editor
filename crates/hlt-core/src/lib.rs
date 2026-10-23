#![allow(clippy::result_large_err)]

mod amend_marks;
mod batch;
mod cancel;
mod cell_format_edit;
mod claim_check;
mod codec;
mod container;
mod cp949_safe;
mod docx;
mod docx_write;
mod drawmark;
mod edit;
mod editor;
mod error;
mod export_fin;
mod export_kipo;
mod export_opinion;
mod format_edit;
mod format_inspect;
mod format_rules;
mod from_hlt;
mod generate;
mod generate_body;
mod generate_characters;
mod generate_images;
mod generate_profile;
mod generate_tail;
mod hml;
mod import_opinion;
mod inspect;
mod kipo_xml;
mod numbering;
mod package;
mod paginate;
#[cfg(feature = "pdf")]
mod pdf;
mod print;
mod read_model;
mod reflow;
mod review_report;
mod save;
mod schema;
mod serde_u64;
mod source_view;
mod submit_check;
mod table_edit;
mod template;
mod text_policy;
mod variable_edit;
mod verify;
mod write;

pub use amend_marks::{
    AmendRow, AmendmentMarks, BasisHit, CitationIssue, CitationKind, ClaimBasis, FigureSignatures,
    ParagraphMark, PhraseBasis, amendment_basis, amendment_marks, amendment_marks_with,
    amendment_table, claim_citation_issues, claim_parents, number_unnumbered,
};
pub use batch::{BatchCheckRow, BatchRow, batch_generate_hlt, batch_submission_checks};
pub use cancel::{CancellationToken, ProgressEvent, ProgressSink, ProgressTotals, VerifyOptions};
pub use cell_format_edit::{
    CellFormatEdit, CellFormatEditDecision, CellFormatEditDecisionStatus, CellFormatEditPlan,
    CellFormatEditRequest, CellFormatEditSaveResult, CellFormatEditSkipReason,
    CellFormatEditStructureResult, plan_cell_format_edits, save_cell_format_edits_as_new,
};
pub use claim_check::{
    CLAIM_ERROR_KINDS, ClaimCheckReport, ClaimVerdict, check_claims, claim_list_from_xml,
};
pub use codec::{
    FirstBlockEvidence, FirstBlockKind, InflateSummary, inflate_raw_stream, inspect_first_block,
    max_backreference_distance,
};
pub use container::{ContainerLayout, Footer, HLT_HEADER_LEN, HLT_MAGIC, read_layout};
pub use cp949_safe::{
    Cp949Choices, Cp949Finding, Cp949Substitution, apply_cp949, cp949_safe_text, make_cp949_safe,
    scan_cp949, scan_cp949_text,
};
pub use docx::{
    DocxDocument, DocxMediaAsset, DocxPackageSummary, DocxPageBreak, inspect_docx_path,
    load_docx_image_payloads,
};
pub use docx_write::{comparison_table_docx, docx_bytes};
pub use drawmark::{DrawMark, read_drf, write_drf};
pub use edit::{
    EditDecision, EditDecisionStatus, EditPlan, EditSaveResult, EditSkipReason,
    EditStructureResult, ExactTextEditRequest, ParagraphTextEdit, document_revision,
    plan_exact_text_edits, save_exact_text_as_new,
};
pub use editor::{
    EditorParagraph, EditorSession, EditorSpan, FigureKind, FindFormat, FindMatch, FindQuery,
    ReplaceFormat, ReviewOutcome,
};
pub use error::{CoreError, ErrorCode};
pub use export_fin::{
    AMENDABLE, AmendmentEntry, AmendmentExport, AmendmentOptions, ElementKey, FinAmendment,
    FinAmendmentExport, FinExport, FinExportOptions, amendable_elements, apply_amendment,
    build_amendment_dta, build_amendment_dta_with_images, build_amendment_xml, build_fin,
    build_hlz, build_xresult, diff_elements, diff_elements_named, dta_version,
    export_amendment_from_fin, export_amendment_path, export_fin_path, final_version_from_fin,
    invention_titles, original_version_from_fin, sha256_hex,
};
pub use export_kipo::{
    KipoExport, KipoExportImage, KipoExportOptions, display_units_to_mm,
    display_units_to_mm_import, export_kipo_xml, export_kipo_xml_path, is_import_convention,
};
pub use format_edit::{
    FormatEditDecision, FormatEditDecisionStatus, FormatEditPlan, FormatEditRequest,
    FormatEditSaveResult, FormatEditSkipReason, FormatEditStructureResult, ParagraphAlign,
    ParagraphFormatEdit, RunStyleEdit, plan_format_edits, save_format_edits_as_new,
};
pub use format_inspect::{
    KNOWN_CHARACTER_FORMAT_MASK, inspect_cell_format_inventory, inspect_format_inventories,
    inspect_format_inventory,
};
pub use format_rules::{
    CellWriteFields, CellWriteSpec, FormatRuleError, cell_width_to_colwidth,
    colwidth_to_cell_width, image_display_units_from_mm, is_centered_top_level_anchor,
    secondary_image_display_units,
};
pub use from_hlt::hml_document_from_hlt;
pub use generate::{
    GeneratedHltDocument, generate_hlt_document, generate_hlt_document_with_profile,
    load_generation_input, load_generation_source,
};
pub use generate_body::{
    GeneratedFormatSpan, GeneratedImageAnchor, GeneratedLine, GeneratedLineEnding, HltBodyPlan,
    plan_hlt_body,
};
pub use generate_characters::{
    GeneratedCharacterPlan, GeneratedFormatCode, generate_hlt_character_spans, plan_hlt_characters,
};
pub use generate_images::{
    GeneratedImageObject, GeneratedImagePayload, GeneratedImagePlan, TiffPixels,
    build_hlt_image_record, build_rgb_tiff, build_tiff, load_hml_image_payloads_from_directory,
    plan_hlt_images,
};
pub use generate_profile::{
    AMENDMENT_DOCUMENT_TYPE, GenerationDialect, GenerationProfile, amendment_profile,
    generation_profile,
};
pub use generate_tail::{
    GeneratedCellLineRange, GeneratedCellRecord, GeneratedRowRecord, GeneratedTableTailPlan,
    GeneratedTerminalCell, generate_cell_table, generate_font_table, generate_line_array,
    generate_section_register, plan_table_tail,
};
pub use hml::{
    HmlCell, HmlDocument, HmlImage, HmlInline, HmlParagraph, HmlSection, HmlSummary, HmlTable,
    hml_semantic_sha256, inspect_hml_path,
};
pub use import_opinion::opinion_hlt_from_text;
pub use inspect::{HltImagePayload, InspectDocument, InspectOptions, inspect_path};
pub use kipo_xml::{KipoXmlDialect, KipoXmlDocument, inspect_kipo_xml_path};
pub use numbering::{number_paragraphs_as_new, plan_paragraph_numbers};
pub use package::{hlt_from_original_fin, hlt_from_package};
pub use paginate::{page_count, page_layout};
#[cfg(feature = "pdf")]
pub use pdf::write_pdf;
pub use print::{
    PagePlan, PlannedCellBox, PlannedGlyph, PlannedLine, PlannedPage, PrintOptions, page_plan,
};
pub use read_model::{
    CellCharacterFormatSpan, CellFormatInventory, CellFormatRecord, CellRecord, CharacterFormat,
    CharacterFormatSpan, CharacterSizeRun, DocumentTreeKind, FixedBlockFingerprints,
    FormatInventory, FormatParagraph, ImageOccurrence, InspectSummary, LineRecord, ParagraphRecord,
    SectionRecord, ShapeRecord, SourceRange, TableRecord, UnresolvedCellFormat, UnresolvedFormat,
};
pub use review_report::{
    DescriptionNote, FigureNote, Proposal, ProposedClaim, ReviewReport, read_review_report,
    review_from_document_xml,
};
pub use save::{
    GeneratedHltSaveResult, SaveAsNewResult, StructuralParity, WriteOptions, repack_as_new,
    save_generated_hlt_as_new, save_generated_hlt_with_profile_as_new, save_new_document_as_new,
    save_new_opinion_as_new,
};
pub use schema::{PackageReport, SchemaReport, validate_kipo_package, validate_kipo_xml};
pub use source_view::{SourceParagraph, paragraph_keys, source_keys, source_paragraphs};
pub use submit_check::{SubmitIssue, SubmitKind, submission_checks};
pub use table_edit::{
    CellPosition, NEW_TABLE_TOTAL_WIDTH, TableEditDecision, TableEditDecisionStatus, TableEditPlan,
    TableEditRequest, TableEditSaveResult, TableEditSkipReason, TableOperation, apply_table_edits,
    cell_position, plan_table_edits,
};
pub use template::{
    DocumentRegions, NewDocument, document_regions, inflate_hlt, new_document_from_template,
    source_generation_profile, stored_wrap_audit,
};
pub use text_policy::{carry_character_codes, restore_automatic_line_breaks};
pub use variable_edit::{
    VariableEditSaveResult, VariableEditStructureResult, VariableTextEditRequest,
    plan_variable_text_edits, save_variable_text_as_new,
};
pub use verify::{VerifyResult, WindowProof, verify_path, verify_reader};
pub use write::{WriteSummary, write_hlt};

pub const CONTRACT_VERSION: u32 = 1;
