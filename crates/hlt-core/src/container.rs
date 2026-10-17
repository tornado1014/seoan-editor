use std::io::{Read, Seek, SeekFrom};

use crate::{
    CoreError, ErrorCode,
    error::{io_error, safe_source},
};

pub const HLT_HEADER_LEN: u64 = 0x108;
pub const HLT_MAGIC: &[u8; 17] = b"HAN Lite Document";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Footer {
    pub crc32: u32,
    pub inflated_size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerLayout {
    pub file_size: u64,
    pub stream_start: u64,
    pub stream_end: u64,
    pub footer_offset: u64,
    pub header: [u8; HLT_HEADER_LEN as usize],
    pub footer: Footer,
}

pub fn read_layout<R: Read + Seek>(
    reader: &mut R,
    source: &str,
) -> Result<ContainerLayout, CoreError> {
    let file_size = reader
        .seek(SeekFrom::End(0))
        .map_err(|error| io_error(&error, "header", source))?;
    if file_size < HLT_HEADER_LEN {
        return Err(layout_error(
            ErrorCode::HeaderTruncated,
            "header",
            source,
            Some(file_size),
        ));
    }

    reader
        .seek(SeekFrom::Start(0))
        .map_err(|error| io_error(&error, "header", source))?;
    let mut header = [0_u8; HLT_HEADER_LEN as usize];
    reader
        .read_exact(&mut header)
        .map_err(|error| io_error(&error, "header", source))?;

    if &header[..HLT_MAGIC.len()] != HLT_MAGIC {
        return Err(layout_error(ErrorCode::NotHlt, "header", source, Some(0)));
    }

    if file_size < HLT_HEADER_LEN + 1 + 8 {
        return Err(layout_error(
            ErrorCode::FooterTruncated,
            "footer",
            source,
            Some(file_size),
        ));
    }

    let footer_offset = file_size - 8;
    reader
        .seek(SeekFrom::Start(footer_offset))
        .map_err(|error| io_error(&error, "footer", source))?;
    let mut footer_bytes = [0_u8; 8];
    reader
        .read_exact(&mut footer_bytes)
        .map_err(|error| io_error(&error, "footer", source))?;
    let footer = Footer {
        crc32: u32::from_le_bytes(footer_bytes[..4].try_into().expect("four-byte CRC slice")),
        inflated_size: u32::from_le_bytes(
            footer_bytes[4..].try_into().expect("four-byte size slice"),
        ),
    };

    Ok(ContainerLayout {
        file_size,
        stream_start: HLT_HEADER_LEN,
        stream_end: footer_offset,
        footer_offset,
        header,
        footer,
    })
}

fn layout_error(code: ErrorCode, phase: &str, source: &str, offset: Option<u64>) -> CoreError {
    let mut error = CoreError::new(code, phase, false);
    error.source = safe_source(source);
    error.offset = offset;
    error
}
