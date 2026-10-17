//! 읽기(inspect)는 무결성(CRC·크기)만 요구하고 4KB 창 증명은 요구하지 않는다.
//! 창 증명은 「통특기가 다시 읽을 수 있게 썼는가」라 verify 와 저장 출력에만 남는다.
//! 통특기 자신의 서식 .frm 63개가 fixed 첫 블록이라 그 증명을 못 낸다.

mod support;

use hlt_core::{ErrorCode, InspectOptions, inspect_path};

fn inspect_bytes(bytes: &[u8]) -> ErrorCode {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), bytes).unwrap();
    match inspect_path(file.path(), InspectOptions::default()) {
        Ok(_) => panic!("a four-byte body is not a document"),
        Err(error) => error.code,
    }
}

#[test]
fn an_intact_stream_without_window_proof_is_read_past_the_gate() {
    let code = inspect_bytes(&support::valid_stored_container(b"test"));
    assert_ne!(code, ErrorCode::WindowNotProven);
}

#[test]
fn integrity_is_still_checked_before_the_window_is_waived() {
    let mut bytes = support::valid_stored_container(b"test");
    let crc = bytes.len() - 8;
    bytes[crc] ^= 0xff;
    assert_eq!(inspect_bytes(&bytes), ErrorCode::FooterCrcMismatch);
}

/// 통특기 서식의 변종 둘(무압축 · 꼬리 뒤 줄바꿈)은 verify 는 못 지나도 읽는다. 컨테이너 단계만 본다 —
/// 합성 본문은 문서가 아니라서 그 뒤 구조 판독에서 떨어진다.
#[test]
fn reads_past_line_breaks_after_the_footer_but_not_other_bytes() {
    // 실물 압축 파일은 압축 표지(0x17)가 1 이다.
    let mut container = support::valid_dynamic_container();
    container[0x17] = 1;
    container.extend_from_slice(b"\r\n\r\n");
    let code = inspect_bytes(&container);
    assert!(
        !matches!(
            code,
            ErrorCode::DeflateTrailingData | ErrorCode::FooterCrcMismatch
        ),
        "{code:?}"
    );

    let mut container = support::valid_dynamic_container();
    container[0x17] = 1;
    container.extend_from_slice(b"\r\nX");
    assert_eq!(inspect_bytes(&container), ErrorCode::DeflateTrailingData);
}

#[test]
fn reads_an_uncompressed_template_without_a_footer() {
    // 실물(PctPatE)처럼 헤더 뒤가 곧 해제본(0 으로 시작 — deflate 로는 저장 블록 길이 검사에서 깨진다)이고 꼬리가 없다.
    let mut container = support::valid_stored_container(b"");
    container.truncate(0x108);
    container[0x17] = 0;
    container.extend_from_slice(&[0; 64]);
    let code = inspect_bytes(&container);
    assert!(
        !matches!(
            code,
            ErrorCode::DeflateInvalid | ErrorCode::FooterCrcMismatch
        ),
        "{code:?}"
    );
}
