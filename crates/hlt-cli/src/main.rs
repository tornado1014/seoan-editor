#![allow(clippy::result_large_err)]

use std::{
    fs::File,
    io::{self, Read, Write},
    path::Path,
};

use hlt_core::{
    CONTRACT_VERSION, CellFormatEditPlan, CellFormatEditRequest, CellFormatEditSaveResult,
    CharacterFormat, CoreError, DocxPackageSummary, EditPlan, EditSaveResult, ErrorCode,
    ExactTextEditRequest, FormatEditPlan, FormatEditRequest, FormatEditSaveResult,
    GeneratedHltSaveResult, GeneratedImagePayload, GenerationDialect, HmlDocument, HmlSummary,
    InspectOptions, InspectSummary, KipoXmlDialect, SaveAsNewResult, TableEditPlan,
    TableEditRequest, TableEditSaveResult, VariableEditSaveResult, VerifyOptions, VerifyResult,
    WriteOptions, apply_table_edits, hml_semantic_sha256, inspect_cell_format_inventory,
    inspect_docx_path, inspect_format_inventory, inspect_hml_path, inspect_kipo_xml_path,
    inspect_path, plan_cell_format_edits, plan_exact_text_edits, plan_format_edits,
    plan_table_edits, plan_variable_text_edits, repack_as_new, save_cell_format_edits_as_new,
    save_exact_text_as_new, save_format_edits_as_new, save_generated_hlt_as_new,
    save_new_document_as_new, save_variable_text_as_new, verify_path,
};
use serde::Serialize;

const EXIT_OK: u8 = 0;
const EXIT_VERIFY_FAILED: u8 = 2;
const EXIT_USAGE: u8 = 64;
const MAX_OPERATIONS_FILE_BYTES: u64 = 5 * 1024 * 1024;
const MAX_EDIT_OPERATIONS: usize = 500;
const MAX_FORMAT_RUNS: usize = 5_000;
const MAX_REPLACEMENT_UTF8_BYTES: usize = 4 * 1024 * 1024;

#[derive(Serialize)]
pub struct CommandEnvelope<T> {
    pub contract_version: u32,
    pub request_id: String,
    pub command: String,
    pub ok: bool,
    pub document_id: Option<String>,
    #[serde(with = "option_u64_string")]
    pub revision: Option<u64>,
    pub result: Option<T>,
    pub warnings: Vec<String>,
    pub error: Option<CoreError>,
}

#[derive(Serialize)]
pub struct InspectPage {
    pub source: String,
    pub summary: InspectSummary,
    pub kind: String,
    #[serde(with = "u64_string")]
    pub cursor: u64,
    pub limit: u16,
    #[serde(with = "option_u64_string")]
    pub next_cursor: Option<u64>,
    pub has_more: bool,
    pub items: Vec<serde_json::Value>,
}

#[derive(Serialize)]
pub struct HmlInspectPage {
    pub source: String,
    pub source_sha256: String,
    pub encoding: String,
    pub title: String,
    pub fonts: Vec<String>,
    pub summary: HmlSummary,
    pub kind: String,
    #[serde(with = "u64_string")]
    pub cursor: u64,
    pub limit: u16,
    #[serde(with = "option_u64_string")]
    pub next_cursor: Option<u64>,
    pub has_more: bool,
    pub items: Vec<serde_json::Value>,
}

#[derive(Serialize)]
pub struct KipoXmlInspectPage {
    pub source: String,
    pub source_sha256: String,
    pub input_encoding: String,
    pub document_root: String,
    pub dialect: KipoXmlDialect,
    pub invention_title: String,
    pub semantic_sha256: String,
    pub page: HmlInspectPage,
}

#[derive(Serialize)]
pub struct DocxInspectPage {
    pub source: String,
    pub source_sha256: String,
    pub semantic_sha256: String,
    pub package: DocxPackageSummary,
    pub page_break_count: u32,
    pub kind: String,
    #[serde(with = "u64_string")]
    pub cursor: u64,
    pub limit: u16,
    #[serde(with = "option_u64_string")]
    pub next_cursor: Option<u64>,
    pub has_more: bool,
    pub page: Option<HmlInspectPage>,
    pub media_items: Vec<serde_json::Value>,
}

#[derive(Serialize)]
pub struct FormatSpanItem {
    pub paragraph_id: String,
    pub start: u32,
    pub end: u32,
    pub raw_code: u32,
    pub raw_attr: u32,
    pub formats: Vec<CharacterFormat>,
}

#[derive(Serialize)]
pub struct FormatInventoryPage {
    pub source: String,
    pub source_sha256: String,
    #[serde(with = "u64_string")]
    pub paragraph_count: u64,
    #[serde(with = "u64_string")]
    pub formatted_span_count: u64,
    #[serde(with = "u64_string")]
    pub unresolved_count: u64,
    #[serde(with = "u64_string")]
    pub cursor: u64,
    pub limit: u16,
    #[serde(with = "option_u64_string")]
    pub next_cursor: Option<u64>,
    pub has_more: bool,
    pub items: Vec<FormatSpanItem>,
}

#[derive(Serialize)]
pub struct CellFormatSpanItem {
    pub cell_id: String,
    pub table_id: String,
    pub start: u32,
    pub end: u32,
    pub raw_code: u32,
    pub raw_attr: u32,
    pub formats: Vec<CharacterFormat>,
}

#[derive(Serialize)]
pub struct CellFormatInventoryPage {
    pub source: String,
    pub source_sha256: String,
    #[serde(with = "u64_string")]
    pub cell_count: u64,
    #[serde(with = "u64_string")]
    pub formatted_span_count: u64,
    #[serde(with = "u64_string")]
    pub unresolved_count: u64,
    #[serde(with = "u64_string")]
    pub cursor: u64,
    pub limit: u16,
    #[serde(with = "option_u64_string")]
    pub next_cursor: Option<u64>,
    pub has_more: bool,
    pub items: Vec<CellFormatSpanItem>,
}

fn main() {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    let exit_code = run(std::env::args().skip(1), &mut stdout, &mut stderr);
    std::process::exit(i32::from(exit_code));
}

fn run<I, O, E>(args: I, stdout: &mut O, stderr: &mut E) -> u8
where
    I: IntoIterator<Item = String>,
    O: Write,
    E: Write,
{
    let args: Vec<String> = args.into_iter().collect();
    if args
        .first()
        .is_some_and(|value| value == "export-amendment")
    {
        return run_export_amendment(&args, stdout, stderr);
    }
    if args.first().is_some_and(|value| value == "export-fin") {
        return run_export_fin(&args, stdout, stderr);
    }
    if args
        .first()
        .is_some_and(|value| value == "validate-kipo-xml")
    {
        return run_validate_kipo_xml(&args, stdout);
    }
    if args.first().is_some_and(|value| value == "check-claims") {
        return run_check_claims(&args, stdout);
    }
    if args.first().is_some_and(|value| value == "export-kipo-xml") {
        return run_export_kipo_xml(&args, stdout, stderr);
    }
    if args.first().is_some_and(|value| value == "generate-hlt") {
        return run_generate_hlt(&args, stdout);
    }
    if args.first().is_some_and(|value| value == "inspect-docx") {
        return run_inspect_docx(&args, stdout, stderr);
    }
    if args
        .first()
        .is_some_and(|value| value == "inspect-kipo-xml")
    {
        return run_inspect_kipo_xml(&args, stdout, stderr);
    }
    if args.first().is_some_and(|value| value == "inspect-hml") {
        return run_inspect_hml(&args, stdout, stderr);
    }
    if args.first().is_some_and(|value| value == "inspect") {
        return run_inspect(&args, stdout, stderr);
    }
    if args.first().is_some_and(|value| value == "inspect-formats") {
        return run_inspect_formats(&args, stdout);
    }
    if args
        .first()
        .is_some_and(|value| value == "inspect-cell-formats")
    {
        return run_inspect_cell_formats(&args, stdout);
    }
    if args
        .first()
        .is_some_and(|value| value == "new-from-template")
    {
        return run_new_from_template(&args, stdout);
    }
    if args.first().is_some_and(|value| value == "print-pdf") {
        return run_print_pdf(&args, stdout);
    }
    if args.first().is_some_and(|value| value == "export-docx") {
        return run_file_command(&args, stdout, |input, output| {
            let bytes = hlt_core::EditorSession::open(input)?.docx()?;
            std::fs::write(output, &bytes)
                .map_err(|_| CoreError::new(ErrorCode::IoError, "write", false))?;
            Ok(serde_json::json!({ "bytes": bytes.len() }))
        });
    }
    if args.first().is_some_and(|value| value == "open-package") {
        return run_file_command(&args, stdout, |input, output| {
            let work = tempfile::tempdir()
                .map_err(|_| CoreError::new(ErrorCode::IoError, "work", false))?;
            let numbers = hlt_core::hlt_from_package(input, work.path(), output)?;
            Ok(serde_json::json!({ "paragraph_numbers": numbers }))
        });
    }
    if args.first().is_some_and(|value| value == "new-opinion") {
        return run_new_opinion(&args, stdout);
    }
    if args
        .first()
        .is_some_and(|value| value == "set-paragraph-shapes-as-new")
    {
        return run_set_paragraph_shapes(&args, stdout);
    }
    if args
        .first()
        .is_some_and(|value| value == "number-paragraphs")
    {
        return run_number_paragraphs(&args, stdout);
    }
    if args.first().is_some_and(|value| value == "repack-as-new") {
        return run_repack(&args, stdout);
    }
    if args.first().is_some_and(|value| value == "edit-plan") {
        return run_edit_plan(&args, stdout);
    }
    if args.first().is_some_and(|value| value == "save-as-new") {
        return run_edit_save(&args, stdout);
    }
    if args
        .first()
        .is_some_and(|value| value == "edit-plan-variable")
    {
        return run_variable_edit_plan(&args, stdout);
    }
    if args
        .first()
        .is_some_and(|value| value == "save-variable-as-new")
    {
        return run_variable_edit_save(&args, stdout);
    }
    if args.first().is_some_and(|value| value == "format-plan") {
        return run_format_edit_plan(&args, stdout);
    }
    if args
        .first()
        .is_some_and(|value| value == "save-formats-as-new")
    {
        return run_format_edit_save(&args, stdout);
    }
    if args
        .first()
        .is_some_and(|value| value == "cell-format-plan")
    {
        return run_cell_format_edit_plan(&args, stdout);
    }
    if args
        .first()
        .is_some_and(|value| value == "save-cell-formats-as-new")
    {
        return run_cell_format_edit_save(&args, stdout);
    }
    if args.first().is_some_and(|value| value == "table-plan") {
        return run_table_edit_plan(&args, stdout);
    }
    if args
        .first()
        .is_some_and(|value| value == "save-table-as-new")
    {
        return run_table_edit_save(&args, stdout);
    }
    let (input, human) = match args.as_slice() {
        [command, input] if command == "verify" => (input, false),
        [command, input, flag] if command == "verify" && flag == "--human" => (input, true),
        _ => {
            let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
            let envelope: CommandEnvelope<VerifyResult> = error_envelope("verify", error);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_USAGE;
        }
    };

    match verify_path(Path::new(input), VerifyOptions::default()) {
        Ok(result) => {
            let source = result.source.clone();
            let envelope = success_envelope("verify", result);
            if write_envelope(stdout, &envelope).is_err() {
                return EXIT_VERIFY_FAILED;
            }
            if human {
                let _ = writeln!(stderr, "HLT 검증 성공: {source}");
            }
            EXIT_OK
        }
        Err(error) => {
            let code = error.code;
            let envelope: CommandEnvelope<VerifyResult> = error_envelope("verify", error);
            let _ = write_envelope(stdout, &envelope);
            if human {
                let _ = writeln!(stderr, "HLT 검증 실패: {code}");
            }
            EXIT_VERIFY_FAILED
        }
    }
}

fn run_inspect_docx<O: Write, E: Write>(args: &[String], stdout: &mut O, _stderr: &mut E) -> u8 {
    let Some(input) = args.get(1) else {
        return inspect_docx_usage(stdout);
    };
    let mut kind = "summary".to_string();
    let mut cursor = 0_u64;
    let mut limit = 100_u16;
    let mut index = 2_usize;
    while index < args.len() {
        let flag = args[index].as_str();
        let Some(value) = args.get(index + 1) else {
            return inspect_docx_usage(stdout);
        };
        match flag {
            "--kind"
                if matches!(
                    value.as_str(),
                    "summary" | "sections" | "paragraphs" | "tables" | "cells" | "images" | "media"
                ) =>
            {
                kind = value.clone()
            }
            "--cursor" => match value.parse::<u64>() {
                Ok(parsed) => cursor = parsed,
                Err(_) => return inspect_docx_usage(stdout),
            },
            "--limit" => match value.parse::<u16>() {
                Ok(parsed) if (1..=500).contains(&parsed) => limit = parsed,
                _ => return inspect_docx_usage(stdout),
            },
            _ => return inspect_docx_usage(stdout),
        }
        index += 2;
    }
    match inspect_docx_path(Path::new(input)) {
        Ok(result) => {
            let semantic_sha256 = hml_semantic_sha256(&result.document);
            let (page, media_items, next_cursor, has_more) = if kind == "media" {
                let (items, next, more) = paginate(&result.media, cursor, limit);
                (None, items, next, more)
            } else {
                let document = result.document;
                let (items, next, more) = match kind.as_str() {
                    "summary" => (Vec::new(), None, false),
                    "sections" => paginate(&document.sections, cursor, limit),
                    "paragraphs" => paginate(&document.paragraphs, cursor, limit),
                    "tables" => paginate(&document.tables, cursor, limit),
                    "cells" => paginate(&document.cells, cursor, limit),
                    "images" => paginate(&document.images, cursor, limit),
                    _ => unreachable!("validated"),
                };
                let page = HmlInspectPage {
                    source: result.source.clone(),
                    source_sha256: result.source_sha256.clone(),
                    encoding: document.encoding,
                    title: document.title,
                    fonts: document.fonts,
                    summary: document.summary,
                    kind: kind.clone(),
                    cursor,
                    limit,
                    next_cursor: next,
                    has_more: more,
                    items,
                };
                (Some(page), Vec::new(), next, more)
            };
            let response = DocxInspectPage {
                source: result.source,
                source_sha256: result.source_sha256,
                semantic_sha256,
                package: result.package,
                page_break_count: result.page_breaks.len() as u32,
                kind,
                cursor,
                limit,
                next_cursor,
                has_more,
                page,
                media_items,
            };
            if write_envelope(stdout, &success_envelope("inspect-docx", response)).is_err() {
                return EXIT_VERIFY_FAILED;
            }
            EXIT_OK
        }
        Err(error) => {
            let envelope: CommandEnvelope<DocxInspectPage> = error_envelope("inspect-docx", error);
            let _ = write_envelope(stdout, &envelope);
            EXIT_VERIFY_FAILED
        }
    }
}

fn run_inspect_kipo_xml<O: Write, E: Write>(args: &[String], stdout: &mut O, stderr: &mut E) -> u8 {
    let Some(input) = args.get(1) else {
        return inspect_kipo_xml_usage(stdout);
    };
    let mut kind = "summary".to_string();
    let mut cursor = 0_u64;
    let mut limit = 100_u16;
    let mut human = false;
    let mut index = 2_usize;
    while index < args.len() {
        match args[index].as_str() {
            "--human" => {
                human = true;
                index += 1;
            }
            "--kind" | "--cursor" | "--limit" => {
                let flag = args[index].as_str();
                let Some(value) = args.get(index + 1) else {
                    return inspect_kipo_xml_usage(stdout);
                };
                match flag {
                    "--kind"
                        if matches!(
                            value.as_str(),
                            "summary" | "sections" | "paragraphs" | "tables" | "cells" | "images"
                        ) =>
                    {
                        kind = value.clone();
                    }
                    "--cursor" => match value.parse::<u64>() {
                        Ok(parsed) => cursor = parsed,
                        Err(_) => return inspect_kipo_xml_usage(stdout),
                    },
                    "--limit" => match value.parse::<u16>() {
                        Ok(parsed) if (1..=500).contains(&parsed) => limit = parsed,
                        _ => return inspect_kipo_xml_usage(stdout),
                    },
                    _ => return inspect_kipo_xml_usage(stdout),
                }
                index += 2;
            }
            _ => return inspect_kipo_xml_usage(stdout),
        }
    }

    match inspect_kipo_xml_path(Path::new(input)) {
        Ok(result) => {
            let document = result.document;
            let (items, next_cursor, has_more) = match kind.as_str() {
                "summary" => (Vec::new(), None, false),
                "sections" => paginate(&document.sections, cursor, limit),
                "paragraphs" => paginate(&document.paragraphs, cursor, limit),
                "tables" => paginate(&document.tables, cursor, limit),
                "cells" => paginate(&document.cells, cursor, limit),
                "images" => paginate(&document.images, cursor, limit),
                _ => unreachable!("kind validated above"),
            };
            let semantic_sha256 = hml_semantic_sha256(&document);
            let page = HmlInspectPage {
                source: result.source.clone(),
                source_sha256: result.source_sha256.clone(),
                encoding: result.input_encoding.clone(),
                title: document.title,
                fonts: document.fonts,
                summary: document.summary,
                kind,
                cursor,
                limit,
                next_cursor,
                has_more,
                items,
            };
            let source = result.source;
            let response = KipoXmlInspectPage {
                source: source.clone(),
                source_sha256: result.source_sha256,
                input_encoding: result.input_encoding,
                document_root: result.document_root,
                dialect: result.dialect,
                invention_title: result.invention_title,
                semantic_sha256,
                page,
            };
            let envelope = success_envelope("inspect-kipo-xml", response);
            if write_envelope(stdout, &envelope).is_err() {
                return EXIT_VERIFY_FAILED;
            }
            if human {
                let _ = writeln!(stderr, "KIPO XML 구조 검사 성공: {source}");
            }
            EXIT_OK
        }
        Err(error) => {
            let code = error.code;
            let envelope: CommandEnvelope<KipoXmlInspectPage> =
                error_envelope("inspect-kipo-xml", error);
            let _ = write_envelope(stdout, &envelope);
            if human {
                let _ = writeln!(stderr, "KIPO XML 구조 검사 실패: {code}");
            }
            EXIT_VERIFY_FAILED
        }
    }
}

fn run_inspect_hml<O: Write, E: Write>(args: &[String], stdout: &mut O, stderr: &mut E) -> u8 {
    let Some(input) = args.get(1) else {
        return inspect_hml_usage(stdout);
    };
    let mut kind = "summary".to_string();
    let mut cursor = 0_u64;
    let mut limit = 100_u16;
    let mut human = false;
    let mut index = 2_usize;
    while index < args.len() {
        match args[index].as_str() {
            "--human" => {
                human = true;
                index += 1;
            }
            "--kind" | "--cursor" | "--limit" => {
                let flag = args[index].as_str();
                let Some(value) = args.get(index + 1) else {
                    return inspect_hml_usage(stdout);
                };
                match flag {
                    "--kind"
                        if matches!(
                            value.as_str(),
                            "summary" | "sections" | "paragraphs" | "tables" | "cells" | "images"
                        ) =>
                    {
                        kind = value.clone();
                    }
                    "--cursor" => match value.parse::<u64>() {
                        Ok(parsed) => cursor = parsed,
                        Err(_) => return inspect_hml_usage(stdout),
                    },
                    "--limit" => match value.parse::<u16>() {
                        Ok(parsed) if (1..=500).contains(&parsed) => limit = parsed,
                        _ => return inspect_hml_usage(stdout),
                    },
                    _ => return inspect_hml_usage(stdout),
                }
                index += 2;
            }
            _ => return inspect_hml_usage(stdout),
        }
    }

    match inspect_hml_path(Path::new(input)) {
        Ok(document) => {
            let (items, next_cursor, has_more) = match kind.as_str() {
                "summary" => (Vec::new(), None, false),
                "sections" => paginate(&document.sections, cursor, limit),
                "paragraphs" => paginate(&document.paragraphs, cursor, limit),
                "tables" => paginate(&document.tables, cursor, limit),
                "cells" => paginate(&document.cells, cursor, limit),
                "images" => paginate(&document.images, cursor, limit),
                _ => unreachable!("kind validated above"),
            };
            let source = document.source.clone();
            let page = HmlInspectPage {
                source: source.clone(),
                source_sha256: document.source_sha256,
                encoding: document.encoding,
                title: document.title,
                fonts: document.fonts,
                summary: document.summary,
                kind,
                cursor,
                limit,
                next_cursor,
                has_more,
                items,
            };
            let envelope = success_envelope("inspect-hml", page);
            if write_envelope(stdout, &envelope).is_err() {
                return EXIT_VERIFY_FAILED;
            }
            if human {
                let _ = writeln!(stderr, "HML 구조 검사 성공: {source}");
            }
            EXIT_OK
        }
        Err(error) => {
            let code = error.code;
            let envelope: CommandEnvelope<HmlInspectPage> = error_envelope("inspect-hml", error);
            let _ = write_envelope(stdout, &envelope);
            if human {
                let _ = writeln!(stderr, "HML 구조 검사 실패: {code}");
            }
            EXIT_VERIFY_FAILED
        }
    }
}

fn run_inspect_formats(args: &[String], stdout: &mut impl Write) -> u8 {
    let Some(input) = args.get(1) else {
        return inspect_formats_usage(stdout);
    };
    let mut cursor = 0_u64;
    let mut limit = 100_u16;
    let mut index = 2_usize;
    while index < args.len() {
        let flag = args[index].as_str();
        let Some(value) = args.get(index + 1) else {
            return inspect_formats_usage(stdout);
        };
        match flag {
            "--cursor" => match value.parse::<u64>() {
                Ok(parsed) => cursor = parsed,
                Err(_) => return inspect_formats_usage(stdout),
            },
            "--limit" => match value.parse::<u16>() {
                Ok(parsed) if (1..=500).contains(&parsed) => limit = parsed,
                _ => return inspect_formats_usage(stdout),
            },
            _ => return inspect_formats_usage(stdout),
        }
        index += 2;
    }

    match inspect_format_inventory(Path::new(input)) {
        Ok(inventory) => {
            let paragraph_count = inventory.paragraphs.len() as u64;
            let unresolved_count = inventory.unresolved.len() as u64;
            let spans: Vec<FormatSpanItem> = inventory
                .paragraphs
                .into_iter()
                .flat_map(|paragraph| paragraph.spans)
                .map(|span| FormatSpanItem {
                    paragraph_id: span.paragraph_id,
                    start: span.start,
                    end: span.end,
                    raw_code: span.raw_code,
                    raw_attr: span.raw_attr,
                    formats: span.formats,
                })
                .collect();
            let formatted_span_count = spans.len() as u64;
            let start = usize::try_from(cursor)
                .unwrap_or(usize::MAX)
                .min(spans.len());
            let end = start.saturating_add(usize::from(limit)).min(spans.len());
            let has_more = end < spans.len();
            let next_cursor = has_more.then_some(end as u64);
            let page = FormatInventoryPage {
                source: inventory.source,
                source_sha256: inventory.source_sha256,
                paragraph_count,
                formatted_span_count,
                unresolved_count,
                cursor,
                limit,
                next_cursor,
                has_more,
                items: spans.into_iter().skip(start).take(end - start).collect(),
            };
            let envelope = success_envelope("inspect-formats", page);
            if write_envelope(stdout, &envelope).is_err() {
                return EXIT_VERIFY_FAILED;
            }
            EXIT_OK
        }
        Err(error) => {
            let envelope: CommandEnvelope<FormatInventoryPage> =
                error_envelope("inspect-formats", error);
            let _ = write_envelope(stdout, &envelope);
            EXIT_VERIFY_FAILED
        }
    }
}

fn run_inspect_cell_formats(args: &[String], stdout: &mut impl Write) -> u8 {
    let Some(input) = args.get(1) else {
        return inspect_cell_formats_usage(stdout);
    };
    let mut cursor = 0_u64;
    let mut limit = 100_u16;
    let mut index = 2_usize;
    while index < args.len() {
        let flag = args[index].as_str();
        let Some(value) = args.get(index + 1) else {
            return inspect_cell_formats_usage(stdout);
        };
        match flag {
            "--cursor" => match value.parse::<u64>() {
                Ok(parsed) => cursor = parsed,
                Err(_) => return inspect_cell_formats_usage(stdout),
            },
            "--limit" => match value.parse::<u16>() {
                Ok(parsed) if (1..=500).contains(&parsed) => limit = parsed,
                _ => return inspect_cell_formats_usage(stdout),
            },
            _ => return inspect_cell_formats_usage(stdout),
        }
        index += 2;
    }

    match inspect_cell_format_inventory(Path::new(input)) {
        Ok(inventory) => {
            let cell_count = inventory.cells.len() as u64;
            let unresolved_count = inventory.unresolved.len() as u64;
            let spans: Vec<CellFormatSpanItem> = inventory
                .cells
                .into_iter()
                .flat_map(|cell| {
                    let table_id = cell.table_id;
                    cell.spans.into_iter().map(move |span| CellFormatSpanItem {
                        cell_id: span.cell_id,
                        table_id: table_id.clone(),
                        start: span.start,
                        end: span.end,
                        raw_code: span.raw_code,
                        raw_attr: span.raw_attr,
                        formats: span.formats,
                    })
                })
                .collect();
            let formatted_span_count = spans.len() as u64;
            let start = usize::try_from(cursor)
                .unwrap_or(usize::MAX)
                .min(spans.len());
            let end = start.saturating_add(usize::from(limit)).min(spans.len());
            let has_more = end < spans.len();
            let next_cursor = has_more.then_some(end as u64);
            let page = CellFormatInventoryPage {
                source: inventory.source,
                source_sha256: inventory.source_sha256,
                cell_count,
                formatted_span_count,
                unresolved_count,
                cursor,
                limit,
                next_cursor,
                has_more,
                items: spans.into_iter().skip(start).take(end - start).collect(),
            };
            let envelope = success_envelope("inspect-cell-formats", page);
            if write_envelope(stdout, &envelope).is_err() {
                return EXIT_VERIFY_FAILED;
            }
            EXIT_OK
        }
        Err(error) => {
            let envelope: CommandEnvelope<CellFormatInventoryPage> =
                error_envelope("inspect-cell-formats", error);
            let _ = write_envelope(stdout, &envelope);
            EXIT_VERIFY_FAILED
        }
    }
}

fn run_format_edit_plan(args: &[String], stdout: &mut impl Write) -> u8 {
    let [command, input, operations_flag, operations_path] = args else {
        return format_edit_plan_usage(stdout);
    };
    debug_assert_eq!(command, "format-plan");
    if operations_flag != "--operations" {
        return format_edit_plan_usage(stdout);
    }
    let request = match load_format_operations(Path::new(operations_path)) {
        Ok(request) => request,
        Err(error) => {
            let envelope: CommandEnvelope<FormatEditPlan> = error_envelope("format-plan", *error);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_VERIFY_FAILED;
        }
    };
    match plan_format_edits(Path::new(input), &request) {
        Ok(result) => {
            let envelope = success_envelope("format-plan", result);
            if write_envelope(stdout, &envelope).is_err() {
                return EXIT_VERIFY_FAILED;
            }
            EXIT_OK
        }
        Err(error) => {
            let envelope: CommandEnvelope<FormatEditPlan> = error_envelope("format-plan", error);
            let _ = write_envelope(stdout, &envelope);
            EXIT_VERIFY_FAILED
        }
    }
}

fn run_format_edit_save(args: &[String], stdout: &mut impl Write) -> u8 {
    let [command, input, output, operations_flag, operations_path] = args else {
        return format_edit_save_usage(stdout);
    };
    debug_assert_eq!(command, "save-formats-as-new");
    if operations_flag != "--operations" {
        return format_edit_save_usage(stdout);
    }
    let request = match load_format_operations(Path::new(operations_path)) {
        Ok(request) => request,
        Err(error) => {
            let envelope: CommandEnvelope<FormatEditSaveResult> =
                error_envelope("save-formats-as-new", *error);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_VERIFY_FAILED;
        }
    };
    match save_format_edits_as_new(
        Path::new(input),
        Path::new(output),
        &request,
        WriteOptions::default(),
    ) {
        Ok(result) => {
            let envelope = success_envelope("save-formats-as-new", result);
            if write_envelope(stdout, &envelope).is_err() {
                return EXIT_VERIFY_FAILED;
            }
            EXIT_OK
        }
        Err(error) => {
            let envelope: CommandEnvelope<FormatEditSaveResult> =
                error_envelope("save-formats-as-new", error);
            let _ = write_envelope(stdout, &envelope);
            EXIT_VERIFY_FAILED
        }
    }
}

fn run_cell_format_edit_plan(args: &[String], stdout: &mut impl Write) -> u8 {
    let [command, input, operations_flag, operations_path] = args else {
        return cell_format_edit_plan_usage(stdout);
    };
    debug_assert_eq!(command, "cell-format-plan");
    if operations_flag != "--operations" {
        return cell_format_edit_plan_usage(stdout);
    }
    let request = match load_cell_format_operations(Path::new(operations_path)) {
        Ok(request) => request,
        Err(error) => {
            let envelope: CommandEnvelope<CellFormatEditPlan> =
                error_envelope("cell-format-plan", *error);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_VERIFY_FAILED;
        }
    };
    match plan_cell_format_edits(Path::new(input), &request) {
        Ok(result) => {
            let envelope = success_envelope("cell-format-plan", result);
            if write_envelope(stdout, &envelope).is_err() {
                return EXIT_VERIFY_FAILED;
            }
            EXIT_OK
        }
        Err(error) => {
            let envelope: CommandEnvelope<CellFormatEditPlan> =
                error_envelope("cell-format-plan", error);
            let _ = write_envelope(stdout, &envelope);
            EXIT_VERIFY_FAILED
        }
    }
}

fn run_cell_format_edit_save(args: &[String], stdout: &mut impl Write) -> u8 {
    let [command, input, output, operations_flag, operations_path] = args else {
        return cell_format_edit_save_usage(stdout);
    };
    debug_assert_eq!(command, "save-cell-formats-as-new");
    if operations_flag != "--operations" {
        return cell_format_edit_save_usage(stdout);
    }
    let request = match load_cell_format_operations(Path::new(operations_path)) {
        Ok(request) => request,
        Err(error) => {
            let envelope: CommandEnvelope<CellFormatEditSaveResult> =
                error_envelope("save-cell-formats-as-new", *error);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_VERIFY_FAILED;
        }
    };
    match save_cell_format_edits_as_new(
        Path::new(input),
        Path::new(output),
        &request,
        WriteOptions::default(),
    ) {
        Ok(result) => {
            let envelope = success_envelope("save-cell-formats-as-new", result);
            if write_envelope(stdout, &envelope).is_err() {
                return EXIT_VERIFY_FAILED;
            }
            EXIT_OK
        }
        Err(error) => {
            let envelope: CommandEnvelope<CellFormatEditSaveResult> =
                error_envelope("save-cell-formats-as-new", error);
            let _ = write_envelope(stdout, &envelope);
            EXIT_VERIFY_FAILED
        }
    }
}

fn run_edit_plan(args: &[String], stdout: &mut impl Write) -> u8 {
    let [command, input, operations_flag, operations_path] = args else {
        return edit_plan_usage(stdout);
    };
    debug_assert_eq!(command, "edit-plan");
    if operations_flag != "--operations" {
        return edit_plan_usage(stdout);
    }
    let request = match load_operations(Path::new(operations_path)) {
        Ok(request) => request,
        Err(error) => {
            let envelope: CommandEnvelope<EditPlan> = error_envelope("edit-plan", *error);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_VERIFY_FAILED;
        }
    };
    match plan_exact_text_edits(Path::new(input), &request) {
        Ok(result) => {
            let envelope = success_envelope("edit-plan", result);
            if write_envelope(stdout, &envelope).is_err() {
                return EXIT_VERIFY_FAILED;
            }
            EXIT_OK
        }
        Err(error) => {
            let envelope: CommandEnvelope<EditPlan> = error_envelope("edit-plan", error);
            let _ = write_envelope(stdout, &envelope);
            EXIT_VERIFY_FAILED
        }
    }
}

fn run_edit_save(args: &[String], stdout: &mut impl Write) -> u8 {
    let [command, input, output, operations_flag, operations_path] = args else {
        return edit_save_usage(stdout);
    };
    debug_assert_eq!(command, "save-as-new");
    if operations_flag != "--operations" {
        return edit_save_usage(stdout);
    }
    let request = match load_operations(Path::new(operations_path)) {
        Ok(request) => request,
        Err(error) => {
            let envelope: CommandEnvelope<EditSaveResult> = error_envelope("save-as-new", *error);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_VERIFY_FAILED;
        }
    };
    match save_exact_text_as_new(
        Path::new(input),
        Path::new(output),
        &request,
        WriteOptions::default(),
    ) {
        Ok(result) => {
            let envelope = success_envelope("save-as-new", result);
            if write_envelope(stdout, &envelope).is_err() {
                return EXIT_VERIFY_FAILED;
            }
            EXIT_OK
        }
        Err(error) => {
            let envelope: CommandEnvelope<EditSaveResult> = error_envelope("save-as-new", error);
            let _ = write_envelope(stdout, &envelope);
            EXIT_VERIFY_FAILED
        }
    }
}

fn run_variable_edit_plan(args: &[String], stdout: &mut impl Write) -> u8 {
    let [command, input, operations_flag, operations_path] = args else {
        return variable_edit_plan_usage(stdout);
    };
    debug_assert_eq!(command, "edit-plan-variable");
    if operations_flag != "--operations" {
        return variable_edit_plan_usage(stdout);
    }
    let request = match load_operations(Path::new(operations_path)) {
        Ok(request) => request,
        Err(error) => {
            let envelope: CommandEnvelope<EditPlan> = error_envelope("edit-plan-variable", *error);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_VERIFY_FAILED;
        }
    };
    match plan_variable_text_edits(Path::new(input), &request) {
        Ok(result) => {
            let envelope = success_envelope("edit-plan-variable", result);
            if write_envelope(stdout, &envelope).is_err() {
                return EXIT_VERIFY_FAILED;
            }
            EXIT_OK
        }
        Err(error) => {
            let envelope: CommandEnvelope<EditPlan> = error_envelope("edit-plan-variable", error);
            let _ = write_envelope(stdout, &envelope);
            EXIT_VERIFY_FAILED
        }
    }
}

fn run_variable_edit_save(args: &[String], stdout: &mut impl Write) -> u8 {
    let [command, input, output, operations_flag, operations_path] = args else {
        return variable_edit_save_usage(stdout);
    };
    debug_assert_eq!(command, "save-variable-as-new");
    if operations_flag != "--operations" {
        return variable_edit_save_usage(stdout);
    }
    let request = match load_operations(Path::new(operations_path)) {
        Ok(request) => request,
        Err(error) => {
            let envelope: CommandEnvelope<VariableEditSaveResult> =
                error_envelope("save-variable-as-new", *error);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_VERIFY_FAILED;
        }
    };
    match save_variable_text_as_new(
        Path::new(input),
        Path::new(output),
        &request,
        WriteOptions::default(),
    ) {
        Ok(result) => {
            let envelope = success_envelope("save-variable-as-new", result);
            if write_envelope(stdout, &envelope).is_err() {
                return EXIT_VERIFY_FAILED;
            }
            EXIT_OK
        }
        Err(error) => {
            let envelope: CommandEnvelope<VariableEditSaveResult> =
                error_envelope("save-variable-as-new", error);
            let _ = write_envelope(stdout, &envelope);
            EXIT_VERIFY_FAILED
        }
    }
}

fn load_operations(path: &Path) -> Result<ExactTextEditRequest, Box<CoreError>> {
    let source = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("operations.json")
        .to_string();
    let mut file = File::open(path).map_err(|error| operations_io_error(&error, &source))?;
    let metadata = file
        .metadata()
        .map_err(|error| operations_io_error(&error, &source))?;
    if metadata.len() > MAX_OPERATIONS_FILE_BYTES {
        return Err(operations_too_large(&source));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take(MAX_OPERATIONS_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| operations_io_error(&error, &source))?;
    if bytes.len() as u64 > MAX_OPERATIONS_FILE_BYTES {
        return Err(operations_too_large(&source));
    }
    let request: ExactTextEditRequest = serde_json::from_slice(&bytes)
        .map_err(|_| Box::new(operations_error(ErrorCode::ConversionFailed, &source)))?;
    let replacement_bytes = request
        .operations
        .iter()
        .try_fold(0_usize, |total, operation| {
            total.checked_add(operation.replacement.len())
        });
    if request.operations.len() > MAX_EDIT_OPERATIONS
        || replacement_bytes.is_none_or(|value| value > MAX_REPLACEMENT_UTF8_BYTES)
    {
        return Err(operations_too_large(&source));
    }
    Ok(request)
}

fn load_format_operations(path: &Path) -> Result<FormatEditRequest, Box<CoreError>> {
    let source = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("operations.json")
        .to_string();
    let mut file = File::open(path).map_err(|error| operations_io_error(&error, &source))?;
    let metadata = file
        .metadata()
        .map_err(|error| operations_io_error(&error, &source))?;
    if metadata.len() > MAX_OPERATIONS_FILE_BYTES {
        return Err(format_operations_too_large(&source));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take(MAX_OPERATIONS_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| operations_io_error(&error, &source))?;
    if bytes.len() as u64 > MAX_OPERATIONS_FILE_BYTES {
        return Err(format_operations_too_large(&source));
    }
    let request: FormatEditRequest = serde_json::from_slice(&bytes)
        .map_err(|_| Box::new(operations_error(ErrorCode::ConversionFailed, &source)))?;
    let total_runs = request
        .operations
        .iter()
        .try_fold(0_usize, |total, operation| {
            total.checked_add(operation.runs.len())
        });
    if request.operations.len() > MAX_EDIT_OPERATIONS
        || total_runs.is_none_or(|value| value > MAX_FORMAT_RUNS)
    {
        return Err(format_operations_too_large(&source));
    }
    Ok(request)
}

fn load_cell_format_operations(path: &Path) -> Result<CellFormatEditRequest, Box<CoreError>> {
    let source = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("operations.json")
        .to_string();
    let mut file = File::open(path).map_err(|error| operations_io_error(&error, &source))?;
    let metadata = file
        .metadata()
        .map_err(|error| operations_io_error(&error, &source))?;
    if metadata.len() > MAX_OPERATIONS_FILE_BYTES {
        return Err(format_operations_too_large(&source));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take(MAX_OPERATIONS_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| operations_io_error(&error, &source))?;
    if bytes.len() as u64 > MAX_OPERATIONS_FILE_BYTES {
        return Err(format_operations_too_large(&source));
    }
    let request: CellFormatEditRequest = serde_json::from_slice(&bytes)
        .map_err(|_| Box::new(operations_error(ErrorCode::ConversionFailed, &source)))?;
    let total_runs = request
        .operations
        .iter()
        .try_fold(0_usize, |total, operation| {
            total.checked_add(operation.runs.len())
        });
    if request.operations.len() > MAX_EDIT_OPERATIONS
        || total_runs.is_none_or(|value| value > MAX_FORMAT_RUNS)
    {
        return Err(format_operations_too_large(&source));
    }
    Ok(request)
}

fn format_operations_too_large(source: &str) -> Box<CoreError> {
    let mut error = operations_error(ErrorCode::ResourceTooLarge, source);
    error.expected = Some(format!(
        "file_bytes<={MAX_OPERATIONS_FILE_BYTES},operations<={MAX_EDIT_OPERATIONS},runs<={MAX_FORMAT_RUNS}"
    ));
    Box::new(error)
}

fn operations_too_large(source: &str) -> Box<CoreError> {
    let mut error = operations_error(ErrorCode::ResourceTooLarge, source);
    error.expected = Some(format!(
        "file_bytes<={MAX_OPERATIONS_FILE_BYTES},operations<={MAX_EDIT_OPERATIONS},replacement_utf8_bytes<={MAX_REPLACEMENT_UTF8_BYTES}"
    ));
    Box::new(error)
}

fn operations_io_error(error: &io::Error, source: &str) -> Box<CoreError> {
    let code = if error.kind() == io::ErrorKind::PermissionDenied {
        ErrorCode::PermissionDenied
    } else {
        ErrorCode::IoError
    };
    Box::new(operations_error(code, source))
}

fn operations_error(code: ErrorCode, source: &str) -> CoreError {
    let mut error = CoreError::new(code, "operations", false);
    error.source = Some(source.to_string());
    error
}

fn run_repack(args: &[String], stdout: &mut impl Write) -> u8 {
    let [command, input, output] = args else {
        return repack_usage(stdout);
    };
    debug_assert_eq!(command, "repack-as-new");
    match repack_as_new(Path::new(input), Path::new(output), WriteOptions::default()) {
        Ok(result) => {
            let envelope = success_envelope("repack-as-new", result);
            if write_envelope(stdout, &envelope).is_err() {
                return EXIT_VERIFY_FAILED;
            }
            EXIT_OK
        }
        Err(error) => {
            let envelope: CommandEnvelope<SaveAsNewResult> = error_envelope("repack-as-new", error);
            let _ = write_envelope(stdout, &envelope);
            EXIT_VERIFY_FAILED
        }
    }
}

/// `<명령> <input> <absent-output>` 꼴: 출력이 있으면 거부하고 `work` 결과를 봉투에 싣는다.
/// `export-docx <in.HLT> <out.docx>` · `open-package <in.hlz|in.fin> <out.HLT>`.
fn run_file_command(
    args: &[String],
    stdout: &mut impl Write,
    work: impl FnOnce(&Path, &Path) -> Result<serde_json::Value, CoreError>,
) -> u8 {
    let command = args[0].as_str();
    let [_, input, output] = args else {
        let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
        let _ = write_envelope(stdout, &error_envelope::<serde_json::Value>(command, error));
        return EXIT_USAGE;
    };
    let result = if Path::new(output).exists() {
        Err(CoreError::new(ErrorCode::OutputExists, "output", false))
    } else {
        work(Path::new(input), Path::new(output))
    };
    finish_value(command, result, stdout)
}

/// 새 의견서: `new-opinion <absent-output.HLT> [--body <body.txt>]` — 본문 줄 하나 = 문단 하나.
fn run_new_opinion(args: &[String], stdout: &mut impl Write) -> u8 {
    let (output, body) = match args {
        [_, output] => (output, None),
        [_, output, flag, body] if flag == "--body" => (output, Some(body)),
        _ => {
            let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
            let _ = write_envelope(
                stdout,
                &error_envelope::<serde_json::Value>("new-opinion", error),
            );
            return EXIT_USAGE;
        }
    };
    let result = (|| {
        if Path::new(output).exists() {
            return Err(CoreError::new(ErrorCode::OutputExists, "output", false));
        }
        let text = match body {
            Some(body) => std::fs::read_to_string(body)
                .map_err(|_| CoreError::new(ErrorCode::IoError, "body", false))?,
            None => String::new(),
        };
        hlt_core::opinion_hlt_from_text(&text, Path::new(output))?;
        Ok(serde_json::json!({ "paragraphs": text.lines().count() }))
    })();
    finish_value("new-opinion", result, stdout)
}

fn finish_value(
    command: &str,
    result: Result<serde_json::Value, CoreError>,
    stdout: &mut impl Write,
) -> u8 {
    let failed = result.is_err();
    let envelope = match result {
        Ok(result) => success_envelope(command, result),
        Err(error) => error_envelope(command, error),
    };
    if write_envelope(stdout, &envelope).is_err() || failed {
        return EXIT_VERIFY_FAILED;
    }
    EXIT_OK
}

/// 인쇄: `print-pdf <in.HLT> <out.pdf> --font <ttf/ttc> [--font-index N] [--no-page-numbers]`.
/// 글꼴은 사용자 시스템의 것을 쓴다(바탕체 재배포 금지 · 맥 `~/Library/Fonts/batang.ttc` 1번 = BatangChe).
fn run_print_pdf(args: &[String], stdout: &mut impl Write) -> u8 {
    fn usage(stdout: &mut impl Write) -> u8 {
        let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
        let envelope: CommandEnvelope<serde_json::Value> = error_envelope("print-pdf", error);
        let _ = write_envelope(stdout, &envelope);
        EXIT_USAGE
    }
    let (Some(input), Some(output)) = (args.get(1), args.get(2)) else {
        return usage(stdout);
    };
    let (mut font, mut index) = (None, 0_u32);
    let mut options = hlt_core::PrintOptions::default();
    let mut rest = args[3..].iter();
    while let Some(flag) = rest.next() {
        if flag == "--no-page-numbers" {
            options.page_numbers = false;
            continue;
        }
        match (flag.as_str(), rest.next()) {
            ("--font", Some(path)) => font = Some(path.clone()),
            ("--font-index", Some(value)) => match value.parse() {
                Ok(value) => index = value,
                Err(_) => return usage(stdout),
            },
            _ => return usage(stdout),
        }
    }
    let Some(font) = font else {
        return usage(stdout);
    };
    let result = (|| -> Result<serde_json::Value, CoreError> {
        if Path::new(output).exists() {
            return Err(CoreError::new(ErrorCode::OutputExists, "output", false));
        }
        let document = hlt_core::inspect_path(
            Path::new(input),
            hlt_core::InspectOptions {
                include_image_payloads: true,
                ..hlt_core::InspectOptions::default()
            },
        )?;
        let (_, inflated) = hlt_core::inflate_hlt(Path::new(input))?;
        let plan = hlt_core::page_plan(&document, &inflated, options);
        let font =
            std::fs::read(&font).map_err(|_| CoreError::new(ErrorCode::IoError, "font", false))?;
        let pdf = hlt_core::write_pdf(&plan, &document.image_payloads, font, index)?;
        std::fs::write(output, &pdf)
            .map_err(|_| CoreError::new(ErrorCode::IoError, "write", false))?;
        Ok(serde_json::json!({ "pages": plan.pages.len(), "bytes": pdf.len() }))
    })();
    let failed = result.is_err();
    let envelope = match result {
        Ok(result) => success_envelope("print-pdf", result),
        Err(error) => error_envelope("print-pdf", error),
    };
    if write_envelope(stdout, &envelope).is_err() || failed {
        return EXIT_VERIFY_FAILED;
    }
    EXIT_OK
}

/// 문단번호 넣기: 본문 문단에 식별번호를 1부터 매긴 새 파일.
fn run_number_paragraphs(args: &[String], stdout: &mut impl Write) -> u8 {
    let [_, input, output] = args else {
        let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
        let envelope: CommandEnvelope<SaveAsNewResult> = error_envelope("number-paragraphs", error);
        let _ = write_envelope(stdout, &envelope);
        return EXIT_USAGE;
    };
    let result = hlt_core::number_paragraphs_as_new(
        Path::new(input),
        Path::new(output),
        WriteOptions::default(),
    );
    let failed = result.is_err();
    let envelope = match result {
        Ok(result) => success_envelope("number-paragraphs", result),
        Err(error) => error_envelope("number-paragraphs", error),
    };
    if write_envelope(stdout, &envelope).is_err() || failed {
        return EXIT_VERIFY_FAILED;
    }
    EXIT_OK
}

#[derive(serde::Deserialize)]
struct ParagraphShapeRequest {
    operations: Vec<ParagraphShapeOperation>,
}

#[derive(serde::Deserialize)]
struct ParagraphShapeOperation {
    paragraph_id: String,
    align: String,
    indent_raw: i32,
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[derive(Serialize)]
struct ParagraphShapeResult {
    input_source: String,
    output_source: String,
    operation_count: usize,
    /// 저장본을 다시 열어 문단마다 (정렬, 들여쓰기)가 요청과 같은지 본 결과.
    readback_equal: bool,
}

/// 문단 정렬 · 들여쓰기를 바꿔 새 `.HLT` 로(오프라인 정렬 · 화학식 · 표 제목). 편집기 세션으로 다시 굽고
/// 저장본을 다시 열어 요청한 모양이 그대로인지 본다. 다르면 실패(출력 파일은 남긴다 — 판정은 봉투가 한다).
fn run_set_paragraph_shapes(args: &[String], stdout: &mut impl Write) -> u8 {
    const COMMAND: &str = "set-paragraph-shapes-as-new";
    let [_, input, output, flag, operations] = args else {
        let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
        let envelope: CommandEnvelope<ParagraphShapeResult> = error_envelope(COMMAND, error);
        let _ = write_envelope(stdout, &envelope);
        return EXIT_USAGE;
    };
    let result = (|| {
        if flag != "--operations" {
            return Err(CoreError::new(
                ErrorCode::VerificationFailed,
                "usage",
                false,
            ));
        }
        let request: ParagraphShapeRequest = std::fs::read(operations)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .ok_or_else(|| CoreError::new(ErrorCode::VerificationFailed, "operations", false))?;
        let shapes: Vec<(String, String, i32)> = request
            .operations
            .into_iter()
            .map(|op| (op.paragraph_id, op.align, op.indent_raw))
            .collect();
        let mut session = hlt_core::EditorSession::open(Path::new(input))?;
        session.set_paragraph_shapes(&shapes)?;
        session.save_as(Path::new(output))?;
        let reopened = hlt_core::EditorSession::open(Path::new(output))?;
        let readback_equal = shapes.iter().all(|(id, align, indent)| {
            reopened.document.paragraphs.iter().any(|p| {
                &p.id == id && &p.align == align && i64::from(p.indent_raw) == i64::from(*indent)
            })
        });
        Ok(ParagraphShapeResult {
            input_source: file_name(input),
            output_source: file_name(output),
            operation_count: shapes.len(),
            readback_equal,
        })
    })();
    let failed = !matches!(&result, Ok(r) if r.readback_equal);
    let envelope = match result {
        Ok(result) => success_envelope(COMMAND, result),
        Err(error) => error_envelope(COMMAND, error),
    };
    if write_envelope(stdout, &envelope).is_err() || failed {
        return EXIT_VERIFY_FAILED;
    }
    EXIT_OK
}

/// 통특기 서식(`Epasl/FRM/*.frm`) → 「새 문서」 `.HLT`. 통특기가 서식을 열고 새 이름으로 저장한 것과
/// 같은 바이트를 목표로 한다.
fn run_new_from_template(args: &[String], stdout: &mut impl Write) -> u8 {
    let [command, template, output] = args else {
        let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
        let envelope: CommandEnvelope<GeneratedHltSaveResult> =
            error_envelope("new-from-template", error);
        let _ = write_envelope(stdout, &envelope);
        return EXIT_USAGE;
    };
    debug_assert_eq!(command, "new-from-template");
    match save_new_document_as_new(
        Path::new(template),
        Path::new(output),
        WriteOptions::default(),
    ) {
        Ok(result) => {
            let envelope = success_envelope("new-from-template", result);
            if write_envelope(stdout, &envelope).is_err() {
                return EXIT_VERIFY_FAILED;
            }
            EXIT_OK
        }
        Err(error) => {
            let envelope: CommandEnvelope<GeneratedHltSaveResult> =
                error_envelope("new-from-template", error);
            let _ = write_envelope(stdout, &envelope);
            EXIT_VERIFY_FAILED
        }
    }
}

fn run_generate_hlt(args: &[String], stdout: &mut impl Write) -> u8 {
    let [command, input, output, dialect_flag, dialect_value] = args else {
        return generate_hlt_usage(stdout);
    };
    debug_assert_eq!(command, "generate-hlt");
    if dialect_flag != "--dialect" {
        return generate_hlt_usage(stdout);
    }
    let dialect = match dialect_value.as_str() {
        "patent-specification-ko" => GenerationDialect::PatentSpecificationKo,
        "pct-patent" => GenerationDialect::PctPatent,
        _ => return generate_hlt_usage(stdout),
    };
    let input_path = Path::new(input);
    let loaded = load_generation_document(input_path);
    let (document, payloads) = match loaded {
        Ok(value) => value,
        Err(error) => {
            let envelope: CommandEnvelope<GeneratedHltSaveResult> =
                error_envelope("generate-hlt", *error);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_VERIFY_FAILED;
        }
    };
    match save_generated_hlt_as_new(
        input_path,
        &document,
        dialect,
        &payloads,
        Path::new(output),
        WriteOptions::default(),
    ) {
        Ok(result) => {
            let envelope = success_envelope("generate-hlt", result);
            if write_envelope(stdout, &envelope).is_err() {
                return EXIT_VERIFY_FAILED;
            }
            EXIT_OK
        }
        Err(error) => {
            let envelope: CommandEnvelope<GeneratedHltSaveResult> =
                error_envelope("generate-hlt", error);
            let _ = write_envelope(stdout, &envelope);
            EXIT_VERIFY_FAILED
        }
    }
}

// 표 구조 편집은 생성 소스(.docx/.hml/.xml)의 의미 모델을 고친 뒤 HLT 를 다시 굽는다.
// HLT 를 되읽어 의미 모델로 되돌리는 경로는 아직 없다.
fn run_table_edit_plan(args: &[String], stdout: &mut impl Write) -> u8 {
    let [command, input, operations_flag, operations_path] = args else {
        return table_edit_plan_usage(stdout);
    };
    debug_assert_eq!(command, "table-plan");
    if operations_flag != "--operations" {
        return table_edit_plan_usage(stdout);
    }
    let request = match load_table_operations(Path::new(operations_path)) {
        Ok(request) => request,
        Err(error) => {
            let envelope: CommandEnvelope<TableEditPlan> = error_envelope("table-plan", *error);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_VERIFY_FAILED;
        }
    };
    let document = match load_generation_document(Path::new(input)) {
        Ok(value) => value.0,
        Err(error) => {
            let envelope: CommandEnvelope<TableEditPlan> = error_envelope("table-plan", *error);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_VERIFY_FAILED;
        }
    };
    match plan_table_edits(&document, &request.operations) {
        Ok(result) => {
            let envelope = success_envelope("table-plan", result);
            if write_envelope(stdout, &envelope).is_err() {
                return EXIT_VERIFY_FAILED;
            }
            EXIT_OK
        }
        Err(error) => {
            let envelope: CommandEnvelope<TableEditPlan> = error_envelope("table-plan", error);
            let _ = write_envelope(stdout, &envelope);
            EXIT_VERIFY_FAILED
        }
    }
}

fn run_table_edit_save(args: &[String], stdout: &mut impl Write) -> u8 {
    let [
        command,
        input,
        output,
        dialect_flag,
        dialect_value,
        operations_flag,
        operations_path,
    ] = args
    else {
        return table_edit_save_usage(stdout);
    };
    debug_assert_eq!(command, "save-table-as-new");
    if dialect_flag != "--dialect" || operations_flag != "--operations" {
        return table_edit_save_usage(stdout);
    }
    let dialect = match dialect_value.as_str() {
        "patent-specification-ko" => GenerationDialect::PatentSpecificationKo,
        "pct-patent" => GenerationDialect::PctPatent,
        _ => return table_edit_save_usage(stdout),
    };
    let request = match load_table_operations(Path::new(operations_path)) {
        Ok(request) => request,
        Err(error) => {
            let envelope: CommandEnvelope<TableEditSaveResult> =
                error_envelope("save-table-as-new", *error);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_VERIFY_FAILED;
        }
    };
    let input_path = Path::new(input);
    let (mut document, payloads) = match load_generation_document(input_path) {
        Ok(value) => value,
        Err(error) => {
            let envelope: CommandEnvelope<TableEditSaveResult> =
                error_envelope("save-table-as-new", *error);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_VERIFY_FAILED;
        }
    };
    let edit_plan = match apply_table_edits(&mut document, &request.operations) {
        Ok(plan) => plan,
        Err(error) => {
            let envelope: CommandEnvelope<TableEditSaveResult> =
                error_envelope("save-table-as-new", error);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_VERIFY_FAILED;
        }
    };
    match save_generated_hlt_as_new(
        input_path,
        &document,
        dialect,
        &payloads,
        Path::new(output),
        WriteOptions::default(),
    ) {
        Ok(generated) => {
            let envelope = success_envelope(
                "save-table-as-new",
                TableEditSaveResult {
                    generated,
                    edit_plan,
                },
            );
            if write_envelope(stdout, &envelope).is_err() {
                return EXIT_VERIFY_FAILED;
            }
            EXIT_OK
        }
        Err(error) => {
            let envelope: CommandEnvelope<TableEditSaveResult> =
                error_envelope("save-table-as-new", error);
            let _ = write_envelope(stdout, &envelope);
            EXIT_VERIFY_FAILED
        }
    }
}

fn load_table_operations(path: &Path) -> Result<TableEditRequest, Box<CoreError>> {
    let source = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("operations.json")
        .to_string();
    let mut file = File::open(path).map_err(|error| operations_io_error(&error, &source))?;
    let metadata = file
        .metadata()
        .map_err(|error| operations_io_error(&error, &source))?;
    if metadata.len() > MAX_OPERATIONS_FILE_BYTES {
        return Err(Box::new(operations_error(
            ErrorCode::SizeLimitExceeded,
            &source,
        )));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take(MAX_OPERATIONS_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| operations_io_error(&error, &source))?;
    if bytes.len() as u64 > MAX_OPERATIONS_FILE_BYTES {
        return Err(Box::new(operations_error(
            ErrorCode::SizeLimitExceeded,
            &source,
        )));
    }
    let request: TableEditRequest = serde_json::from_slice(&bytes)
        .map_err(|_| Box::new(operations_error(ErrorCode::ConversionFailed, &source)))?;
    if request.operations.len() > MAX_EDIT_OPERATIONS {
        return Err(Box::new(operations_error(
            ErrorCode::SizeLimitExceeded,
            &source,
        )));
    }
    Ok(request)
}

fn load_generation_document(
    input: &Path,
) -> Result<(HmlDocument, Vec<GeneratedImagePayload>), Box<CoreError>> {
    hlt_core::load_generation_input(input).map_err(Box::new)
}

fn table_edit_plan_usage(writer: &mut impl Write) -> u8 {
    let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
    let envelope: CommandEnvelope<TableEditPlan> = error_envelope("table-plan", error);
    let _ = write_envelope(writer, &envelope);
    EXIT_USAGE
}

fn table_edit_save_usage(writer: &mut impl Write) -> u8 {
    let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
    let envelope: CommandEnvelope<TableEditSaveResult> = error_envelope("save-table-as-new", error);
    let _ = write_envelope(writer, &envelope);
    EXIT_USAGE
}

fn run_inspect<O: Write, E: Write>(args: &[String], stdout: &mut O, stderr: &mut E) -> u8 {
    let Some(input) = args.get(1) else {
        return inspect_usage(stdout);
    };
    let mut kind = "summary".to_string();
    let mut cursor = 0_u64;
    let mut limit = 100_u16;
    let mut human = false;
    let mut index = 2_usize;
    while index < args.len() {
        match args[index].as_str() {
            "--human" => {
                human = true;
                index += 1;
            }
            "--kind" | "--cursor" | "--limit" => {
                let flag = args[index].as_str();
                let Some(value) = args.get(index + 1) else {
                    return inspect_usage(stdout);
                };
                match flag {
                    "--kind"
                        if matches!(
                            value.as_str(),
                            "summary" | "sections" | "paragraphs" | "tables" | "cells" | "images"
                        ) =>
                    {
                        kind = value.clone()
                    }
                    "--cursor" => match value.parse::<u64>() {
                        Ok(parsed) => cursor = parsed,
                        Err(_) => return inspect_usage(stdout),
                    },
                    "--limit" => match value.parse::<u16>() {
                        Ok(parsed) if (1..=500).contains(&parsed) => limit = parsed,
                        _ => return inspect_usage(stdout),
                    },
                    _ => return inspect_usage(stdout),
                }
                index += 2;
            }
            _ => return inspect_usage(stdout),
        }
    }

    match inspect_path(
        Path::new(input),
        InspectOptions {
            include_image_identities: kind == "images",
            ..InspectOptions::default()
        },
    ) {
        Ok(document) => {
            let source = Path::new(input)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("input.HLT")
                .to_string();
            let (items, next_cursor, has_more) = match kind.as_str() {
                "summary" => (Vec::new(), None, false),
                "sections" => paginate(&document.sections, cursor, limit),
                "paragraphs" => paginate(&document.paragraphs, cursor, limit),
                "tables" => paginate(&document.tables, cursor, limit),
                "cells" => paginate(&document.cells, cursor, limit),
                "images" => paginate(&document.images, cursor, limit),
                _ => unreachable!("kind validated above"),
            };
            let page = InspectPage {
                source: source.clone(),
                summary: document.summary.clone(),
                kind,
                cursor,
                limit,
                next_cursor,
                has_more,
                items,
            };
            let envelope = success_envelope("inspect", page);
            if write_envelope(stdout, &envelope).is_err() {
                return EXIT_VERIFY_FAILED;
            }
            if human {
                let _ = writeln!(stderr, "HLT 구조 검사 성공: {source}");
            }
            EXIT_OK
        }
        Err(error) => {
            let code = error.code;
            let envelope: CommandEnvelope<InspectPage> = error_envelope("inspect", error);
            let _ = write_envelope(stdout, &envelope);
            if human {
                let _ = writeln!(stderr, "HLT 구조 검사 실패: {code}");
            }
            EXIT_VERIFY_FAILED
        }
    }
}

fn paginate<T: Serialize>(
    values: &[T],
    cursor: u64,
    limit: u16,
) -> (Vec<serde_json::Value>, Option<u64>, bool) {
    let start = usize::try_from(cursor)
        .unwrap_or(usize::MAX)
        .min(values.len());
    let end = start.saturating_add(usize::from(limit)).min(values.len());
    let items = values[start..end]
        .iter()
        .map(|value| serde_json::to_value(value).expect("serializable inspect item"))
        .collect();
    let has_more = end < values.len();
    let next_cursor = has_more.then_some(end as u64);
    (items, next_cursor, has_more)
}

fn inspect_usage(writer: &mut impl Write) -> u8 {
    let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
    let envelope: CommandEnvelope<InspectPage> = error_envelope("inspect", error);
    let _ = write_envelope(writer, &envelope);
    EXIT_USAGE
}

fn inspect_hml_usage(writer: &mut impl Write) -> u8 {
    let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
    let envelope: CommandEnvelope<HmlInspectPage> = error_envelope("inspect-hml", error);
    let _ = write_envelope(writer, &envelope);
    EXIT_USAGE
}

fn inspect_kipo_xml_usage(writer: &mut impl Write) -> u8 {
    let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
    let envelope: CommandEnvelope<KipoXmlInspectPage> = error_envelope("inspect-kipo-xml", error);
    let _ = write_envelope(writer, &envelope);
    EXIT_USAGE
}

fn inspect_docx_usage(writer: &mut impl Write) -> u8 {
    let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
    let envelope: CommandEnvelope<DocxInspectPage> = error_envelope("inspect-docx", error);
    let _ = write_envelope(writer, &envelope);
    EXIT_USAGE
}

fn inspect_formats_usage(writer: &mut impl Write) -> u8 {
    let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
    let envelope: CommandEnvelope<FormatInventoryPage> = error_envelope("inspect-formats", error);
    let _ = write_envelope(writer, &envelope);
    EXIT_USAGE
}

fn inspect_cell_formats_usage(writer: &mut impl Write) -> u8 {
    let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
    let envelope: CommandEnvelope<CellFormatInventoryPage> =
        error_envelope("inspect-cell-formats", error);
    let _ = write_envelope(writer, &envelope);
    EXIT_USAGE
}

fn repack_usage(writer: &mut impl Write) -> u8 {
    let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
    let envelope: CommandEnvelope<SaveAsNewResult> = error_envelope("repack-as-new", error);
    let _ = write_envelope(writer, &envelope);
    EXIT_USAGE
}

fn generate_hlt_usage(writer: &mut impl Write) -> u8 {
    let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
    let envelope: CommandEnvelope<GeneratedHltSaveResult> = error_envelope("generate-hlt", error);
    let _ = write_envelope(writer, &envelope);
    EXIT_USAGE
}

fn edit_plan_usage(writer: &mut impl Write) -> u8 {
    let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
    let envelope: CommandEnvelope<EditPlan> = error_envelope("edit-plan", error);
    let _ = write_envelope(writer, &envelope);
    EXIT_USAGE
}

fn edit_save_usage(writer: &mut impl Write) -> u8 {
    let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
    let envelope: CommandEnvelope<EditSaveResult> = error_envelope("save-as-new", error);
    let _ = write_envelope(writer, &envelope);
    EXIT_USAGE
}

fn variable_edit_plan_usage(writer: &mut impl Write) -> u8 {
    let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
    let envelope: CommandEnvelope<EditPlan> = error_envelope("edit-plan-variable", error);
    let _ = write_envelope(writer, &envelope);
    EXIT_USAGE
}

fn variable_edit_save_usage(writer: &mut impl Write) -> u8 {
    let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
    let envelope: CommandEnvelope<VariableEditSaveResult> =
        error_envelope("save-variable-as-new", error);
    let _ = write_envelope(writer, &envelope);
    EXIT_USAGE
}

fn format_edit_plan_usage(writer: &mut impl Write) -> u8 {
    let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
    let envelope: CommandEnvelope<FormatEditPlan> = error_envelope("format-plan", error);
    let _ = write_envelope(writer, &envelope);
    EXIT_USAGE
}

fn format_edit_save_usage(writer: &mut impl Write) -> u8 {
    let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
    let envelope: CommandEnvelope<FormatEditSaveResult> =
        error_envelope("save-formats-as-new", error);
    let _ = write_envelope(writer, &envelope);
    EXIT_USAGE
}

fn cell_format_edit_plan_usage(writer: &mut impl Write) -> u8 {
    let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
    let envelope: CommandEnvelope<CellFormatEditPlan> = error_envelope("cell-format-plan", error);
    let _ = write_envelope(writer, &envelope);
    EXIT_USAGE
}

fn cell_format_edit_save_usage(writer: &mut impl Write) -> u8 {
    let error = CoreError::new(ErrorCode::VerificationFailed, "usage", false);
    let envelope: CommandEnvelope<CellFormatEditSaveResult> =
        error_envelope("save-cell-formats-as-new", error);
    let _ = write_envelope(writer, &envelope);
    EXIT_USAGE
}

fn success_envelope<T>(command: &str, result: T) -> CommandEnvelope<T> {
    CommandEnvelope {
        contract_version: CONTRACT_VERSION,
        request_id: uuid::Uuid::new_v4().to_string(),
        command: command.to_string(),
        ok: true,
        document_id: None,
        revision: None,
        result: Some(result),
        warnings: Vec::new(),
        error: None,
    }
}

fn error_envelope<T>(command: &str, error: CoreError) -> CommandEnvelope<T> {
    CommandEnvelope {
        contract_version: CONTRACT_VERSION,
        request_id: uuid::Uuid::new_v4().to_string(),
        command: command.to_string(),
        ok: false,
        document_id: None,
        revision: None,
        result: None,
        warnings: Vec::new(),
        error: Some(error),
    }
}

fn write_envelope<T: Serialize>(
    writer: &mut impl Write,
    envelope: &CommandEnvelope<T>,
) -> io::Result<()> {
    serde_json::to_writer(&mut *writer, envelope)?;
    writer.write_all(b"\n")
}

mod option_u64_string {
    use serde::Serializer;

    pub fn serialize<S>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(value) => serializer.serialize_some(&value.to_string()),
            None => serializer.serialize_none(),
        }
    }
}

mod u64_string {
    use serde::Serializer;

    pub fn serialize<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }
}

#[derive(Debug, Serialize)]
struct KipoXmlExportResult {
    input_source: String,
    output_source: String,
    output_sha256: String,
    output_bytes: String,
    paragraph_count: String,
    claim_count: String,
    figure_count: String,
    image_count: String,
}

fn export_kipo_xml_usage<O: Write>(stdout: &mut O) -> u8 {
    let _ = writeln!(
        stdout,
        "usage: hlt-cli export-kipo-xml <input.HLT> <absent-output.xml> [--document-id <id>] [--keaps-version <v>] [--image-stem <stem>] [--page-count <n>] [--schema-dir <Epasl/INCLUDE/Schema>]"
    );
    EXIT_USAGE
}

fn run_export_kipo_xml<O: Write, E: Write>(args: &[String], stdout: &mut O, _stderr: &mut E) -> u8 {
    let (Some(input), Some(output)) = (args.get(1), args.get(2)) else {
        return export_kipo_xml_usage(stdout);
    };
    let mut options = hlt_core::KipoExportOptions::default();
    let mut schema_dir: Option<String> = None;
    let mut index = 3_usize;
    while index < args.len() {
        let Some(value) = args.get(index + 1) else {
            return export_kipo_xml_usage(stdout);
        };
        match args[index].as_str() {
            "--document-id" => options.document_id = value.clone(),
            "--keaps-version" => options.keaps_version = value.clone(),
            "--image-stem" => options.image_stem = value.clone(),
            "--page-count" => match value.parse::<u32>() {
                Ok(parsed) if parsed >= 1 => options.page_count = Some(parsed),
                _ => return export_kipo_xml_usage(stdout),
            },
            "--schema-dir" => schema_dir = Some(value.clone()),
            _ => return export_kipo_xml_usage(stdout),
        }
        index += 2;
    }
    let input_path = Path::new(input);
    let output_path = Path::new(output);
    if output_path.exists() {
        let mut error = CoreError::new(ErrorCode::OutputExists, "export_kipo", false);
        error.source = output_path
            .file_name()
            .map(|value| value.to_string_lossy().to_string());
        let envelope: CommandEnvelope<KipoXmlExportResult> =
            error_envelope("export-kipo-xml", error);
        let _ = write_envelope(stdout, &envelope);
        return EXIT_VERIFY_FAILED;
    }
    let export = match hlt_core::export_kipo_xml_path(input_path, &options) {
        Ok(value) => value,
        Err(error) => {
            let envelope: CommandEnvelope<KipoXmlExportResult> =
                error_envelope("export-kipo-xml", error);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_VERIFY_FAILED;
        }
    };
    let bytes = export.xml.as_bytes();
    if let Err(error) = schema_gate("export.xml", bytes, schema_dir.as_deref()) {
        let envelope: CommandEnvelope<KipoXmlExportResult> =
            error_envelope("export-kipo-xml", error);
        let _ = write_envelope(stdout, &envelope);
        return EXIT_VERIFY_FAILED;
    }
    let mut file = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)
    {
        Ok(file) => file,
        Err(error) => {
            let mut failure = CoreError::new(ErrorCode::IoError, "export_kipo", false);
            failure
                .details
                .insert("io".into(), error.kind().to_string());
            let envelope: CommandEnvelope<KipoXmlExportResult> =
                error_envelope("export-kipo-xml", failure);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_VERIFY_FAILED;
        }
    };
    if file.write_all(bytes).is_err() || file.sync_all().is_err() {
        return EXIT_VERIFY_FAILED;
    }
    let result = KipoXmlExportResult {
        input_source: file_label(input_path),
        output_source: file_label(output_path),
        output_sha256: export.xml_sha256.clone(),
        output_bytes: bytes.len().to_string(),
        paragraph_count: export.paragraph_count.to_string(),
        claim_count: export.claim_count.to_string(),
        figure_count: export.figure_count.to_string(),
        image_count: export.images.len().to_string(),
    };
    let envelope = success_envelope("export-kipo-xml", result);
    if write_envelope(stdout, &envelope).is_err() {
        return EXIT_VERIFY_FAILED;
    }
    EXIT_OK
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default()
}

#[derive(Debug, Serialize)]
struct FinExportResult {
    input_source: String,
    output_source: String,
    output_sha256: String,
    output_bytes: String,
    hlz_bytes: String,
    xml_bytes: String,
    paragraph_count: String,
    claim_count: String,
    figure_count: String,
    image_count: String,
    amendment_count: String,
}

/// 제출 직전 KIPO 스키마 문(P4-1). `schema_dir` 가 없으면 열어 둔다 — KIPO 스키마는 저장소 밖 자산이라 경로로 받는다.
fn schema_gate(name: &str, bytes: &[u8], schema_dir: Option<&str>) -> Result<(), CoreError> {
    let Some(dir) = schema_dir else {
        return Ok(());
    };
    let report = hlt_core::validate_kipo_package(name, bytes, Path::new(dir))?;
    if report.valid() {
        return Ok(());
    }
    let mut error = CoreError::new(ErrorCode::VerificationFailed, "kipo_schema", false);
    if let Some((document, result)) = report.documents.iter().find(|(_, result)| !result.valid) {
        error.source = Some(document.clone());
        error
            .details
            .insert("errors".into(), result.errors.join("\n"));
    }
    Err(error)
}

#[derive(Debug, Serialize)]
struct SchemaDocumentResult {
    name: String,
    valid: bool,
    errors: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SchemaValidationResult {
    input_source: String,
    valid: bool,
    documents: Vec<SchemaDocumentResult>,
    skipped: Vec<String>,
}

fn validate_kipo_xml_usage<O: Write>(stdout: &mut O) -> u8 {
    let _ = writeln!(
        stdout,
        "usage: hlt-cli validate-kipo-xml <input.xml|input.hlz|input.fin> --schema-dir <Epasl/INCLUDE/Schema>"
    );
    EXIT_USAGE
}

/// KIPO 스키마 검증(통특기 제출파일 생성의 「XML 파일 검증」). 통과하지 않으면 결과를 실은 채 종료 코드로 알린다.
fn run_validate_kipo_xml<O: Write>(args: &[String], stdout: &mut O) -> u8 {
    let (Some(input), Some("--schema-dir"), Some(dir), None) = (
        args.get(1),
        args.get(2).map(String::as_str),
        args.get(3),
        args.get(4),
    ) else {
        return validate_kipo_xml_usage(stdout);
    };
    let input_path = Path::new(input);
    let report = std::fs::read(input_path)
        .map_err(|error| {
            let mut failure = CoreError::new(ErrorCode::IoError, "kipo_schema", false);
            failure
                .details
                .insert("io".into(), error.kind().to_string());
            failure
        })
        .and_then(|bytes| {
            hlt_core::validate_kipo_package(&file_label(input_path), &bytes, Path::new(dir))
        });
    let report = match report {
        Ok(report) => report,
        Err(error) => {
            let envelope: CommandEnvelope<SchemaValidationResult> =
                error_envelope("validate-kipo-xml", error);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_VERIFY_FAILED;
        }
    };
    let valid = report.valid();
    let result = SchemaValidationResult {
        input_source: file_label(input_path),
        valid,
        documents: report
            .documents
            .into_iter()
            .map(|(name, result)| SchemaDocumentResult {
                name,
                valid: result.valid,
                errors: result.errors,
            })
            .collect(),
        skipped: report.skipped,
    };
    let envelope = success_envelope("validate-kipo-xml", result);
    if write_envelope(stdout, &envelope).is_err() {
        return EXIT_VERIFY_FAILED;
    }
    if valid { EXIT_OK } else { EXIT_VERIFY_FAILED }
}

#[derive(Debug, Serialize)]
struct ClaimCheckResult {
    input_source: String,
    claim_count: usize,
    error_count: usize,
    /// 빈 번호에 채운 「삭제」 항(보정 명세서).
    filled: Vec<String>,
    claims: Vec<hlt_core::ClaimVerdict>,
}

fn check_claims_usage<O: Write>(stdout: &mut O) -> u8 {
    let _ = writeln!(
        stdout,
        "usage: hlt-cli check-claims <input.HLT|input.xml> --checker-dir <claim checker dir>"
    );
    EXIT_USAGE
}

/// 청구항 오류점검(특허로 점검 페이지 JS). 판정이 나면 오류가 있어도 0 — 페이지처럼 알려 주기만 한다.
fn run_check_claims<O: Write>(args: &[String], stdout: &mut O) -> u8 {
    let (Some(input), Some("--checker-dir"), Some(dir), None) = (
        args.get(1),
        args.get(2).map(String::as_str),
        args.get(3),
        args.get(4),
    ) else {
        return check_claims_usage(stdout);
    };
    let input_path = Path::new(input);
    let report = claim_check_xml(input_path).and_then(|xml| {
        let claims = hlt_core::claim_list_from_xml(&xml);
        hlt_core::check_claims(&claims, Path::new(dir)).map(|report| (claims.len(), report))
    });
    let (claim_count, report) = match report {
        Ok(value) => value,
        Err(error) => {
            let envelope: CommandEnvelope<ClaimCheckResult> = error_envelope("check-claims", error);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_VERIFY_FAILED;
        }
    };
    let result = ClaimCheckResult {
        input_source: file_label(input_path),
        claim_count,
        error_count: report.error_count(),
        filled: report.filled,
        claims: report.claims,
    };
    if write_envelope(stdout, &success_envelope("check-claims", result)).is_err() {
        return EXIT_VERIFY_FAILED;
    }
    EXIT_OK
}

/// 점검할 KIPO XML: `.HLT` 는 제출 XML 내보내기, `.xml` 은 그대로.
fn claim_check_xml(path: &Path) -> Result<String, CoreError> {
    let io = |kind: std::io::ErrorKind| {
        let mut failure = CoreError::new(ErrorCode::IoError, "claim_check", false);
        failure.details.insert("io".into(), kind.to_string());
        failure
    };
    let lower = path.to_string_lossy().to_ascii_lowercase();
    if lower.ends_with(".hlt") {
        return hlt_core::export_kipo_xml_path(path, &hlt_core::KipoExportOptions::default())
            .map(|export| export.xml);
    }
    let bytes = std::fs::read(path).map_err(|error| io(error.kind()))?;
    String::from_utf8(bytes)
        .map_err(|_| CoreError::new(ErrorCode::UnsupportedFormat, "claim_check", false))
}

fn export_fin_usage<O: Write>(stdout: &mut O) -> u8 {
    let _ = writeln!(
        stdout,
        "usage: hlt-cli export-fin <input.HLT> <absent-output.fin> --hlz-name <name.hlz> --app-date <YYYY-MM-DD> --app-time <stamp> [--image-time <YYYY:MM:DD HH:MM:SS>] [--schema-dir <Epasl/INCLUDE/Schema>] [--document-id <id>] [--page-count <n>] [--amendment <name.dta>=<amendment.xml>=<YYYY-MM-DD>]..."
    );
    EXIT_USAGE
}

fn run_export_fin<O: Write, E: Write>(args: &[String], stdout: &mut O, _stderr: &mut E) -> u8 {
    let (Some(input), Some(output)) = (args.get(1), args.get(2)) else {
        return export_fin_usage(stdout);
    };
    let mut kipo = hlt_core::KipoExportOptions::default();
    let mut fin = hlt_core::FinExportOptions {
        hlz_name: String::new(),
        app_date: String::new(),
        app_time: String::new(),
        image_time: String::new(),
        amendments: Vec::new(),
    };
    let mut schema_dir: Option<String> = None;
    let mut index = 3_usize;
    while index < args.len() {
        let Some(value) = args.get(index + 1) else {
            return export_fin_usage(stdout);
        };
        match args[index].as_str() {
            "--hlz-name" => fin.hlz_name = value.clone(),
            "--app-date" => fin.app_date = value.clone(),
            "--app-time" => fin.app_time = value.clone(),
            "--image-time" => fin.image_time = value.clone(),
            "--document-id" => kipo.document_id = value.clone(),
            "--keaps-version" => kipo.keaps_version = value.clone(),
            "--image-stem" => kipo.image_stem = value.clone(),
            "--page-count" => match value.parse::<u32>() {
                Ok(parsed) if parsed >= 1 => kipo.page_count = Some(parsed),
                _ => return export_fin_usage(stdout),
            },
            "--amendment" => {
                let parts: Vec<&str> = value.split('=').collect();
                let [name, xml_path, date] = parts.as_slice() else {
                    return export_fin_usage(stdout);
                };
                let Ok(xml) = std::fs::read_to_string(xml_path) else {
                    return export_fin_usage(stdout);
                };
                let Ok(bytes) = hlt_core::build_amendment_dta(name, &xml) else {
                    return export_fin_usage(stdout);
                };
                fin.amendments.push(hlt_core::FinAmendment {
                    file_name: (*name).to_string(),
                    date: (*date).to_string(),
                    bytes,
                });
            }
            "--schema-dir" => schema_dir = Some(value.clone()),
            _ => return export_fin_usage(stdout),
        }
        index += 2;
    }
    if fin.hlz_name.is_empty() || fin.app_date.is_empty() || fin.app_time.is_empty() {
        return export_fin_usage(stdout);
    }
    let input_path = Path::new(input);
    let output_path = Path::new(output);
    if output_path.exists() {
        let mut error = CoreError::new(ErrorCode::OutputExists, "export_fin", false);
        error.source = output_path
            .file_name()
            .map(|value| value.to_string_lossy().to_string());
        let envelope: CommandEnvelope<FinExportResult> = error_envelope("export-fin", error);
        let _ = write_envelope(stdout, &envelope);
        return EXIT_VERIFY_FAILED;
    }
    let export = match hlt_core::export_fin_path(input_path, &kipo, &fin) {
        Ok(value) => value,
        Err(error) => {
            let envelope: CommandEnvelope<FinExportResult> = error_envelope("export-fin", error);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_VERIFY_FAILED;
        }
    };
    if let Err(error) = schema_gate(&file_label(output_path), &export.fin, schema_dir.as_deref()) {
        let envelope: CommandEnvelope<FinExportResult> = error_envelope("export-fin", error);
        let _ = write_envelope(stdout, &envelope);
        return EXIT_VERIFY_FAILED;
    }
    let mut file = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)
    {
        Ok(file) => file,
        Err(error) => {
            let mut failure = CoreError::new(ErrorCode::IoError, "export_fin", false);
            failure
                .details
                .insert("io".into(), error.kind().to_string());
            let envelope: CommandEnvelope<FinExportResult> = error_envelope("export-fin", failure);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_VERIFY_FAILED;
        }
    };
    if file.write_all(&export.fin).is_err() || file.sync_all().is_err() {
        return EXIT_VERIFY_FAILED;
    }
    let result = FinExportResult {
        input_source: file_label(input_path),
        output_source: file_label(output_path),
        output_sha256: hlt_core::sha256_hex(&export.fin),
        output_bytes: export.fin.len().to_string(),
        hlz_bytes: export.hlz_bytes.to_string(),
        xml_bytes: export.xml_bytes.to_string(),
        paragraph_count: export.paragraph_count.to_string(),
        claim_count: export.claim_count.to_string(),
        figure_count: export.figure_count.to_string(),
        image_count: export.image_count.to_string(),
        amendment_count: fin.amendments.len().to_string(),
    };
    let envelope = success_envelope("export-fin", result);
    if write_envelope(stdout, &envelope).is_err() {
        return EXIT_VERIFY_FAILED;
    }
    EXIT_OK
}

#[derive(Debug, Serialize)]
struct AmendmentExportResult {
    before_source: String,
    after_source: String,
    output_source: String,
    output_sha256: String,
    output_bytes: String,
    amended_claims: String,
    deleted_claims: String,
}

fn export_amendment_usage<O: Write>(stdout: &mut O) -> u8 {
    let _ = writeln!(
        stdout,
        "usage: hlt-cli export-amendment <before.HLT> <after.HLT> <absent-output.dta> [--document-id <id>] [--dta-version <v>] [--keaps-version <v>] [--en-title <text>] [--image-time <YYYY:MM:DD HH:MM:SS>]\n       hlt-cli export-amendment <final.fin> <after.HLT> <absent-output.fin> --dta-name <name.dta> --date <YYYY-MM-DD>   (보정서생성: 새 .fin)"
    );
    EXIT_USAGE
}

fn run_export_amendment<O: Write, E: Write>(
    args: &[String],
    stdout: &mut O,
    _stderr: &mut E,
) -> u8 {
    let (Some(before), Some(after), Some(output)) = (args.get(1), args.get(2), args.get(3)) else {
        return export_amendment_usage(stdout);
    };
    let kipo = hlt_core::KipoExportOptions::default();
    let mut options = hlt_core::AmendmentOptions::default();
    // 보정 전이 최종본 `.fin` 이면 통특기 보정서생성처럼 새 `.fin`(dta 를 더한 것)을 쓴다.
    let fin_mode = before.to_ascii_lowercase().ends_with(".fin");
    if fin_mode {
        options.dta_version = String::new();
    }
    let (mut dta_name, mut date) = (String::new(), String::new());
    let mut index = 4_usize;
    while index < args.len() {
        let Some(value) = args.get(index + 1) else {
            return export_amendment_usage(stdout);
        };
        match args[index].as_str() {
            "--document-id" => options.document_id = value.clone(),
            "--dta-version" => options.dta_version = value.clone(),
            "--keaps-version" => options.keaps_version = value.clone(),
            "--doc-type" => options.doc_type = value.clone(),
            "--en-title" => options.invention_en_title = value.clone(),
            "--image-time" => options.image_time = value.clone(),
            "--ko-title" => options.invention_ko_title = value.clone(),
            "--dta-name" if fin_mode => dta_name = value.clone(),
            "--date" if fin_mode => date = value.clone(),
            _ => return export_amendment_usage(stdout),
        }
        index += 2;
    }
    let output_path = Path::new(output);
    if output_path.exists() {
        let mut error = CoreError::new(ErrorCode::OutputExists, "export_amendment", false);
        error.source = output_path
            .file_name()
            .map(|value| value.to_string_lossy().to_string());
        let envelope: CommandEnvelope<AmendmentExportResult> =
            error_envelope("export-amendment", error);
        let _ = write_envelope(stdout, &envelope);
        return EXIT_VERIFY_FAILED;
    }
    if fin_mode && (dta_name.is_empty() || date.is_empty()) {
        return export_amendment_usage(stdout);
    }
    let name = file_label(output_path);
    let exported = if fin_mode {
        hlt_core::export_amendment_from_fin(
            Path::new(before),
            Path::new(after),
            &dta_name,
            &date,
            &kipo,
            &options,
        )
        .map(|export| (export.fin, export.amended, export.deleted))
    } else {
        hlt_core::export_amendment_path(Path::new(before), Path::new(after), &name, &kipo, &options)
            .map(|export| (export.dta, export.amended, export.deleted))
    };
    let (bytes, amended, deleted) = match exported {
        Ok(value) => value,
        Err(error) => {
            let envelope: CommandEnvelope<AmendmentExportResult> =
                error_envelope("export-amendment", error);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_VERIFY_FAILED;
        }
    };
    let mut file = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)
    {
        Ok(file) => file,
        Err(error) => {
            let mut failure = CoreError::new(ErrorCode::IoError, "export_amendment", false);
            failure
                .details
                .insert("io".into(), error.kind().to_string());
            let envelope: CommandEnvelope<AmendmentExportResult> =
                error_envelope("export-amendment", failure);
            let _ = write_envelope(stdout, &envelope);
            return EXIT_VERIFY_FAILED;
        }
    };
    if file.write_all(&bytes).is_err() || file.sync_all().is_err() {
        return EXIT_VERIFY_FAILED;
    }
    let result = AmendmentExportResult {
        before_source: file_label(Path::new(before)),
        after_source: file_label(Path::new(after)),
        output_source: name,
        output_sha256: hlt_core::sha256_hex(&bytes),
        output_bytes: bytes.len().to_string(),
        amended_claims: amended.to_string(),
        deleted_claims: deleted.to_string(),
    };
    let envelope = success_envelope("export-amendment", result);
    if write_envelope(stdout, &envelope).is_err() {
        return EXIT_VERIFY_FAILED;
    }
    EXIT_OK
}
