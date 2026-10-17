use hlt_core::{
    ErrorCode, GenerationDialect, WriteOptions, inspect_hml_path, inspect_path,
    save_generated_hlt_as_new,
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
};

#[test]
fn saves_an_absent_hlt_then_verifies_and_reinspects_it() {
    let source_file = long_fixture();
    let source = source_file.path();
    let document = inspect_hml_path(source).unwrap();
    let output_dir = tempfile::tempdir().unwrap();
    let output = output_dir.path().join("generated-counterpart.HLT");
    let source_sha_before = file_sha256(source);

    let result = save_generated_hlt_as_new(
        source,
        &document,
        GenerationDialect::PatentSpecificationKo,
        &[],
        &output,
        WriteOptions::default(),
    )
    .unwrap();

    assert_eq!(file_sha256(source), source_sha_before);
    assert_eq!(result.input_sha256, source_sha_before);
    assert_eq!(result.output_sha256, file_sha256(&output));
    assert!(result.verification.footer_crc_ok);
    assert!(result.verification.footer_size_ok);
    assert!(result.verification.window_ok);
    assert_eq!(result.summary.document_type_raw, 32_908);
    assert_eq!(result.summary.section_count, 2);
    assert!(result.summary.line_count > 200);
    assert_eq!(result.summary.span_count, result.summary.line_count);
    assert_eq!(result.summary.paragraph_wipoid_nonzero, 0);
    assert_eq!(result.summary.paragraph_wipoid_distinct, 0);
    assert_eq!(
        inspect_path(&output, Default::default()).unwrap().summary,
        result.summary
    );
}

fn long_fixture() -> tempfile::NamedTempFile {
    let source = fs::read_to_string(fixture_path()).unwrap();
    let source = source.replace("plain <b>bold</b> text.", &"A".repeat(20_000));
    let mut file = tempfile::Builder::new().suffix(".hml").tempfile().unwrap();
    file.write_all(source.as_bytes()).unwrap();
    file.flush().unwrap();
    file
}

#[test]
fn refuses_same_existing_or_precancelled_outputs_without_changing_them() {
    let source = fixture_path();
    let document = inspect_hml_path(&source).unwrap();
    let source_sha_before = file_sha256(&source);

    let same_error = save_generated_hlt_as_new(
        &source,
        &document,
        GenerationDialect::PatentSpecificationKo,
        &[],
        &source,
        WriteOptions::default(),
    )
    .unwrap_err();
    assert_eq!(same_error.code, ErrorCode::OutputSameAsInput);
    assert_eq!(file_sha256(&source), source_sha_before);

    let output_dir = tempfile::tempdir().unwrap();
    let existing = output_dir.path().join("existing.HLT");
    fs::write(&existing, b"protected").unwrap();
    let existing_sha = file_sha256(&existing);
    let existing_error = save_generated_hlt_as_new(
        &source,
        &document,
        GenerationDialect::PatentSpecificationKo,
        &[],
        &existing,
        WriteOptions::default(),
    )
    .unwrap_err();
    assert_eq!(existing_error.code, ErrorCode::OutputExists);
    assert_eq!(file_sha256(&existing), existing_sha);

    let cancelled = output_dir.path().join("cancelled.HLT");
    let options = WriteOptions::default();
    options.verify.cancellation.cancel();
    let cancel_error = save_generated_hlt_as_new(
        &source,
        &document,
        GenerationDialect::PatentSpecificationKo,
        &[],
        &cancelled,
        options,
    )
    .unwrap_err();
    assert_eq!(cancel_error.code, ErrorCode::Cancelled);
    assert!(!cancelled.exists());
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/generate_counterpart.hml")
}

fn file_sha256(path: &std::path::Path) -> String {
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
