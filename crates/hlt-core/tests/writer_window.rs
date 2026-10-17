mod support;

use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};

use support::deflate_trace::{BlockKind, trace_raw};

#[test]
fn rust_writer_uses_raw_twelve_bit_window_for_ascii_cp949_and_long_distance() {
    for body in [ascii_fixture(), cp949_fixture(), distance_fixture()] {
        let stream = compress_raw(&body, 12);
        let trace = trace_raw(&stream).unwrap();

        assert!(trace.max_distance <= 4096, "trace={trace:?}");
        assert_eq!(trace.output_bytes, body.len());
        assert_eq!(inflate_with_window(&stream, 12).unwrap(), body);
    }

    let distance_trace = trace_raw(&compress_raw(&distance_fixture(), 12)).unwrap();
    assert_eq!(distance_trace.first_block_kind, BlockKind::Dynamic);
    assert!(distance_trace.first_hdist <= 24, "trace={distance_trace:?}");
}

#[test]
fn compressor_window_argument_changes_the_maximum_emitted_distance() {
    let body = distance_fixture();
    let stream_11 = compress_raw(&body, 11);
    let stream_12 = compress_raw(&body, 12);
    let trace_11 = trace_raw(&stream_11).unwrap();
    let trace_12 = trace_raw(&stream_12).unwrap();

    assert!(trace_11.max_distance <= 2048, "trace={trace_11:?}");
    assert!(trace_12.max_distance > 2048, "trace={trace_12:?}");
    assert!(trace_12.max_distance <= 4096, "trace={trace_12:?}");
    assert_eq!(inflate_with_window(&stream_11, 15).unwrap(), body);
    assert_eq!(inflate_with_window(&stream_12, 15).unwrap(), body);
}

fn compress_raw(input: &[u8], window_bits: u8) -> Vec<u8> {
    let mut compressor = Compress::new_with_window_bits(Compression::new(6), false, window_bits);
    let mut output = vec![0_u8; input.len().saturating_mul(2).saturating_add(4096)];
    let status = compressor
        .compress(input, &mut output, FlushCompress::Finish)
        .unwrap();
    assert_eq!(status, Status::StreamEnd);
    output.truncate(compressor.total_out() as usize);
    output
}

fn inflate_with_window(stream: &[u8], window_bits: u8) -> Result<Vec<u8>, String> {
    let mut decompressor = Decompress::new_with_window_bits(false, window_bits);
    let mut decoded = Vec::new();
    let mut input_position = 0_usize;
    loop {
        let mut output = [0_u8; 257];
        let before_input = decompressor.total_in();
        let before_output = decompressor.total_out();
        let status = decompressor
            .decompress(
                &stream[input_position..],
                &mut output,
                FlushDecompress::Finish,
            )
            .map_err(|error| error.to_string())?;
        let consumed = (decompressor.total_in() - before_input) as usize;
        let produced = (decompressor.total_out() - before_output) as usize;
        input_position += consumed;
        decoded.extend_from_slice(&output[..produced]);
        if status == Status::StreamEnd {
            return Ok(decoded);
        }
        if consumed == 0 && produced == 0 {
            return Err(format!("decoder made no progress: {status:?}"));
        }
        if decoded.len() > 32 * 1024 * 1024 {
            return Err("decoder output limit exceeded".to_string());
        }
    }
}

fn ascii_fixture() -> Vec<u8> {
    b"HAN Lite Document paragraph table image section anchor ".repeat(300)
}

fn cp949_fixture() -> Vec<u8> {
    [0xc7, 0xd1, 0xb1, 0xdb, 0x20, 0xc6, 0xaf, 0xc7, 0xe3, 0x20].repeat(1400)
}

fn distance_fixture() -> Vec<u8> {
    let marker: Vec<u8> = (0_u16..258)
        .map(|index| ((index * 73 + 41) % 251) as u8)
        .collect();
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
    body.extend_from_slice(&marker);
    body.extend(std::iter::repeat_n(0xfd, 2600));
    body.extend_from_slice(&marker);
    body.extend(b"dynamic-block-padding".repeat(200));
    body
}
