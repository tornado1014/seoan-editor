mod support;

use std::io::Cursor;

use hlt_core::{ErrorCode, HLT_HEADER_LEN, HLT_MAGIC, inflate_raw_stream, read_layout};

#[test]
fn reads_exact_header_footer_and_stream_end() {
    let bytes = support::valid_stored_container(b"test");
    let mut cursor = Cursor::new(bytes);

    let layout = read_layout(&mut cursor, "small.HLT").unwrap();

    assert_eq!(layout.header[..HLT_MAGIC.len()], HLT_MAGIC[..]);
    assert_eq!(layout.stream_start, 0x108);
    assert_eq!(layout.stream_end, 0x108 + 9);
    assert_eq!(layout.footer_offset, 0x108 + 9);
    assert_eq!(layout.footer.crc32, 3_632_233_996);
    assert_eq!(layout.footer.inflated_size, 4);

    let summary = inflate_raw_stream(&mut cursor, &layout, &Default::default()).unwrap();
    assert_eq!(summary.stream_end, 0x108 + 9);
    assert_eq!(summary.compressed_bytes, 9);
    assert_eq!(summary.inflated, 4);
    assert_eq!(summary.crc32, 3_632_233_996);
}

#[test]
fn rejects_one_byte_after_raw_stream_as_trailing_data() {
    let bytes = support::valid_stored_container_with_stream_suffix(b"test", &[0x00]);
    let mut cursor = Cursor::new(bytes);
    let layout = read_layout(&mut cursor, "trailing.HLT").unwrap();

    let error = inflate_raw_stream(&mut cursor, &layout, &Default::default()).unwrap_err();

    assert_eq!(error.code, ErrorCode::DeflateTrailingData);
    assert_eq!(error.offset, Some(0x108 + 9));
}

#[test]
fn rejects_a_file_shorter_than_the_exact_header() {
    let mut cursor = Cursor::new(vec![0; HLT_HEADER_LEN as usize - 1]);

    let error = read_layout(&mut cursor, "short.HLT").unwrap_err();

    assert_eq!(error.code, ErrorCode::HeaderTruncated);
}

#[test]
fn validates_magic_before_footer_length_at_the_header_boundary() {
    let mut invalid = vec![0; HLT_HEADER_LEN as usize];
    invalid[..16].copy_from_slice(b"Not an HLT file!");
    let error = read_layout(&mut Cursor::new(invalid), "invalid.HLT").unwrap_err();
    assert_eq!(error.code, ErrorCode::NotHlt);

    let mut valid = vec![0; HLT_HEADER_LEN as usize];
    valid[..HLT_MAGIC.len()].copy_from_slice(HLT_MAGIC);
    let error = read_layout(&mut Cursor::new(valid), "no-footer.HLT").unwrap_err();
    assert_eq!(error.code, ErrorCode::FooterTruncated);
}
