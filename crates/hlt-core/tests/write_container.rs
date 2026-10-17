mod support;

use std::io::{self, Cursor, Read, Write};

use flate2::{Decompress, FlushDecompress, Status};
use hlt_core::{ErrorCode, HLT_HEADER_LEN, HLT_MAGIC, inspect_first_block, read_layout, write_hlt};

use support::deflate_trace::{BlockKind, trace_raw};

#[test]
fn writes_a_roundtrippable_hlt_with_preserved_header_and_footer() {
    let header = distinctive_header();
    let body = writer_body_fixture();
    let mut output = Vec::new();

    let summary = write_hlt(
        &header,
        &mut ChunkedReader::new(Cursor::new(&body), 7),
        &mut output,
    )
    .unwrap();
    let mut cursor = Cursor::new(&output);
    let layout = read_layout(&mut cursor, "generated.HLT").unwrap();
    let compressed = &output[layout.stream_start as usize..layout.stream_end as usize];
    let trace = trace_raw(compressed).unwrap();

    assert_eq!(layout.header, header);
    assert_eq!(inflate_raw(compressed, body.len()).unwrap(), body);
    assert_eq!(layout.footer.crc32, crc32fast::hash(&body));
    assert_eq!(layout.footer.inflated_size, body.len() as u32);
    assert_eq!(summary.inflated_bytes, body.len() as u64);
    assert_eq!(summary.compressed_bytes, compressed.len() as u64);
    assert_eq!(summary.crc32, crc32fast::hash(&body));
    assert_eq!(trace.first_block_kind, BlockKind::Dynamic);
    assert!(trace.first_hdist <= 24, "trace={trace:?}");
    assert!(trace.max_distance <= 4096, "trace={trace:?}");
    assert!(inspect_first_block(compressed).unwrap().window_ok);
}

#[test]
fn preserves_cp949_nul_and_image_like_binary_bytes() {
    let header = distinctive_header();
    let mut body = [0xc7, 0xd1, 0xb1, 0xdb, 0x00, 0x5f, 0xff, 0x15].repeat(5000);
    body.extend(writer_body_fixture());
    let mut output = Vec::new();

    write_hlt(&header, &mut Cursor::new(&body), &mut output).unwrap();
    let mut cursor = Cursor::new(&output);
    let layout = read_layout(&mut cursor, "binary.HLT").unwrap();
    let compressed = &output[layout.stream_start as usize..layout.stream_end as usize];

    assert_eq!(inflate_raw(compressed, body.len()).unwrap(), body);
}

#[test]
fn rejects_an_invalid_header_before_writing_output() {
    let mut output = Vec::new();
    let error = write_hlt(
        &[0_u8; HLT_HEADER_LEN as usize],
        &mut Cursor::new(b"body"),
        &mut output,
    )
    .unwrap_err();

    assert_eq!(error.code, ErrorCode::HeaderInvalid);
    assert_eq!(error.phase, "write_header");
    assert!(output.is_empty());
}

#[test]
fn returns_an_error_instead_of_a_success_summary_when_output_fails() {
    let header = distinctive_header();
    let body = writer_body_fixture();
    let mut output = FailingWriter::new(HLT_HEADER_LEN as usize + 32);

    let error = write_hlt(&header, &mut Cursor::new(body), &mut output).unwrap_err();

    assert_eq!(error.code, ErrorCode::IoError);
    assert_eq!(error.phase, "write_stream");
}

fn distinctive_header() -> [u8; HLT_HEADER_LEN as usize] {
    let mut header = [0_u8; HLT_HEADER_LEN as usize];
    header[..HLT_MAGIC.len()].copy_from_slice(HLT_MAGIC);
    for (index, byte) in header[HLT_MAGIC.len()..].iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(37).wrapping_add(11);
    }
    header
}

fn writer_body_fixture() -> Vec<u8> {
    let mut state = 0x1234_5678_u32;
    let mut body = Vec::with_capacity(40_000);
    for _ in 0..32_768 {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let sample = (state >> 24) as u8;
        body.push(match sample {
            0..=127 => b'A',
            128..=191 => b'B' + (sample & 3),
            _ => sample,
        });
    }
    let marker: Vec<u8> = (0_u16..258)
        .map(|index| ((index * 73 + 41) % 251) as u8)
        .collect();
    body.extend_from_slice(&marker);
    body.extend(std::iter::repeat_n(0xfd, 2600));
    body.extend_from_slice(&marker);
    body.extend(b"dynamic-block-padding".repeat(200));
    body
}

fn inflate_raw(compressed: &[u8], expected_size: usize) -> Result<Vec<u8>, String> {
    let mut decoder = Decompress::new(false);
    let mut output = vec![0_u8; expected_size + 1];
    let status = decoder
        .decompress(compressed, &mut output, FlushDecompress::Finish)
        .map_err(|error| error.to_string())?;
    if status != Status::StreamEnd {
        return Err(format!("unexpected decoder status: {status:?}"));
    }
    output.truncate(decoder.total_out() as usize);
    Ok(output)
}

struct ChunkedReader<R> {
    inner: R,
    max_chunk: usize,
}

impl<R> ChunkedReader<R> {
    fn new(inner: R, max_chunk: usize) -> Self {
        Self { inner, max_chunk }
    }
}

impl<R: Read> Read for ChunkedReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let limit = buffer.len().min(self.max_chunk);
        self.inner.read(&mut buffer[..limit])
    }
}

struct FailingWriter {
    accepted: usize,
    fail_after: usize,
}

impl FailingWriter {
    fn new(fail_after: usize) -> Self {
        Self {
            accepted: 0,
            fail_after,
        }
    }
}

impl Write for FailingWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.accepted >= self.fail_after {
            return Err(io::Error::other("synthetic output failure"));
        }
        let accepted = buffer.len().min(self.fail_after - self.accepted);
        self.accepted += accepted;
        Ok(accepted)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
