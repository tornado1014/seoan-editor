mod support;

use std::{fs, io::Read, path::Path};

use flate2::read::DeflateDecoder;
use hlt_core::{
    ErrorCode, HLT_HEADER_LEN, WriteOptions, inspect_path, read_layout, repack_as_new, verify_path,
};
use sha2::{Digest, Sha256};

#[test]
fn repacks_the_pct_table_fixture_without_changing_header_body_or_structure() {
    let source = support::pct_table_fixture();
    let output_dir = tempfile::tempdir().unwrap();
    let output = output_dir.path().join("pct_table_repacked.HLT");
    let source_sha_before = file_sha256(source.path());
    let (source_header, source_body) = decoded_parts(source.path());
    let source_document = inspect_path(source.path(), Default::default()).unwrap();

    let result = repack_as_new(source.path(), &output, WriteOptions::default()).unwrap();

    let (output_header, output_body) = decoded_parts(&output);
    let output_document = inspect_path(&output, Default::default()).unwrap();
    let output_verify = verify_path(&output, Default::default()).unwrap();
    assert_eq!(source_header, output_header);
    assert_eq!(source_body, output_body);
    assert_eq!(source_document.summary, output_document.summary);
    assert_eq!(source_document.summary.section_count, 3);
    assert_eq!(source_document.summary.line_count, 29);
    assert_eq!(source_document.summary.span_count, 29);
    assert_eq!(source_document.summary.table_count, 1);
    assert_eq!(source_document.summary.cell_count, 16);
    assert!(output_verify.window_ok);
    assert!(result.structural_parity.header_equal);
    assert!(result.structural_parity.inflated_equal);
    assert!(result.structural_parity.summary_equal);
    assert!(result.structural_parity.fixed_blocks_equal);
    assert_eq!(result.input_sha256, source_sha_before);
    assert_eq!(result.output_sha256, file_sha256(&output));
    assert_eq!(file_sha256(source.path()), source_sha_before);
}

#[test]
fn refuses_to_use_the_input_path_as_output() {
    let source = support::pct_table_fixture();
    let source_sha_before = file_sha256(source.path());

    let error = repack_as_new(source.path(), source.path(), WriteOptions::default()).unwrap_err();

    assert_eq!(error.code, ErrorCode::OutputSameAsInput);
    assert_eq!(file_sha256(source.path()), source_sha_before);
}

#[test]
fn refuses_an_existing_output_without_changing_it() {
    let source = support::pct_table_fixture();
    let output_dir = tempfile::tempdir().unwrap();
    let output = output_dir.path().join("existing.HLT");
    fs::write(&output, b"protected existing output").unwrap();
    let output_sha_before = file_sha256(&output);

    let error = repack_as_new(source.path(), &output, WriteOptions::default()).unwrap_err();

    assert_eq!(error.code, ErrorCode::OutputExists);
    assert_eq!(file_sha256(&output), output_sha_before);
}

#[test]
fn invalid_source_does_not_create_an_output() {
    let mut source = tempfile::Builder::new().suffix(".HLT").tempfile().unwrap();
    std::io::Write::write_all(&mut source, b"not an HLT").unwrap();
    let output_dir = tempfile::tempdir().unwrap();
    let output = output_dir.path().join("must-not-exist.HLT");

    let error = repack_as_new(source.path(), &output, WriteOptions::default()).unwrap_err();

    assert_eq!(error.code, ErrorCode::HeaderTruncated);
    assert!(!output.exists());
}

fn decoded_parts(path: &Path) -> ([u8; HLT_HEADER_LEN as usize], Vec<u8>) {
    let bytes = fs::read(path).unwrap();
    let mut cursor = std::io::Cursor::new(&bytes);
    let layout = read_layout(&mut cursor, "fixture.HLT").unwrap();
    let mut decoder =
        DeflateDecoder::new(&bytes[layout.stream_start as usize..layout.stream_end as usize]);
    let mut body = Vec::new();
    decoder.read_to_end(&mut body).unwrap();
    (layout.header, body)
}

fn file_sha256(path: &Path) -> String {
    let mut file = fs::File::open(path).unwrap();
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).unwrap();
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    format!("{:X}", hasher.finalize())
}
