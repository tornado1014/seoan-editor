use std::io::{self, Read, Write};

use flate2::{Compress, Compression, FlushCompress, Status};
use serde::{Deserialize, Serialize};

use crate::{CoreError, ErrorCode, HLT_HEADER_LEN, HLT_MAGIC};

const BUFFER_SIZE: usize = 64 * 1024;
const HLT_WINDOW_BITS: u8 = 12;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteSummary {
    #[serde(with = "crate::serde_u64::as_string")]
    pub compressed_bytes: u64,
    #[serde(with = "crate::serde_u64::as_string")]
    pub inflated_bytes: u64,
    #[serde(with = "crate::serde_u64::u32_as_string")]
    pub crc32: u32,
}

pub fn write_hlt<R: Read, W: Write>(
    header: &[u8; HLT_HEADER_LEN as usize],
    inflated: &mut R,
    output: &mut W,
) -> Result<WriteSummary, CoreError> {
    write_hlt_with_level(header, inflated, output, 6)
}

/// 압축 수준을 고르는 쓰기. 편집기 중간 판은 1(빠름) — 저장(통특기에 넘길 파일)은 늘 6 이다.
pub(crate) fn write_hlt_with_level<R: Read, W: Write>(
    header: &[u8; HLT_HEADER_LEN as usize],
    inflated: &mut R,
    output: &mut W,
    level: u32,
) -> Result<WriteSummary, CoreError> {
    if &header[..HLT_MAGIC.len()] != HLT_MAGIC {
        return Err(CoreError::new(
            ErrorCode::HeaderInvalid,
            "write_header",
            false,
        ));
    }
    output
        .write_all(header)
        .map_err(|error| write_io_error(&error, "write_header"))?;

    let mut compressor =
        Compress::new_with_window_bits(Compression::new(level), false, HLT_WINDOW_BITS);
    let mut input_buffer = [0_u8; BUFFER_SIZE];
    let mut output_buffer = [0_u8; BUFFER_SIZE];
    let mut crc = crc32fast::Hasher::new();
    let mut inflated_bytes = 0_u64;

    loop {
        let read = inflated
            .read(&mut input_buffer)
            .map_err(|error| write_io_error(&error, "read_body"))?;
        if read == 0 {
            break;
        }
        inflated_bytes = checked_inflated_size(inflated_bytes, read)?;
        crc.update(&input_buffer[..read]);

        let mut input_position = 0_usize;
        while input_position < read {
            let before_input = compressor.total_in();
            let before_output = compressor.total_out();
            let status = compressor
                .compress(
                    &input_buffer[input_position..read],
                    &mut output_buffer,
                    FlushCompress::None,
                )
                .map_err(|_| compression_error())?;
            let consumed = (compressor.total_in() - before_input) as usize;
            let produced = (compressor.total_out() - before_output) as usize;
            input_position += consumed;
            if produced != 0 {
                output
                    .write_all(&output_buffer[..produced])
                    .map_err(|error| write_io_error(&error, "write_stream"))?;
            }
            if status == Status::StreamEnd || (consumed == 0 && produced == 0) {
                return Err(compression_error());
            }
        }
    }

    loop {
        let before_output = compressor.total_out();
        let status = compressor
            .compress(&[], &mut output_buffer, FlushCompress::Finish)
            .map_err(|_| compression_error())?;
        let produced = (compressor.total_out() - before_output) as usize;
        if produced != 0 {
            output
                .write_all(&output_buffer[..produced])
                .map_err(|error| write_io_error(&error, "write_stream"))?;
        }
        if status == Status::StreamEnd {
            break;
        }
        if produced == 0 {
            return Err(compression_error());
        }
    }

    let crc32 = crc.finalize();
    let footer_size = u32::try_from(inflated_bytes).map_err(|_| size_limit_error())?;
    output
        .write_all(&crc32.to_le_bytes())
        .and_then(|_| output.write_all(&footer_size.to_le_bytes()))
        .map_err(|error| write_io_error(&error, "write_footer"))?;

    Ok(WriteSummary {
        compressed_bytes: compressor.total_out(),
        inflated_bytes,
        crc32,
    })
}

fn checked_inflated_size(current: u64, read: usize) -> Result<u64, CoreError> {
    let next = current
        .checked_add(read as u64)
        .ok_or_else(size_limit_error)?;
    if next > u64::from(u32::MAX) {
        return Err(size_limit_error());
    }
    Ok(next)
}

fn compression_error() -> CoreError {
    CoreError::new(ErrorCode::ConversionFailed, "compress", false)
}

fn size_limit_error() -> CoreError {
    let mut error = CoreError::new(ErrorCode::SizeLimitExceeded, "write_stream", false);
    error.expected = Some(u32::MAX.to_string());
    error
}

fn write_io_error(error: &io::Error, phase: &str) -> CoreError {
    let code = match error.kind() {
        io::ErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
        io::ErrorKind::StorageFull => ErrorCode::DiskFull,
        _ => ErrorCode::IoError,
    };
    let mut core_error = CoreError::new(code, phase, false);
    core_error
        .details
        .insert("io_kind".to_string(), format!("{:?}", error.kind()));
    core_error
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_an_inflated_size_that_cannot_fit_the_footer() {
        let error = checked_inflated_size(u64::from(u32::MAX), 1).unwrap_err();

        assert_eq!(error.code, ErrorCode::SizeLimitExceeded);
        assert_eq!(error.expected.as_deref(), Some("4294967295"));
    }
}
