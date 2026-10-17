use std::{
    cmp,
    io::{self, Read, Seek, SeekFrom, Write},
};

use flate2::{Decompress, FlushDecompress, Status};
use serde::{Deserialize, Serialize};

use crate::{
    ContainerLayout, CoreError, ErrorCode, ProgressEvent, ProgressTotals, VerifyOptions,
    error::io_error,
};

const BUFFER_SIZE: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FirstBlockKind {
    Stored,
    Fixed,
    Dynamic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirstBlockEvidence {
    pub final_block: bool,
    pub kind: FirstBlockKind,
    pub hlit: Option<u16>,
    pub hdist: Option<u16>,
    pub hclen: Option<u8>,
    pub window_ok: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InflateSummary {
    pub stream_end: u64,
    pub compressed_bytes: u64,
    pub inflated: u64,
    pub crc32: u32,
}

pub fn inspect_first_block(prefix: &[u8]) -> Result<FirstBlockEvidence, CoreError> {
    let mut bits = BitReader::new(prefix);
    let final_block = bits.read(1)? != 0;
    let block_type = bits.read(2)?;

    let (kind, hlit, hdist, hclen, window_ok) = match block_type {
        0 => (FirstBlockKind::Stored, None, None, None, false),
        1 => (FirstBlockKind::Fixed, None, None, None, false),
        2 => {
            let hlit = bits.read(5)? + 257;
            let hdist = bits.read(5)? + 1;
            let hclen = bits.read(4)? as u8 + 4;
            (
                FirstBlockKind::Dynamic,
                Some(hlit),
                Some(hdist),
                Some(hclen),
                hdist <= 24,
            )
        }
        _ => return Err(first_block_error()),
    };

    Ok(FirstBlockEvidence {
        final_block,
        kind,
        hlit,
        hdist,
        hclen,
        window_ok,
    })
}

pub fn inflate_raw_stream<R: Read + Seek>(
    reader: &mut R,
    layout: &ContainerLayout,
    options: &VerifyOptions,
) -> Result<InflateSummary, CoreError> {
    inflate_raw_to_writer(reader, layout, &mut io::sink(), options)
}

/// 헤더 0x17 — 1 = deflate, 0 = 무압축 저장.
const COMPRESSED_FLAG: usize = 0x17;

/// 무압축 저장 컨테이너인가 — 무압축 저장: 머리 0x108 뒤 본문 원 바이트, 꼬리(CRC·크기) 없음.
/// 판정 = 플래그 0 **그리고** 본문 앞 0x2c 바이트가 모두 0 — 실물 본문은 압축 여부와 상관없이 늘 0 × 0x2c 로 시작한다(15/15).
/// 0 으로 시작하는 deflate 는 무압축 블록이고 LEN·NLEN 이 0·0 이라 결코 유효하지 않으니 둘은 겹치지 않는다.
/// 플래그만으로는 못 가른다: 합성 픽스처에 0x17 = 0 인 deflate 가 있다(P1).
pub(crate) fn is_stored<R: Read + Seek>(
    reader: &mut R,
    layout: &ContainerLayout,
    _options: &VerifyOptions,
) -> Result<bool, CoreError> {
    const STORED_PREFIX: u64 = 0x2c;
    if layout.header[COMPRESSED_FLAG] != 0 || layout.file_size < layout.stream_start + STORED_PREFIX
    {
        return Ok(false);
    }
    reader
        .seek(SeekFrom::Start(layout.stream_start))
        .map_err(|error| io_error(&error, "stored", ""))?;
    let mut prefix = [0_u8; STORED_PREFIX as usize];
    reader
        .read_exact(&mut prefix)
        .map_err(|error| io_error(&error, "stored", ""))?;
    Ok(prefix.iter().all(|byte| *byte == 0))
}

/// 본문을 푼다(deflate) — 무압축 저장이면 그대로 옮긴다. 엄격한 경로(verify · 저장 원본 열기)가 모두 여기를 거친다.
pub(crate) fn inflate_raw_to_writer<R: Read + Seek, W: Write>(
    reader: &mut R,
    layout: &ContainerLayout,
    output_writer: &mut W,
    options: &VerifyOptions,
) -> Result<InflateSummary, CoreError> {
    if is_stored(reader, layout, options)? {
        return copy_stored_to_writer(reader, layout, output_writer, options);
    }
    inflate_deflate_to_writer(reader, layout, output_writer, options)
}

fn copy_stored_to_writer<R: Read + Seek, W: Write>(
    reader: &mut R,
    layout: &ContainerLayout,
    output_writer: &mut W,
    options: &VerifyOptions,
) -> Result<InflateSummary, CoreError> {
    reader
        .seek(SeekFrom::Start(layout.stream_start))
        .map_err(|error| io_error(&error, "inflate", ""))?;
    let body = layout.file_size - layout.stream_start;
    if body > options.max_inflated_bytes {
        return Err(codec_error(
            ErrorCode::SizeLimitExceeded,
            layout,
            layout.stream_start,
        ));
    }
    let mut hasher = crc32fast::Hasher::new();
    let mut buffer = [0_u8; BUFFER_SIZE];
    let mut copied = 0_u64;
    while copied < body {
        check_cancelled(options, layout, layout.stream_start + copied)?;
        let want = cmp::min(BUFFER_SIZE as u64, body - copied) as usize;
        reader
            .read_exact(&mut buffer[..want])
            .map_err(|error| io_error(&error, "inflate", ""))?;
        hasher.update(&buffer[..want]);
        output_writer
            .write_all(&buffer[..want])
            .map_err(|error| io_error(&error, "inflate_output", ""))?;
        copied += want as u64;
        emit_progress(options, copied, copied, body);
    }
    Ok(InflateSummary {
        stream_end: layout.file_size,
        compressed_bytes: body,
        inflated: body,
        crc32: hasher.finalize(),
    })
}

fn inflate_deflate_to_writer<R: Read + Seek, W: Write>(
    reader: &mut R,
    layout: &ContainerLayout,
    output_writer: &mut W,
    options: &VerifyOptions,
) -> Result<InflateSummary, CoreError> {
    check_cancelled(options, layout, layout.stream_start)?;
    reader
        .seek(SeekFrom::Start(layout.stream_start))
        .map_err(|error| io_error(&error, "inflate", ""))?;

    let compressed_range = layout.footer_offset - layout.stream_start;
    let mut read_from_range = 0_u64;
    let mut input = [0_u8; BUFFER_SIZE];
    let mut input_len = 0_usize;
    let mut input_position = 0_usize;
    let mut output = [0_u8; BUFFER_SIZE];
    let mut decompressor = Decompress::new(false);
    let mut hasher = crc32fast::Hasher::new();
    let mut inflated = 0_u64;

    loop {
        check_cancelled(
            options,
            layout,
            layout.stream_start + decompressor.total_in(),
        )?;

        if input_position == input_len {
            if read_from_range == compressed_range {
                return Err(codec_error(
                    ErrorCode::DeflateInvalid,
                    layout,
                    layout.footer_offset,
                ));
            }
            let to_read = cmp::min(BUFFER_SIZE as u64, compressed_range - read_from_range) as usize;
            let read = reader
                .read(&mut input[..to_read])
                .map_err(|error| io_error(&error, "inflate", ""))?;
            if read == 0 {
                return Err(codec_error(
                    ErrorCode::DeflateInvalid,
                    layout,
                    layout.stream_start + read_from_range,
                ));
            }
            read_from_range += read as u64;
            input_len = read;
            input_position = 0;
            check_cancelled(
                options,
                layout,
                layout.stream_start + decompressor.total_in(),
            )?;
        }

        let before_input = decompressor.total_in();
        let before_output = decompressor.total_out();
        let status = decompressor
            .decompress(
                &input[input_position..input_len],
                &mut output,
                FlushDecompress::None,
            )
            .map_err(|_| {
                codec_error(
                    ErrorCode::DeflateInvalid,
                    layout,
                    layout.stream_start + decompressor.total_in(),
                )
            })?;
        let consumed = (decompressor.total_in() - before_input) as usize;
        let produced = (decompressor.total_out() - before_output) as usize;
        input_position += consumed;

        if produced > 0 {
            check_cancelled(
                options,
                layout,
                layout.stream_start + decompressor.total_in(),
            )?;
            let next_inflated = inflated.checked_add(produced as u64).ok_or_else(|| {
                codec_error(
                    ErrorCode::SizeLimitExceeded,
                    layout,
                    layout.stream_start + decompressor.total_in(),
                )
            })?;
            if next_inflated > options.max_inflated_bytes {
                return Err(codec_error(
                    ErrorCode::SizeLimitExceeded,
                    layout,
                    layout.stream_start + decompressor.total_in(),
                ));
            }
            hasher.update(&output[..produced]);
            output_writer
                .write_all(&output[..produced])
                .map_err(|error| io_error(&error, "inflate_output", ""))?;
            inflated = next_inflated;
            emit_progress(options, decompressor.total_in(), inflated, compressed_range);
        }

        if status == Status::StreamEnd {
            let stream_end = layout.stream_start + decompressor.total_in();
            if stream_end != layout.footer_offset {
                return Err(codec_error(
                    ErrorCode::DeflateTrailingData,
                    layout,
                    stream_end,
                ));
            }
            return Ok(InflateSummary {
                stream_end,
                compressed_bytes: decompressor.total_in(),
                inflated,
                crc32: hasher.finalize(),
            });
        }

        if consumed == 0 && produced == 0 && input_position < input_len {
            return Err(codec_error(
                ErrorCode::DeflateInvalid,
                layout,
                layout.stream_start + decompressor.total_in(),
            ));
        }
    }
}

fn check_cancelled(
    options: &VerifyOptions,
    layout: &ContainerLayout,
    offset: u64,
) -> Result<(), CoreError> {
    if options.cancellation.is_cancelled() {
        Err(codec_error(ErrorCode::Cancelled, layout, offset))
    } else {
        Ok(())
    }
}

fn emit_progress(
    options: &VerifyOptions,
    compressed_bytes: u64,
    inflated_bytes: u64,
    compressed_range: u64,
) {
    if let Some(progress) = &options.progress {
        let fraction = if compressed_range == 0 {
            1.0
        } else {
            (compressed_bytes as f64 / compressed_range as f64).clamp(0.0, 1.0)
        };
        progress(ProgressEvent {
            request_id: options.request_id.clone(),
            phase: "inflate".to_string(),
            totals: ProgressTotals {
                compressed_bytes,
                inflated_bytes,
                items: 0,
            },
            fraction,
        });
    }
}

fn codec_error(code: ErrorCode, layout: &ContainerLayout, offset: u64) -> CoreError {
    let mut error = CoreError::new(code, "inflate", false);
    error.offset = Some(offset);
    if code == ErrorCode::SizeLimitExceeded {
        error.expected = Some("max_inflated_bytes".to_string());
    }
    debug_assert!(offset >= layout.stream_start);
    error
}

struct BitReader<'a> {
    bytes: &'a [u8],
    bit_offset: usize,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            bit_offset: 0,
        }
    }

    fn align(&mut self) {
        self.bit_offset = self.bit_offset.div_ceil(8) * 8;
    }

    fn read(&mut self, count: usize) -> Result<u16, CoreError> {
        if self.bit_offset + count > self.bytes.len() * 8 {
            return Err(first_block_error());
        }

        let mut value = 0_u16;
        for output_bit in 0..count {
            let input_bit = self.bit_offset + output_bit;
            let bit = (self.bytes[input_bit / 8] >> (input_bit % 8)) & 1;
            value |= u16::from(bit) << output_bit;
        }
        self.bit_offset += count;
        Ok(value)
    }
}

fn first_block_error() -> CoreError {
    CoreError::new(ErrorCode::DeflateInvalid, "window", false)
}

/// 통특기 리더의 창(4KB)보다 먼 역참조가 없음을 스트림을 끝까지 풀어 증명한다.
/// 첫 블록 헤더 증명(`inspect_first_block`)은 고정·저장 블록에서 아무것도 말하지 못한다 — 통특기 자신의
/// 서식 63개와 작은 새 문서가 그렇다. 출력은 만들지 않고 거리만 센다(내용 무결성은 CRC·크기가 본다).
/// 돌려주는 값 = 가장 먼 역참조 거리(없으면 0).
pub fn max_backreference_distance(stream: &[u8]) -> Result<u32, CoreError> {
    const LENGTH_BASE: [u16; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
        131, 163, 195, 227, 258,
    ];
    const LENGTH_EXTRA: [u8; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];
    const DISTANCE_BASE: [u16; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
        2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    const DISTANCE_EXTRA: [u8; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
        13, 13,
    ];
    const CODE_LENGTH_ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let mut bits = BitReader::new(stream);
    let (mut produced, mut farthest) = (0_u64, 0_u32);
    loop {
        let last = bits.read(1)? != 0;
        match bits.read(2)? {
            0 => {
                bits.align();
                let length = bits.read(16)?;
                if bits.read(16)? != !length {
                    return Err(first_block_error());
                }
                bits.bit_offset += 8 * usize::from(length);
                if bits.bit_offset > stream.len() * 8 {
                    return Err(first_block_error());
                }
                produced += u64::from(length);
            }
            kind @ (1 | 2) => {
                let (literals, distances) = if kind == 1 {
                    let mut lengths = [8_u8; 288];
                    lengths[144..256].fill(9);
                    lengths[256..280].fill(7);
                    (Huffman::new(&lengths)?, Huffman::new(&[5; 30])?)
                } else {
                    let literal_count = usize::from(bits.read(5)?) + 257;
                    let distance_count = usize::from(bits.read(5)?) + 1;
                    let code_count = usize::from(bits.read(4)?) + 4;
                    let mut code_lengths = [0_u8; 19];
                    for &slot in &CODE_LENGTH_ORDER[..code_count] {
                        code_lengths[slot] = bits.read(3)? as u8;
                    }
                    let code = Huffman::new(&code_lengths)?;
                    let mut lengths = vec![0_u8; literal_count + distance_count];
                    let mut at = 0;
                    while at < lengths.len() {
                        let (value, repeat) = match code.decode(&mut bits)? {
                            symbol @ 0..=15 => (symbol as u8, 1),
                            16 if at > 0 => (lengths[at - 1], 3 + usize::from(bits.read(2)?)),
                            17 => (0, 3 + usize::from(bits.read(3)?)),
                            18 => (0, 11 + usize::from(bits.read(7)?)),
                            _ => return Err(first_block_error()),
                        };
                        let end = at + repeat;
                        lengths
                            .get_mut(at..end)
                            .ok_or_else(first_block_error)?
                            .fill(value);
                        at = end;
                    }
                    (
                        Huffman::new(&lengths[..literal_count])?,
                        Huffman::new(&lengths[literal_count..])?,
                    )
                };
                loop {
                    match literals.decode(&mut bits)? {
                        symbol @ 0..=255 => {
                            let _ = symbol;
                            produced += 1;
                        }
                        256 => break,
                        symbol @ 257..=285 => {
                            let index = usize::from(symbol - 257);
                            let length =
                                LENGTH_BASE[index] + bits.read(usize::from(LENGTH_EXTRA[index]))?;
                            let index = usize::from(distances.decode(&mut bits)?);
                            let (Some(base), Some(extra)) =
                                (DISTANCE_BASE.get(index), DISTANCE_EXTRA.get(index))
                            else {
                                return Err(first_block_error());
                            };
                            let distance =
                                u32::from(*base) + u32::from(bits.read(usize::from(*extra))?);
                            if u64::from(distance) > produced {
                                return Err(first_block_error());
                            }
                            farthest = farthest.max(distance);
                            produced += u64::from(length);
                        }
                        _ => return Err(first_block_error()),
                    }
                }
            }
            _ => return Err(first_block_error()),
        }
        if last {
            return Ok(farthest);
        }
    }
}

/// 정준 허프만 부호(RFC 1951 §3.2.2). 한 비트씩 읽어 길이별 첫 부호와 비교한다.
struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Self, CoreError> {
        let mut counts = [0_u16; 16];
        for &length in lengths {
            *counts
                .get_mut(usize::from(length))
                .ok_or_else(first_block_error)? += 1;
        }
        counts[0] = 0;
        let mut offsets = [0_u16; 16];
        for length in 1..15 {
            offsets[length + 1] = offsets[length] + counts[length];
        }
        let mut symbols = vec![0_u16; lengths.len()];
        for (symbol, &length) in lengths.iter().enumerate() {
            if length != 0 {
                let slot = &mut offsets[usize::from(length)];
                symbols[usize::from(*slot)] = symbol as u16;
                *slot += 1;
            }
        }
        Ok(Self { counts, symbols })
    }

    fn decode(&self, bits: &mut BitReader<'_>) -> Result<u16, CoreError> {
        let (mut code, mut first, mut index) = (0_i32, 0_i32, 0_i32);
        for length in 1..16 {
            code |= i32::from(bits.read(1)?);
            let count = i32::from(self.counts[length]);
            if code - first < count {
                return self
                    .symbols
                    .get((index + code - first) as usize)
                    .copied()
                    .ok_or_else(first_block_error);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(first_block_error())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::{Compression, write::DeflateEncoder};

    use super::*;

    fn deflate(data: &[u8], level: u32) -> Vec<u8> {
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(level));
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    /// 저장 블록 5000바이트 + 고정 블록(길이 3 · 거리 `distance` 역참조 + 끝).
    fn far_reference(distance: u32) -> Vec<u8> {
        let data: Vec<u8> = (0..5000_u32).map(|i| (i * 7 % 251) as u8).collect();
        let mut stream = vec![0x00];
        stream.extend_from_slice(&5000_u16.to_le_bytes());
        stream.extend_from_slice(&(!5000_u16).to_le_bytes());
        stream.extend_from_slice(&data);
        let (mut bits, mut value) = (0_usize, 0_u64);
        let mut push = |field: u64, width: usize| {
            value |= field << bits;
            bits += width;
        };
        let msb = |code: u64, width: usize| {
            (0..width).fold(0, |acc, i| acc | ((code >> i) & 1) << (width - 1 - i))
        };
        let index = DISTANCE_TABLE
            .iter()
            .rposition(|base| u32::from(*base) <= distance)
            .unwrap();
        push(1, 1);
        push(1, 2);
        push(msb(1, 7), 7);
        push(msb(index as u64, 5), 5);
        push(
            u64::from(distance - u32::from(DISTANCE_TABLE[index])),
            usize::from(DISTANCE_BITS[index]),
        );
        push(0, 7);
        stream.extend_from_slice(&value.to_le_bytes()[..bits.div_ceil(8)]);
        stream
    }
    const DISTANCE_TABLE: [u16; 26] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
        2049, 3073, 4097, 6145,
    ];
    const DISTANCE_BITS: [u8; 26] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11,
    ];

    #[test]
    fn measures_back_references_across_stored_fixed_and_dynamic_blocks() {
        assert_eq!(
            max_backreference_distance(&far_reference(4096)).unwrap(),
            4096
        );
        assert_eq!(
            max_backreference_distance(&far_reference(4097)).unwrap(),
            4097
        );
        assert_eq!(
            max_backreference_distance(&far_reference(5000)).unwrap(),
            5000
        );
        // 저장 블록만: 역참조 없음.
        assert_eq!(max_backreference_distance(&deflate(b"abc", 0)).unwrap(), 0);
        // 짧은 반복 → zlib 이 고정 블록을 고른다.
        let fixed = deflate(&b"ab".repeat(20), 6);
        assert_eq!(
            inspect_first_block(&fixed).unwrap().kind,
            FirstBlockKind::Fixed
        );
        assert_eq!(max_backreference_distance(&fixed).unwrap(), 2);
        // 동적 블록: 32KB 창 인코더로 8000 떨어진 반복.
        let mut far: Vec<u8> = (0..8000_u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
            .collect();
        far.extend_from_within(..300);
        let dynamic = deflate(&far, 9);
        assert_eq!(
            inspect_first_block(&dynamic).unwrap().kind,
            FirstBlockKind::Dynamic
        );
        assert!(max_backreference_distance(&dynamic).unwrap() >= 8000);
    }

    #[test]
    fn refuses_a_reference_before_the_start_of_output() {
        let mut stream = far_reference(5000);
        // 저장 블록 길이를 4000 으로 줄이면 거리 5000 은 출력 앞을 가리킨다.
        stream[1..3].copy_from_slice(&4000_u16.to_le_bytes());
        stream[3..5].copy_from_slice(&(!4000_u16).to_le_bytes());
        stream.drain(5 + 4000..5 + 5000);
        assert!(max_backreference_distance(&stream).is_err());
    }
}
