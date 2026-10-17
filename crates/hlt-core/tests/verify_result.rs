mod support;

use std::{
    io::{self, Cursor, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use hlt_core::{ErrorCode, VerifyOptions, verify_path, verify_reader};

#[test]
fn verifies_recorded_python_positive_control() {
    let result = verify_path(
        &repo_path("work/pct_a/pct_a_full_v5.HLT"),
        Default::default(),
    )
    .unwrap();

    assert_eq!(result.file_size, 25_715_558);
    assert_eq!(
        result.input_sha256,
        "57640E1BBA733FCE23C141AC11E8133C06BDAD3667EC14590BCD3B5E51771581"
    );
    assert!(result.header_ok && result.magic_ok && result.inflate_ok);
    assert_eq!(result.footer_crc, 3_822_552_335);
    assert_eq!(result.footer_size, 469_761_837);
    assert_eq!(result.inflated, 469_761_837);
    assert!(result.footer_crc_ok && result.footer_size_ok && result.window_ok);
    assert_eq!(
        (
            result.first_block.hlit,
            result.first_block.hdist,
            result.first_block.hclen,
        ),
        (Some(286), Some(24), Some(17))
    );

    let json = serde_json::to_value(result).unwrap();
    assert_eq!(json["file_size"], "25715558");
    assert_eq!(json["footer_crc"], "3822552335");
    assert_eq!(json["inflated"], "469761837");
}

#[test]
fn reports_footer_mismatches_before_window_fingerprint_failure() {
    let mut crc_bytes = support::valid_stored_container(b"test");
    let footer_offset = crc_bytes.len() - 8;
    crc_bytes[footer_offset..footer_offset + 4].fill(0);
    let crc_error = verify_reader(
        &mut Cursor::new(crc_bytes),
        "crc.HLT",
        VerifyOptions::default(),
    )
    .unwrap_err();
    assert_eq!(crc_error.code, ErrorCode::FooterCrcMismatch);

    let mut size_bytes = support::valid_stored_container(b"test");
    let size_offset = size_bytes.len() - 4;
    size_bytes[size_offset..].fill(0);
    let size_error = verify_reader(
        &mut Cursor::new(size_bytes),
        "size.HLT",
        VerifyOptions::default(),
    )
    .unwrap_err();
    assert_eq!(size_error.code, ErrorCode::FooterSizeMismatch);
}

#[test]
fn reports_a_stream_without_an_end_marker_as_invalid() {
    let bytes = support::container_from_raw(b"", &[0x00]);

    let error = verify_reader(
        &mut Cursor::new(bytes),
        "invalid-stream.HLT",
        VerifyOptions::default(),
    )
    .unwrap_err();

    assert_eq!(error.code, ErrorCode::DeflateInvalid);
}

#[test]
fn sha_pass_uses_bounded_reads_and_restores_the_callers_seek_position() {
    let mut reader = TrackingReader::new(support::valid_dynamic_container());
    reader.seek(SeekFrom::Start(17)).unwrap();

    let result = verify_reader(&mut reader, "bounded.HLT", VerifyOptions::default()).unwrap();

    assert_eq!(result.inflated, 3_600);
    assert_eq!(result.compressed_bytes, 65);
    assert!(result.window_ok);
    assert!(reader.max_requested <= 64 * 1024);
    assert_eq!(reader.stream_position().unwrap(), 17);
}

fn repo_path(relative: impl AsRef<Path>) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

struct TrackingReader {
    inner: Cursor<Vec<u8>>,
    max_requested: usize,
}

impl TrackingReader {
    fn new(bytes: Vec<u8>) -> Self {
        Self {
            inner: Cursor::new(bytes),
            max_requested: 0,
        }
    }
}

impl Read for TrackingReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.max_requested = self.max_requested.max(buffer.len());
        self.inner.read(buffer)
    }
}

impl Seek for TrackingReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

#[test]
fn accepts_a_stored_container_without_a_footer() {
    // 통특기 무압축 저장(0x17 = 0): 머리 뒤 본문 원 바이트(늘 0 × 0x2c 로 시작) · 꼬리 없음.
    let mut bytes = support::valid_stored_container(b"");
    bytes.truncate(0x108);
    bytes[0x17] = 0;
    let mut body = vec![0_u8; 0x2c];
    body.extend_from_slice(b"stored body");
    bytes.extend_from_slice(&body);

    let result = verify_reader(
        &mut Cursor::new(bytes),
        "stored.HLT",
        VerifyOptions::default(),
    )
    .unwrap();

    assert_eq!(result.window_proof, hlt_core::WindowProof::Stored);
    assert_eq!(result.inflated, body.len() as u64);
    assert_eq!(result.computed_crc, crc32fast::hash(&body));
}
