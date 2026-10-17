use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    CoreError, ErrorCode, FirstBlockEvidence, ProgressEvent, ProgressTotals, VerifyOptions,
    error::{io_error, safe_source},
    inflate_raw_stream, inspect_first_block, max_backreference_distance, read_layout,
};

const HASH_BUFFER_SIZE: usize = 64 * 1024;

/// 통특기 리더의 LZ 창(바이트).
const HLT_WINDOW: u32 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowProof {
    /// 첫 블록이 동적이고 거리 부호가 24개 이하 — 4KB 창 인코더만 낼 수 있는 모양.
    FirstBlock,
    /// 스트림을 끝까지 풀어 잰 가장 먼 역참조가 4096 이하.
    Decoded { max_distance: u32 },
    /// 무압축 저장(헤더 0x17 = 0) — deflate 가 없으니 창도 없다.
    Stored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyResult {
    pub source: String,
    #[serde(with = "crate::serde_u64::as_string")]
    pub file_size: u64,
    pub input_sha256: String,
    pub header_ok: bool,
    pub magic_ok: bool,
    #[serde(with = "crate::serde_u64::as_string")]
    pub header_len: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub stream_start: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub stream_end: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub footer_offset: u64,
    #[serde(with = "crate::serde_u64::u32_as_string")]
    pub footer_crc: u32,
    #[serde(with = "crate::serde_u64::u32_as_string")]
    pub computed_crc: u32,
    #[serde(with = "crate::serde_u64::u32_as_string")]
    pub footer_size: u32,
    #[serde(with = "crate::serde_u64::as_string")]
    pub computed_size: u64,
    pub footer_crc_ok: bool,
    pub footer_size_ok: bool,
    pub inflate_ok: bool,
    #[serde(with = "crate::serde_u64::as_string")]
    pub inflated: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub compressed_bytes: u64,
    pub window_ok: bool,
    /// 창 증명을 무엇으로 했는가. 첫 블록 헤더로 못 하면 스트림을 풀어 역참조 거리를 잰다.
    pub window_proof: WindowProof,
    pub first_block: FirstBlockEvidence,
}

pub fn verify_path(path: &Path, options: VerifyOptions) -> Result<VerifyResult, CoreError> {
    let source = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("input.HLT")
        .to_string();
    let mut file = File::open(path).map_err(|error| io_error(&error, "open", &source))?;
    verify_reader(&mut file, &source, options)
}

pub fn verify_reader<R: Read + Seek>(
    reader: &mut R,
    source: &str,
    options: VerifyOptions,
) -> Result<VerifyResult, CoreError> {
    let original_position = reader
        .stream_position()
        .map_err(|error| io_error(&error, "open", source))?;
    let result = verify_reader_inner(reader, source, &options);
    let restored = reader
        .seek(SeekFrom::Start(original_position))
        .map_err(|error| io_error(&error, "open", source));

    match restored {
        Ok(_) => result,
        Err(error) => Err(error),
    }
}

fn verify_reader_inner<R: Read + Seek>(
    reader: &mut R,
    source: &str,
    options: &VerifyOptions,
) -> Result<VerifyResult, CoreError> {
    let source = safe_source(source).unwrap_or_default();
    let layout = read_layout(reader, &source)?;
    emit_progress(options, "header", 0, 0, 0.0);
    check_cancelled(options, &source, layout.stream_start)?;
    if crate::codec::is_stored(reader, &layout, options)
        .map_err(|error| attach_source(error, &source, layout.stream_start))?
    {
        return verify_stored(reader, &layout, &source, options);
    }

    if layout.footer_offset - layout.stream_start < 3 {
        return Err(verification_error(
            ErrorCode::DeflateInvalid,
            "window",
            &source,
            layout.stream_start,
        ));
    }
    reader
        .seek(SeekFrom::Start(layout.stream_start))
        .map_err(|error| io_error(&error, "window", &source))?;
    let mut prefix = [0_u8; 3];
    reader
        .read_exact(&mut prefix)
        .map_err(|error| io_error(&error, "window", &source))?;
    let first_block = inspect_first_block(&prefix)
        .map_err(|error| attach_source(error, &source, layout.stream_start))?;

    let summary = inflate_raw_stream(reader, &layout, options)
        .map_err(|error| attach_source(error, &source, layout.stream_start))?;
    check_cancelled(options, &source, summary.stream_end)?;

    if summary.crc32 != layout.footer.crc32 {
        let mut error = verification_error(
            ErrorCode::FooterCrcMismatch,
            "footer",
            &source,
            layout.footer_offset,
        );
        error.expected = Some(layout.footer.crc32.to_string());
        error.actual = Some(summary.crc32.to_string());
        return Err(error);
    }
    if summary.inflated != u64::from(layout.footer.inflated_size) {
        let mut error = verification_error(
            ErrorCode::FooterSizeMismatch,
            "footer",
            &source,
            layout.footer_offset + 4,
        );
        error.expected = Some(layout.footer.inflated_size.to_string());
        error.actual = Some(summary.inflated.to_string());
        return Err(error);
    }
    emit_progress(
        options,
        "footer",
        summary.compressed_bytes,
        summary.inflated,
        1.0,
    );

    let window_proof = if first_block.window_ok {
        WindowProof::FirstBlock
    } else {
        // ponytail: 압축 스트림을 통째로 메모리에 올린다. 헤더 증명이 실패한 파일(작은 문서·고정 블록)에서만 돈다.
        reader
            .seek(SeekFrom::Start(layout.stream_start))
            .map_err(|error| io_error(&error, "window", &source))?;
        let mut stream = vec![0_u8; (summary.stream_end - layout.stream_start) as usize];
        reader
            .read_exact(&mut stream)
            .map_err(|error| io_error(&error, "window", &source))?;
        let max_distance = max_backreference_distance(&stream)
            .map_err(|error| attach_source(error, &source, layout.stream_start))?;
        if max_distance > HLT_WINDOW {
            let mut error = verification_error(
                ErrorCode::WindowNotProven,
                "window",
                &source,
                layout.stream_start,
            );
            error.expected = Some(HLT_WINDOW.to_string());
            error.actual = Some(max_distance.to_string());
            return Err(error);
        }
        WindowProof::Decoded { max_distance }
    };

    let input_sha256 = compute_sha256(reader, &source, options)?;
    check_cancelled(options, &source, layout.footer_offset)?;
    emit_progress(
        options,
        "complete",
        summary.compressed_bytes,
        summary.inflated,
        1.0,
    );

    Ok(VerifyResult {
        source,
        file_size: layout.file_size,
        input_sha256,
        header_ok: true,
        magic_ok: true,
        header_len: crate::HLT_HEADER_LEN,
        stream_start: layout.stream_start,
        stream_end: summary.stream_end,
        footer_offset: layout.footer_offset,
        footer_crc: layout.footer.crc32,
        computed_crc: summary.crc32,
        footer_size: layout.footer.inflated_size,
        computed_size: summary.inflated,
        footer_crc_ok: true,
        footer_size_ok: true,
        inflate_ok: true,
        inflated: summary.inflated,
        compressed_bytes: summary.compressed_bytes,
        window_ok: true,
        window_proof,
        first_block,
    })
}

/// 무압축 저장 컨테이너. 꼬리가 없으므로 꼬리 칸은 계산값을 싣고(대조할 것이 없다) 첫 블록 증거는 「본문 전체 = 무압축 블록 하나」로 적는다.
fn verify_stored<R: Read + Seek>(
    reader: &mut R,
    layout: &crate::container::ContainerLayout,
    source: &str,
    options: &VerifyOptions,
) -> Result<VerifyResult, CoreError> {
    let summary = inflate_raw_stream(reader, layout, options)
        .map_err(|error| attach_source(error, source, layout.stream_start))?;
    let input_sha256 = compute_sha256(reader, source, options)?;
    emit_progress(
        options,
        "complete",
        summary.compressed_bytes,
        summary.inflated,
        1.0,
    );
    Ok(VerifyResult {
        source: source.to_string(),
        file_size: layout.file_size,
        input_sha256,
        header_ok: true,
        magic_ok: true,
        header_len: crate::HLT_HEADER_LEN,
        stream_start: layout.stream_start,
        stream_end: summary.stream_end,
        footer_offset: layout.file_size,
        footer_crc: summary.crc32,
        computed_crc: summary.crc32,
        footer_size: summary.inflated as u32,
        computed_size: summary.inflated,
        footer_crc_ok: true,
        footer_size_ok: true,
        inflate_ok: true,
        inflated: summary.inflated,
        compressed_bytes: summary.compressed_bytes,
        window_ok: true,
        window_proof: WindowProof::Stored,
        first_block: crate::codec::FirstBlockEvidence {
            final_block: true,
            kind: crate::codec::FirstBlockKind::Stored,
            hlit: None,
            hdist: None,
            hclen: None,
            window_ok: true,
        },
    })
}

fn compute_sha256<R: Read + Seek>(
    reader: &mut R,
    source: &str,
    options: &VerifyOptions,
) -> Result<String, CoreError> {
    let restore_position = reader
        .stream_position()
        .map_err(|error| io_error(&error, "hash", source))?;
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|error| io_error(&error, "hash", source))?;

    let result = (|| {
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; HASH_BUFFER_SIZE];
        loop {
            check_cancelled(options, source, reader.stream_position().unwrap_or(0))?;
            let read = reader
                .read(&mut buffer)
                .map_err(|error| io_error(&error, "hash", source))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        Ok(format!("{:X}", hasher.finalize()))
    })();

    reader
        .seek(SeekFrom::Start(restore_position))
        .map_err(|error| io_error(&error, "hash", source))?;
    result
}

fn check_cancelled(options: &VerifyOptions, source: &str, offset: u64) -> Result<(), CoreError> {
    if options.cancellation.is_cancelled() {
        Err(verification_error(
            ErrorCode::Cancelled,
            "verify",
            source,
            offset,
        ))
    } else {
        Ok(())
    }
}

fn emit_progress(
    options: &VerifyOptions,
    phase: &str,
    compressed_bytes: u64,
    inflated_bytes: u64,
    fraction: f64,
) {
    if let Some(progress) = &options.progress {
        progress(ProgressEvent {
            request_id: options.request_id.clone(),
            phase: phase.to_string(),
            totals: ProgressTotals {
                compressed_bytes,
                inflated_bytes,
                items: 0,
            },
            fraction: fraction.clamp(0.0, 1.0),
        });
    }
}

fn verification_error(code: ErrorCode, phase: &str, source: &str, offset: u64) -> CoreError {
    let mut error = CoreError::new(code, phase, false);
    error.source = safe_source(source);
    error.offset = Some(offset);
    error
}

fn attach_source(mut error: CoreError, source: &str, fallback_offset: u64) -> CoreError {
    if error.source.is_none() {
        error.source = safe_source(source);
    }
    if error.offset.is_none() {
        error.offset = Some(fallback_offset);
    }
    error
}
