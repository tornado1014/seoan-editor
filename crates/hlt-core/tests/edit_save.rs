mod support;

use std::{fs, io::Read, path::Path};

use flate2::read::DeflateDecoder;
use hlt_core::{
    ErrorCode, ExactTextEditRequest, HLT_HEADER_LEN, ParagraphTextEdit, VerifyOptions,
    WriteOptions, document_revision, inspect_path, read_layout, save_exact_text_as_new,
    verify_path,
};
use sha2::{Digest, Sha256};

#[test]
fn saves_one_exact_length_paragraph_edit_and_preserves_every_other_structure() {
    let source = support::pct_table_fixture();
    let output_dir = tempfile::tempdir().unwrap();
    let output = output_dir.path().join("PCT_exact_edit.HLT");
    let source_sha_before = file_sha256(source.path());
    let input_document = inspect_path(source.path(), Default::default()).unwrap();
    let request = valid_request(source.path());
    let target = input_document
        .paragraphs
        .iter()
        .find(|paragraph| paragraph.id == "paragraph:3")
        .unwrap();
    let target_line = input_document
        .lines
        .iter()
        .find(|line| line.id == target.line_ids[0])
        .unwrap()
        .source_range;
    let (input_header, input_body) = decoded_parts(source.path());

    let result =
        save_exact_text_as_new(source.path(), &output, &request, WriteOptions::default()).unwrap();

    let output_document = inspect_path(&output, Default::default()).unwrap();
    let (output_header, output_body) = decoded_parts(&output);
    let differences: Vec<_> = input_body
        .iter()
        .zip(&output_body)
        .enumerate()
        .filter_map(|(index, (left, right))| (left != right).then_some(index as u64))
        .collect();
    assert_eq!(input_header, output_header);
    assert_eq!(input_body.len(), output_body.len());
    assert!(!differences.is_empty());
    assert!(
        differences
            .iter()
            .all(|offset| { *offset >= target_line.start_byte && *offset < target_line.end_byte })
    );
    assert_eq!(
        output_document
            .paragraphs
            .iter()
            .find(|paragraph| paragraph.id == "paragraph:3")
            .unwrap()
            .text,
        "구성품 분석 결과"
    );
    for input in &input_document.paragraphs {
        if input.id == "paragraph:3" {
            continue;
        }
        let output_paragraph = output_document
            .paragraphs
            .iter()
            .find(|paragraph| paragraph.id == input.id)
            .unwrap();
        assert_eq!(output_paragraph.text, input.text, "{}", input.id);
    }
    assert_eq!(input_document.summary, output_document.summary);
    assert!(result.structure.header_equal);
    assert!(result.structure.inflated_size_equal);
    assert!(result.structure.summary_equal);
    assert!(result.structure.fixed_blocks_equal);
    assert!(result.structure.target_paragraphs_equal);
    assert!(result.structure.non_target_paragraphs_equal);
    assert!(result.structure.body_changed);
    assert_ne!(result.base_revision, result.next_revision);
    assert_eq!(file_sha256(source.path()), source_sha_before);
    assert_eq!(result.output_sha256, file_sha256(&output));
    assert!(
        verify_path(&output, VerifyOptions::default())
            .unwrap()
            .window_ok
    );
}

#[test]
fn zero_ready_same_path_and_existing_output_fail_without_clobbering() {
    let source = support::pct_table_fixture();
    let source_sha_before = file_sha256(source.path());
    let mut no_ready = valid_request(source.path());
    no_ready.operations[0].expected_text_sha256 = text_sha256("wrong source");
    let output_dir = tempfile::tempdir().unwrap();
    let absent = output_dir.path().join("absent.HLT");

    let error = save_exact_text_as_new(source.path(), &absent, &no_ready, WriteOptions::default())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ConversionFailed);
    assert!(!absent.exists());

    let request = valid_request(source.path());
    let error = save_exact_text_as_new(
        source.path(),
        source.path(),
        &request,
        WriteOptions::default(),
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::OutputSameAsInput);

    let existing = output_dir.path().join("existing.HLT");
    fs::write(&existing, b"protected").unwrap();
    let error = save_exact_text_as_new(source.path(), &existing, &request, WriteOptions::default())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::OutputExists);
    assert_eq!(fs::read(existing).unwrap(), b"protected");
    assert_eq!(file_sha256(source.path()), source_sha_before);
}

fn valid_request(source: &Path) -> ExactTextEditRequest {
    let verified = verify_path(source, VerifyOptions::default()).unwrap();
    ExactTextEditRequest {
        expected_revision: document_revision(&verified.input_sha256).unwrap(),
        operations: vec![ParagraphTextEdit {
            paragraph_id: "paragraph:3".to_string(),
            expected_text_sha256: text_sha256("구성품 시험 결과"),
            replacement: "구성품 분석 결과".to_string(),
        }],
    }
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

fn text_sha256(text: &str) -> String {
    format!("{:X}", Sha256::digest(text.as_bytes()))
}
