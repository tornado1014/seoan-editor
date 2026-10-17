use std::{
    fs::{self, File},
    io::{Cursor, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    ContainerLayout, CoreError, ErrorCode, GeneratedImagePayload, GenerationDialect, HmlDocument,
    InspectDocument, InspectOptions, InspectSummary, VerifyOptions, VerifyResult, WriteSummary,
    codec::inflate_raw_to_writer,
    error::{io_error, safe_source},
    inflate_hlt, inspect_path, new_document_from_template, read_layout, verify_path, write_hlt,
};

const HASH_BUFFER_SIZE: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InflatedPatch {
    pub(crate) start_byte: u64,
    pub(crate) end_byte: u64,
    pub(crate) replacement: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RewriteSummary {
    pub(crate) input_bytes: u64,
    pub(crate) output_bytes: u64,
    pub(crate) delta_bytes: i64,
}

#[derive(Default)]
pub struct WriteOptions {
    pub verify: VerifyOptions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuralParity {
    pub header_equal: bool,
    pub inflated_equal: bool,
    pub summary_equal: bool,
    pub fixed_blocks_equal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveAsNewResult {
    pub input_source: String,
    pub output_source: String,
    #[serde(with = "crate::serde_u64::as_string")]
    pub input_file_size: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub output_file_size: u64,
    pub input_sha256: String,
    pub output_sha256: String,
    pub writer: WriteSummary,
    pub verification: VerifyResult,
    pub structural_parity: StructuralParity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedHltSaveResult {
    pub input_source: String,
    pub output_source: String,
    #[serde(with = "crate::serde_u64::as_string")]
    pub input_file_size: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub output_file_size: u64,
    pub input_sha256: String,
    pub output_sha256: String,
    pub writer: WriteSummary,
    pub verification: VerifyResult,
    pub summary: InspectSummary,
}

pub(crate) struct InflatedSource {
    pub(crate) source_path: PathBuf,
    pub(crate) source_name: String,
    pub(crate) source_absolute: PathBuf,
    pub(crate) layout: ContainerLayout,
    pub(crate) verification: VerifyResult,
    pub(crate) document: InspectDocument,
    pub(crate) spool: File,
    pub(crate) body_sha256: String,
}

pub(crate) struct PublishedCandidate {
    pub(crate) output_source: String,
    pub(crate) output_file_size: u64,
    pub(crate) output_sha256: String,
    pub(crate) writer: WriteSummary,
    pub(crate) verification: VerifyResult,
}

pub fn save_generated_hlt_as_new(
    source: &Path,
    document: &HmlDocument,
    dialect: GenerationDialect,
    payloads: &[GeneratedImagePayload],
    output: &Path,
    options: WriteOptions,
) -> Result<GeneratedHltSaveResult, CoreError> {
    save_generated_hlt_with_profile_as_new(
        source,
        document,
        dialect,
        &crate::generation_profile(dialect),
        payloads,
        output,
        options,
    )
}

/// [`save_generated_hlt_as_new`] 에 프로필을 직접 준다(원본 틀 프로필 — 의견서).
pub fn save_generated_hlt_with_profile_as_new(
    source: &Path,
    document: &HmlDocument,
    dialect: GenerationDialect,
    profile: &crate::GenerationProfile,
    payloads: &[GeneratedImagePayload],
    output: &Path,
    options: WriteOptions,
) -> Result<GeneratedHltSaveResult, CoreError> {
    let input_source = file_name_or_default(source, "input");
    let source_absolute =
        fs::canonicalize(source).map_err(|error| io_error(&error, "open", &input_source))?;
    let output_source = file_name_or_default(output, "output.HLT");
    let output_absolute = normalized_output_path(output, &output_source)?;
    if paths_equal(&source_absolute, &output_absolute) {
        return Err(path_error(
            ErrorCode::OutputSameAsInput,
            "output",
            &output_source,
        ));
    }
    ensure_output_missing(&output_absolute, &output_source)?;
    check_generation_cancelled(&options.verify, &output_source)?;
    let input_file_size = fs::metadata(&source_absolute)
        .map_err(|error| io_error(&error, "source_metadata", &input_source))?
        .len();
    let input_sha256 = sha256_path(&source_absolute, &input_source)?;
    if input_sha256 != document.source_sha256 {
        return Err(revision_error(
            &input_source,
            &document.source_sha256,
            &input_sha256,
        ));
    }

    let generated =
        crate::generate_hlt_document_with_profile(document, dialect, profile, payloads)?;
    check_generation_cancelled(&options.verify, &output_source)?;
    let output_parent = output_absolute
        .parent()
        .ok_or_else(|| path_error(ErrorCode::PathNotAllowed, "output", &output_source))?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".seoan-generated-")
        .suffix(".HLT.tmp")
        .tempfile_in(output_parent)
        .map_err(|error| io_error(&error, "temp_create", &output_source))?;
    let writer = write_hlt(
        &generated.header,
        &mut Cursor::new(&generated.inflated),
        temporary.as_file_mut(),
    )?;
    temporary
        .as_file_mut()
        .flush()
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|error| io_error(&error, "temp_sync", &output_source))?;
    let mut verification = verify_path(temporary.path(), clone_verify_options(&options.verify))?;
    let inspected = inspect_path(
        temporary.path(),
        InspectOptions {
            max_inflated_bytes: options.verify.max_inflated_bytes,
            ..InspectOptions::default()
        },
    )?;
    validate_generated_summary(&generated, document, &inspected.summary)?;
    let source_sha_after = sha256_path(&source_absolute, &input_source)?;
    if source_sha_after != input_sha256 {
        return Err(revision_error(
            &input_source,
            &input_sha256,
            &source_sha_after,
        ));
    }
    check_generation_cancelled(&options.verify, &output_source)?;
    let output_file_size = fs::metadata(temporary.path())
        .map_err(|error| io_error(&error, "temp_metadata", &output_source))?
        .len();
    let output_sha256 = verification.input_sha256.clone();
    ensure_output_missing(&output_absolute, &output_source)?;
    publish_noclobber(temporary.path(), &output_absolute, &output_source)?;
    drop(temporary);
    verification.source = output_source.clone();
    Ok(GeneratedHltSaveResult {
        input_source,
        output_source,
        input_file_size,
        output_file_size,
        input_sha256,
        output_sha256,
        writer,
        verification,
        summary: inspected.summary,
    })
}

/// 통특기 서식(`.frm`)으로 「새 문서」를 만들어 새 `.HLT` 로 쓴다.
/// 서식은 읽기만 하고(변환 전후 sha256 대조), 출력은 창 증명까지 마친 뒤에만 덮어쓰지 않고 게시한다.
/// 쓴 파일을 다시 풀어 변환 결과와 바이트가 같은지도 본다 — 라이터 결함이 조용히 지나가지 않게.
pub fn save_new_document_as_new(
    template: &Path,
    output: &Path,
    options: WriteOptions,
) -> Result<GeneratedHltSaveResult, CoreError> {
    save_new_document_with_kind(template, output, options, None)
}

/// 통특기 의견서 서식(`Epasl/FRM/Opinion.frm` · 빈 문서에 【의견내용】 하나) 그대로.
const OPINION_FORM: &[u8] = include_bytes!("../assets/forms/Opinion.frm");

/// 새 의견서: 의견서 서식으로 새 문서를 굽고 문서 종류를 7(의견서)로 둔다.
/// 서식으로 막 연 새 문서는 종류 0 이다(통특기도 같다 — 제출 변환이 종류 목록을 띄운다).
/// 7 이어야 편집기가 의견서 틀로 고치고 제출파일이 `OpinionDOC` 로 나간다(실물 의견서 = 7).
pub fn save_new_opinion_as_new(
    output: &Path,
    options: WriteOptions,
) -> Result<GeneratedHltSaveResult, CoreError> {
    let output_source = file_name_or_default(output, "output.HLT");
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut form = tempfile::Builder::new()
        .prefix(".seoan-opinion-form-")
        .suffix(".frm")
        .tempfile_in(parent)
        .map_err(|error| io_error(&error, "temp_create", &output_source))?;
    form.write_all(OPINION_FORM)
        .and_then(|_| form.flush())
        .map_err(|error| io_error(&error, "temp_write", &output_source))?;
    save_new_document_with_kind(form.path(), output, options, Some(7))
}

fn save_new_document_with_kind(
    template: &Path,
    output: &Path,
    options: WriteOptions,
    kind: Option<u16>,
) -> Result<GeneratedHltSaveResult, CoreError> {
    let input_source = file_name_or_default(template, "input");
    let source_absolute =
        fs::canonicalize(template).map_err(|error| io_error(&error, "open", &input_source))?;
    let output_source = file_name_or_default(output, "output.HLT");
    let output_absolute = normalized_output_path(output, &output_source)?;
    if paths_equal(&source_absolute, &output_absolute) {
        return Err(path_error(
            ErrorCode::OutputSameAsInput,
            "output",
            &output_source,
        ));
    }
    ensure_output_missing(&output_absolute, &output_source)?;
    let input_file_size = fs::metadata(&source_absolute)
        .map_err(|error| io_error(&error, "source_metadata", &input_source))?
        .len();
    let input_sha256 = sha256_path(&source_absolute, &input_source)?;

    let mut document = new_document_from_template(&source_absolute)?;
    // 문서 종류 = 헤더 0x20 하위 15비트(맨 윗비트는 압축 표지라 둔다).
    if let Some(kind) = kind {
        let raw = u16::from_le_bytes([document.header[0x20], document.header[0x21]]);
        document.header[0x20..0x22].copy_from_slice(&((raw & 0x8000) | kind).to_le_bytes());
    }
    check_generation_cancelled(&options.verify, &output_source)?;
    let output_parent = output_absolute
        .parent()
        .ok_or_else(|| path_error(ErrorCode::PathNotAllowed, "output", &output_source))?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".seoan-new-document-")
        .suffix(".HLT.tmp")
        .tempfile_in(output_parent)
        .map_err(|error| io_error(&error, "temp_create", &output_source))?;
    let writer = write_hlt(
        &document.header,
        &mut Cursor::new(&document.inflated),
        temporary.as_file_mut(),
    )?;
    temporary
        .as_file_mut()
        .flush()
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|error| io_error(&error, "temp_sync", &output_source))?;
    let mut verification = verify_path(temporary.path(), clone_verify_options(&options.verify))?;
    let inspected = inspect_path(
        temporary.path(),
        InspectOptions {
            max_inflated_bytes: options.verify.max_inflated_bytes,
            ..InspectOptions::default()
        },
    )?;
    let (written_header, written) = inflate_hlt(temporary.path())?;
    if written_header != document.header || written != document.inflated {
        return Err(CoreError::new(
            ErrorCode::VerificationFailed,
            "new_document_roundtrip",
            false,
        ));
    }
    let source_sha_after = sha256_path(&source_absolute, &input_source)?;
    if source_sha_after != input_sha256 {
        return Err(revision_error(
            &input_source,
            &input_sha256,
            &source_sha_after,
        ));
    }
    check_generation_cancelled(&options.verify, &output_source)?;
    let output_file_size = fs::metadata(temporary.path())
        .map_err(|error| io_error(&error, "temp_metadata", &output_source))?
        .len();
    let output_sha256 = verification.input_sha256.clone();
    ensure_output_missing(&output_absolute, &output_source)?;
    publish_noclobber(temporary.path(), &output_absolute, &output_source)?;
    drop(temporary);
    verification.source = output_source.clone();
    Ok(GeneratedHltSaveResult {
        input_source,
        output_source,
        input_file_size,
        output_file_size,
        input_sha256,
        output_sha256,
        writer,
        verification,
        summary: inspected.summary,
    })
}

pub fn repack_as_new(
    source: &Path,
    output: &Path,
    options: WriteOptions,
) -> Result<SaveAsNewResult, CoreError> {
    let mut input = open_inflated_source(source, &options.verify)?;
    let published = publish_inflated_as_new(
        &mut input,
        output,
        &options.verify,
        |source, output_document, output_body_sha256| {
            let parity = StructuralParity {
                header_equal: true,
                inflated_equal: source.body_sha256 == output_body_sha256,
                summary_equal: source.document.summary == output_document.summary,
                fixed_blocks_equal: source.document.summary.fixed_blocks
                    == output_document.summary.fixed_blocks,
            };
            if parity.inflated_equal && parity.summary_equal && parity.fixed_blocks_equal {
                Ok(())
            } else {
                Err(parity_error(&source.source_name, &parity))
            }
        },
    )?;

    Ok(SaveAsNewResult {
        input_source: input.source_name,
        output_source: published.output_source,
        input_file_size: input.verification.file_size,
        output_file_size: published.output_file_size,
        input_sha256: input.verification.input_sha256,
        output_sha256: published.output_sha256,
        writer: published.writer,
        verification: published.verification,
        structural_parity: StructuralParity {
            header_equal: true,
            inflated_equal: true,
            summary_equal: true,
            fixed_blocks_equal: true,
        },
    })
}

pub(crate) fn open_inflated_source(
    source: &Path,
    options: &VerifyOptions,
) -> Result<InflatedSource, CoreError> {
    let source_name = file_name_or_default(source, "input.HLT");
    let source_absolute =
        fs::canonicalize(source).map_err(|error| io_error(&error, "open", &source_name))?;
    let verification = verify_path(&source_absolute, clone_verify_options(options))?;
    let document = inspect_path(
        &source_absolute,
        InspectOptions {
            max_inflated_bytes: options.max_inflated_bytes,
            ..InspectOptions::default()
        },
    )?;
    let mut source_file =
        File::open(&source_absolute).map_err(|error| io_error(&error, "open", &source_name))?;
    let layout = read_layout(&mut source_file, &source_name)?;
    let mut spool =
        tempfile::tempfile().map_err(|error| io_error(&error, "spool_create", &source_name))?;
    inflate_raw_to_writer(&mut source_file, &layout, &mut spool, options)?;
    spool
        .flush()
        .and_then(|_| spool.sync_all())
        .map_err(|error| io_error(&error, "spool_sync", &source_name))?;
    let body_sha256 = sha256_reader(&mut spool, "spool_hash", &source_name)?;
    spool
        .seek(SeekFrom::Start(0))
        .map_err(|error| io_error(&error, "spool_rewind", &source_name))?;
    Ok(InflatedSource {
        source_path: source_absolute.clone(),
        source_name,
        source_absolute,
        layout,
        verification,
        document,
        spool,
        body_sha256,
    })
}

pub(crate) fn publish_inflated_as_new<F>(
    source: &mut InflatedSource,
    output: &Path,
    options: &VerifyOptions,
    validate: F,
) -> Result<PublishedCandidate, CoreError>
where
    F: FnOnce(&InflatedSource, &InspectDocument, &str) -> Result<(), CoreError>,
{
    let output_source = file_name_or_default(output, "output.HLT");
    let output_absolute = normalized_output_path(output, &output_source)?;
    if paths_equal(&source.source_absolute, &output_absolute) {
        return Err(path_error(
            ErrorCode::OutputSameAsInput,
            "output",
            &output_source,
        ));
    }
    ensure_output_missing(&output_absolute, &output_source)?;
    source
        .spool
        .seek(SeekFrom::Start(0))
        .map_err(|error| io_error(&error, "spool_rewind", &source.source_name))?;

    let output_parent = output_absolute
        .parent()
        .ok_or_else(|| path_error(ErrorCode::PathNotAllowed, "output", &output_source))?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".seoan-")
        .suffix(".HLT.tmp")
        .tempfile_in(output_parent)
        .map_err(|error| io_error(&error, "temp_create", &output_source))?;
    let writer = write_hlt(
        &source.layout.header,
        &mut source.spool,
        temporary.as_file_mut(),
    )?;
    temporary
        .as_file_mut()
        .flush()
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|error| io_error(&error, "temp_sync", &output_source))?;

    let mut output_verify = verify_path(temporary.path(), clone_verify_options(options))?;
    let output_document = inspect_path(
        temporary.path(),
        InspectOptions {
            max_inflated_bytes: options.max_inflated_bytes,
            ..InspectOptions::default()
        },
    )?;
    let mut temporary_reader = File::open(temporary.path())
        .map_err(|error| io_error(&error, "temp_open", &output_source))?;
    let output_layout = read_layout(&mut temporary_reader, &output_source)?;
    if source.layout.header != output_layout.header {
        let mut error = CoreError::new(ErrorCode::VerificationFailed, "header_parity", false);
        error.source = safe_source(&output_source);
        return Err(error);
    }
    let mut output_body_hasher = HashWriter::default();
    inflate_raw_to_writer(
        &mut temporary_reader,
        &output_layout,
        &mut output_body_hasher,
        options,
    )?;
    let output_body_sha256 = output_body_hasher.finish();
    validate(source, &output_document, &output_body_sha256)?;

    let source_sha_after = sha256_path(&source.source_path, &source.source_name)?;
    if source_sha_after != source.verification.input_sha256 {
        let mut error = CoreError::new(ErrorCode::RevisionConflict, "source_changed", false);
        error.source = safe_source(&source.source_name);
        error.expected = Some(source.verification.input_sha256.clone());
        error.actual = Some(source_sha_after);
        return Err(error);
    }

    let output_file_size = fs::metadata(temporary.path())
        .map_err(|error| io_error(&error, "temp_metadata", &output_source))?
        .len();
    output_verify.source = output_source.clone();
    let output_sha256 = output_verify.input_sha256.clone();
    ensure_output_missing(&output_absolute, &output_source)?;
    publish_noclobber(temporary.path(), &output_absolute, &output_source)?;
    drop(temporary);

    Ok(PublishedCandidate {
        output_source,
        output_file_size,
        output_sha256,
        writer,
        verification: output_verify,
    })
}

pub(crate) fn rewrite_inflated_spool(
    source: &mut InflatedSource,
    patches: &mut [InflatedPatch],
) -> Result<RewriteSummary, CoreError> {
    let (rewritten, summary) = rewrite_spool_file(&mut source.spool, patches)?;
    source.spool = rewritten;
    Ok(summary)
}

fn rewrite_spool_file(
    source: &mut File,
    patches: &mut [InflatedPatch],
) -> Result<(File, RewriteSummary), CoreError> {
    let input_bytes = source
        .seek(SeekFrom::End(0))
        .map_err(|error| rewrite_io_error(&error, "edit_patch_layout"))?;
    patches.sort_by_key(|patch| (patch.start_byte, patch.end_byte));
    let mut previous_end = 0_u64;
    let mut output_bytes = input_bytes;
    for patch in patches.iter() {
        if patch.start_byte < previous_end
            || patch.start_byte > patch.end_byte
            || patch.end_byte > input_bytes
        {
            return Err(CoreError::new(
                ErrorCode::StructureInvalid,
                "edit_patch_layout",
                false,
            ));
        }
        let removed = patch.end_byte - patch.start_byte;
        let added = u64::try_from(patch.replacement.len())
            .map_err(|_| CoreError::new(ErrorCode::ResourceTooLarge, "edit_patch_layout", false))?;
        output_bytes = output_bytes
            .checked_sub(removed)
            .and_then(|value| value.checked_add(added))
            .ok_or_else(|| {
                CoreError::new(ErrorCode::ResourceTooLarge, "edit_patch_layout", false)
            })?;
        previous_end = patch.end_byte;
    }
    let delta = i128::from(output_bytes) - i128::from(input_bytes);
    let delta_bytes = i64::try_from(delta)
        .map_err(|_| CoreError::new(ErrorCode::ResourceTooLarge, "edit_patch_layout", false))?;

    source
        .seek(SeekFrom::Start(0))
        .map_err(|error| rewrite_io_error(&error, "edit_patch_copy"))?;
    let mut output =
        tempfile::tempfile().map_err(|error| rewrite_io_error(&error, "edit_patch_create"))?;
    let mut cursor = 0_u64;
    for patch in patches.iter() {
        copy_exact_bytes(
            source,
            &mut output,
            patch.start_byte - cursor,
            "edit_patch_copy",
        )?;
        output
            .write_all(&patch.replacement)
            .map_err(|error| rewrite_io_error(&error, "edit_patch_write"))?;
        source
            .seek(SeekFrom::Start(patch.end_byte))
            .map_err(|error| rewrite_io_error(&error, "edit_patch_copy"))?;
        cursor = patch.end_byte;
    }
    copy_exact_bytes(source, &mut output, input_bytes - cursor, "edit_patch_copy")?;
    output
        .flush()
        .and_then(|_| output.sync_all())
        .map_err(|error| rewrite_io_error(&error, "edit_patch_sync"))?;
    output
        .seek(SeekFrom::Start(0))
        .map_err(|error| rewrite_io_error(&error, "edit_patch_rewind"))?;
    source
        .seek(SeekFrom::Start(0))
        .map_err(|error| rewrite_io_error(&error, "edit_patch_rewind"))?;
    Ok((
        output,
        RewriteSummary {
            input_bytes,
            output_bytes,
            delta_bytes,
        },
    ))
}

fn copy_exact_bytes(
    source: &mut File,
    output: &mut File,
    mut remaining: u64,
    phase: &str,
) -> Result<(), CoreError> {
    let mut buffer = [0_u8; HASH_BUFFER_SIZE];
    while remaining > 0 {
        let want =
            usize::try_from(remaining.min(HASH_BUFFER_SIZE as u64)).expect("bounded rewrite read");
        source
            .read_exact(&mut buffer[..want])
            .map_err(|error| rewrite_io_error(&error, phase))?;
        output
            .write_all(&buffer[..want])
            .map_err(|error| rewrite_io_error(&error, phase))?;
        remaining -= want as u64;
    }
    Ok(())
}

fn rewrite_io_error(error: &std::io::Error, phase: &str) -> CoreError {
    let code = if error.kind() == std::io::ErrorKind::PermissionDenied {
        ErrorCode::PermissionDenied
    } else if error.kind() == std::io::ErrorKind::StorageFull {
        ErrorCode::DiskFull
    } else {
        ErrorCode::IoError
    };
    CoreError::new(code, phase, false)
}

fn normalized_output_path(output: &Path, source: &str) -> Result<PathBuf, CoreError> {
    let file_name = output
        .file_name()
        .ok_or_else(|| path_error(ErrorCode::PathNotAllowed, "output", source))?;
    // `Path::new("x.HLT").parent()` 는 `Some("")` 이라 canonicalize 가 NotFound 로 죽는다(파일명만 준 출력).
    let parent = match output.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let parent = fs::canonicalize(parent).map_err(|error| io_error(&error, "output", source))?;
    Ok(parent.join(file_name))
}

fn ensure_output_missing(output: &Path, source: &str) -> Result<(), CoreError> {
    match fs::symlink_metadata(output) {
        Ok(_) => Err(path_error(ErrorCode::OutputExists, "output", source)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(&error, "output", source)),
    }
}

fn publish_noclobber(temp: &Path, output: &Path, source: &str) -> Result<(), CoreError> {
    fs::hard_link(temp, output).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            path_error(ErrorCode::OutputExists, "publish", source)
        } else {
            io_error(&error, "publish", source)
        }
    })
}

fn clone_verify_options(options: &VerifyOptions) -> VerifyOptions {
    VerifyOptions {
        request_id: options.request_id.clone(),
        max_inflated_bytes: options.max_inflated_bytes,
        cancellation: options.cancellation.clone(),
        progress: options.progress.clone(),
    }
}

fn sha256_path(path: &Path, source: &str) -> Result<String, CoreError> {
    let mut file = File::open(path).map_err(|error| io_error(&error, "hash", source))?;
    sha256_reader(&mut file, "hash", source)
}

fn sha256_reader<R: Read + Seek>(
    reader: &mut R,
    phase: &str,
    source: &str,
) -> Result<String, CoreError> {
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|error| io_error(&error, phase, source))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; HASH_BUFFER_SIZE];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| io_error(&error, phase, source))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:X}", hasher.finalize()))
}

#[derive(Default)]
struct HashWriter(Sha256);

impl HashWriter {
    fn finish(self) -> String {
        format!("{:X}", self.0.finalize())
    }
}

impl Write for HashWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0.update(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn validate_generated_summary(
    generated: &crate::GeneratedHltDocument,
    document: &HmlDocument,
    summary: &InspectSummary,
) -> Result<(), CoreError> {
    // 프로필 헤더가 정한 종류(국내 `0x8000 + 140` = 32908 · PCT 32881 · 원본 틀이면 원본 값)가 그대로 읽혀야 한다.
    let document_type = u32::from_le_bytes(
        generated.header[0x20..0x24]
            .try_into()
            .expect("header document type"),
    );
    let expected = [
        ("document_type", u64::from(document_type)),
        ("inflated_bytes", generated.inflated.len() as u64),
        ("sections", u64::from(generated.section_count)),
        ("lines", u64::from(generated.line_count)),
        ("spans", u64::from(generated.span_count)),
        ("tables", u64::from(generated.table_count)),
        // 세로 병합에 덮인 이어짐 셀은 판독 쪽에서 주 셀의 rowspan 으로 접힌다.
        // 그래서 세는 대상은 이어짐을 뺀 칸 수다. 병합 없는 표본만 있던 동안은
        // 두 값이 같아서 이 차이가 드러나지 않았다.
        (
            "cells",
            document
                .cells
                .iter()
                .filter(|cell| !cell.merge_continuation)
                .count() as u64,
        ),
        ("images", u64::from(generated.image_count)),
    ];
    let actual = [
        ("document_type", u64::from(summary.document_type_raw)),
        ("inflated_bytes", summary.inflated_bytes),
        ("sections", summary.section_count),
        ("lines", summary.line_count),
        ("spans", summary.span_count),
        ("tables", summary.table_count),
        ("cells", summary.cell_count),
        ("images", summary.image_count),
    ];
    for ((name, expected), (_, actual)) in expected.into_iter().zip(actual) {
        if expected != actual {
            let mut error =
                CoreError::new(ErrorCode::VerificationFailed, "generated_structure", false);
            error.details.insert("field".into(), name.into());
            error.expected = Some(expected.to_string());
            error.actual = Some(actual.to_string());
            return Err(error);
        }
    }
    Ok(())
}

fn check_generation_cancelled(options: &VerifyOptions, source: &str) -> Result<(), CoreError> {
    if options.cancellation.is_cancelled() {
        let mut error = CoreError::new(ErrorCode::Cancelled, "generate_save", false);
        error.source = safe_source(source);
        Err(error)
    } else {
        Ok(())
    }
}

fn revision_error(source: &str, expected: &str, actual: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::RevisionConflict, "source_changed", false);
    error.source = safe_source(source);
    error.expected = Some(expected.to_string());
    error.actual = Some(actual.to_string());
    error
}

fn parity_error(source: &str, parity: &StructuralParity) -> CoreError {
    let mut error = CoreError::new(ErrorCode::VerificationFailed, "repack_parity", false);
    error.source = safe_source(source);
    error
        .details
        .insert("header_equal".to_string(), parity.header_equal.to_string());
    error.details.insert(
        "inflated_equal".to_string(),
        parity.inflated_equal.to_string(),
    );
    error.details.insert(
        "summary_equal".to_string(),
        parity.summary_equal.to_string(),
    );
    error.details.insert(
        "fixed_blocks_equal".to_string(),
        parity.fixed_blocks_equal.to_string(),
    );
    error
}

fn path_error(code: ErrorCode, phase: &str, source: &str) -> CoreError {
    let mut error = CoreError::new(code, phase, false);
    error.source = safe_source(source);
    error
}

fn file_name_or_default(path: &Path, default: &str) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(default)
        .to_string()
}

#[cfg(windows)]
fn paths_equal(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

#[cfg(not(windows))]
fn paths_equal(left: &Path, right: &Path) -> bool {
    left == right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_race_preserves_the_file_that_won() {
        let directory = tempfile::tempdir().unwrap();
        let temporary = directory.path().join("temporary.HLT");
        let output = directory.path().join("winner.HLT");
        fs::write(&temporary, b"generated candidate").unwrap();
        fs::write(&output, b"race winner").unwrap();

        let error = publish_noclobber(&temporary, &output, "winner.HLT").unwrap_err();

        assert_eq!(error.code, ErrorCode::OutputExists);
        assert_eq!(fs::read(&output).unwrap(), b"race winner");
        assert_eq!(fs::read(&temporary).unwrap(), b"generated candidate");
    }

    #[test]
    fn rewrite_multiple_inflated_regions_in_one_bounded_pass() {
        let mut source = literal_spool(b"0123456789");
        let mut patches = [
            InflatedPatch {
                start_byte: 2,
                end_byte: 4,
                replacement: b"ABC".to_vec(),
            },
            InflatedPatch {
                start_byte: 7,
                end_byte: 9,
                replacement: b"Z".to_vec(),
            },
        ];

        let (mut output, summary) = rewrite_spool_file(&mut source, &mut patches).unwrap();

        assert_eq!(read_all(&mut output), b"01ABC456Z9");
        assert_eq!(summary.input_bytes, 10);
        assert_eq!(summary.output_bytes, 10);
        assert_eq!(summary.delta_bytes, 0);
        assert_eq!(read_all(&mut source), b"0123456789");
    }

    #[test]
    fn rewrite_reports_net_growth_and_shrinkage() {
        let cases = [
            (0, 1, b"grow".as_slice(), 3_i64, b"grow123".as_slice()),
            (1, 3, b"".as_slice(), -2_i64, b"03".as_slice()),
        ];
        for (start, end, replacement, delta, expected) in cases {
            let mut source = literal_spool(b"0123");
            let mut patches = [InflatedPatch {
                start_byte: start,
                end_byte: end,
                replacement: replacement.to_vec(),
            }];

            let (mut output, summary) = rewrite_spool_file(&mut source, &mut patches).unwrap();

            assert_eq!(read_all(&mut output), expected);
            assert_eq!(summary.delta_bytes, delta);
        }
    }

    #[test]
    fn rewrite_rejects_overlap_or_eof_overflow_before_changing_source() {
        let cases = [
            vec![
                InflatedPatch {
                    start_byte: 2,
                    end_byte: 5,
                    replacement: vec![],
                },
                InflatedPatch {
                    start_byte: 4,
                    end_byte: 7,
                    replacement: vec![],
                },
            ],
            vec![InflatedPatch {
                start_byte: 8,
                end_byte: 11,
                replacement: vec![],
            }],
        ];
        for mut patches in cases {
            let mut source = literal_spool(b"0123456789");

            let error = rewrite_spool_file(&mut source, &mut patches).unwrap_err();

            assert_eq!(error.code, ErrorCode::StructureInvalid);
            assert_eq!(error.phase, "edit_patch_layout");
            assert_eq!(read_all(&mut source), b"0123456789");
        }
    }

    fn literal_spool(bytes: &[u8]) -> File {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(bytes).unwrap();
        file.flush().unwrap();
        file
    }

    fn read_all(file: &mut File) -> Vec<u8> {
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut output = Vec::new();
        file.read_to_end(&mut output).unwrap();
        output
    }
}
