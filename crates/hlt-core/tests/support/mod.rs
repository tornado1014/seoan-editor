#![allow(dead_code)]

use std::{
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};

use flate2::read::DeflateDecoder;
use hlt_core::{read_layout, write_hlt};

pub mod deflate_trace;

const HEADER_LEN: usize = 0x108;
const MAGIC: &[u8; 17] = b"HAN Lite Document";

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 고객 사건에서 온 실물 픽스처 — 저장소에 없다(.gitignore · 2026-10-16 히스토리에서 걷어냈다). 로컬 디스크에만 있어
/// 컴파일 때 박지 않고 실행 때 읽는다(없으면 그 테스트만 붉다 · 새로 받은 저장소도 컴파일된다).
fn local_fixture(relative: &str) -> String {
    let path = repo_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "로컬 실물 픽스처가 없다(저장소 밖 자료): {}",
            path.display()
        )
    })
}

pub fn pct_table_fixture() -> tempfile::NamedTempFile {
    let encoded = include_str!("../../../../fixtures/phase2/pct_table_probe.HLT.hex");
    let digits: Vec<u8> = encoded
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    assert_eq!(digits.len() % 2, 0);
    let bytes: Vec<u8> = digits
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).expect("fixture hex");
            u8::from_str_radix(text, 16).expect("fixture byte")
        })
        .collect();
    let mut file = tempfile::Builder::new().suffix(".HLT").tempfile().unwrap();
    std::io::Write::write_all(&mut file, &bytes).unwrap();
    file
}

pub fn formatted_style_fixture() -> tempfile::NamedTempFile {
    let encoded = include_str!("../../../../fixtures/phase2/HML_formatted_style_probe.HLT.hex");
    let digits: Vec<u8> = encoded
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    assert_eq!(digits.len() % 2, 0);
    let bytes: Vec<u8> = digits
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).expect("fixture hex");
            u8::from_str_radix(text, 16).expect("fixture byte")
        })
        .collect();
    let mut file = tempfile::Builder::new().suffix(".HLT").tempfile().unwrap();
    std::io::Write::write_all(&mut file, &bytes).unwrap();
    file
}

pub fn formatted_style_fixture_with_attr(code: u32, attr: u32) -> tempfile::NamedTempFile {
    assert!(
        code < 24,
        "fixture helper only patches character-shape slots before the image"
    );
    let source = formatted_style_fixture();
    let bytes = std::fs::read(source.path()).unwrap();
    let mut input = Cursor::new(&bytes);
    let layout = read_layout(&mut input, "HML_formatted_style_probe.HLT").unwrap();
    let mut decoder =
        DeflateDecoder::new(&bytes[layout.stream_start as usize..layout.stream_end as usize]);
    let mut body = Vec::new();
    decoder.read_to_end(&mut body).unwrap();
    let object_offset = 514 + code as usize * 100;
    body[object_offset..object_offset + 4].copy_from_slice(&attr.to_le_bytes());
    let mut output = tempfile::Builder::new().suffix(".HLT").tempfile().unwrap();
    write_hlt(&layout.header, &mut Cursor::new(body), &mut output).unwrap();
    output.flush().unwrap();
    output
}

pub fn pct_multiline_fixture() -> tempfile::NamedTempFile {
    let source = pct_table_fixture();
    let document = hlt_core::inspect_path(source.path(), Default::default()).unwrap();
    let paragraph = document
        .paragraphs
        .iter()
        .find(|paragraph| paragraph.id == "paragraph:4")
        .unwrap();
    let bytes = std::fs::read(source.path()).unwrap();
    let mut input = Cursor::new(&bytes);
    let layout = read_layout(&mut input, "pct_table_probe.HLT").unwrap();
    let mut decoder =
        DeflateDecoder::new(&bytes[layout.stream_start as usize..layout.stream_end as usize]);
    let mut body = Vec::new();
    decoder.read_to_end(&mut body).unwrap();
    for id in &paragraph.line_ids {
        let line = document.lines.iter().find(|line| &line.id == id).unwrap();
        for byte in
            &mut body[line.source_range.start_byte as usize..line.source_range.end_byte as usize]
        {
            if matches!(*byte, 0x12 | 0x13) {
                *byte = b'x';
            }
        }
    }
    let mut output = tempfile::Builder::new().suffix(".HLT").tempfile().unwrap();
    write_hlt(&layout.header, &mut Cursor::new(body), &mut output).unwrap();
    output.flush().unwrap();
    output
}

pub fn valid_stored_container(body: &[u8]) -> Vec<u8> {
    valid_stored_container_with_stream_suffix(body, &[])
}

pub fn valid_stored_container_with_stream_suffix(body: &[u8], suffix: &[u8]) -> Vec<u8> {
    assert!(body.len() <= u16::MAX as usize);
    let len = body.len() as u16;
    let mut bytes = vec![0; HEADER_LEN];
    bytes[..MAGIC.len()].copy_from_slice(MAGIC);
    bytes.push(0x01);
    bytes.extend_from_slice(&len.to_le_bytes());
    bytes.extend_from_slice(&(!len).to_le_bytes());
    bytes.extend_from_slice(body);
    bytes.extend_from_slice(suffix);
    bytes.extend_from_slice(&crc32fast::hash(body).to_le_bytes());
    bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
    bytes
}

pub fn valid_dynamic_container() -> Vec<u8> {
    let body = b"abcdefghijklmnopqrstuvwxyz0123456789".repeat(100);
    let stream = [
        0xed, 0xca, 0xc7, 0x11, 0x80, 0x20, 0x00, 0x00, 0xc1, 0x96, 0x8c, 0xa8, 0xe5, 0x10, 0x54,
        0xcc, 0x60, 0xc6, 0xea, 0xed, 0x83, 0xb9, 0x7d, 0xaf, 0x54, 0xda, 0xb4, 0x5d, 0x6f, 0x87,
        0x71, 0x9a, 0x97, 0x75, 0x73, 0x7e, 0x3f, 0xce, 0xeb, 0x7e, 0xde, 0xf0, 0x25, 0x69, 0x96,
        0x17, 0xa5, 0xa8, 0xea, 0x46, 0x72, 0x38, 0x1c, 0x0e, 0x87, 0xc3, 0xe1, 0x70, 0x38, 0x1c,
        0x0e, 0xc7, 0xc6, 0x76, 0x7e,
    ];
    container_from_raw(&body, &stream)
}

pub fn container_from_raw(body: &[u8], stream: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; HEADER_LEN];
    bytes[..MAGIC.len()].copy_from_slice(MAGIC);
    bytes.extend_from_slice(stream);
    bytes.extend_from_slice(&crc32fast::hash(body).to_le_bytes());
    bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
    bytes
}

/// 절 표제(【】)가 하나도 없는 실물 의견서. 절 테이블이 비어 있고 본문이
/// ASCII 대괄호 `[의견내용]` 으로 시작해서 【 스캔이 본문 한글 사이로 빠진다.
/// 출처 `opinion_b_의견서.HLT`, SHA-256
/// B4A054F470CB94B5EB72AC2C88EC73FD7DEC4CCF2B27A65E81EC3439EC28EB0B.
pub fn opinion_without_section_table_fixture() -> tempfile::NamedTempFile {
    let encoded = local_fixture("fixtures/opinion/OA_opinion_no_section_table.HLT.hex");
    let digits: Vec<u8> = encoded
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    assert_eq!(digits.len() % 2, 0);
    let bytes: Vec<u8> = digits
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).expect("fixture hex");
            u8::from_str_radix(text, 16).expect("fixture byte")
        })
        .collect();
    let mut file = tempfile::Builder::new().suffix(".HLT").tempfile().unwrap();
    std::io::Write::write_all(&mut file, &bytes).unwrap();
    file
}

/// 국내 명세서 절 구성 회귀 표본. 【발명의 배경이 되는 기술】, 【발명의 내용】,
/// 【해결하고자 하는 과제】, 【과제의 해결 수단】, 【발명의 효과】, 【부호의 설명】을 모두 갖는다.
/// 출처 `domestic_b_보정용.HLT`, SHA-256
/// B36E2145ED01485479CE9A86E2D3B443F379FB7A57C32E731261341444F3A1F8, 60,964 바이트.
/// 정본은 같은 폴더의 `domestic_b_출원서.fin` 이다.
pub fn domestic_specification_fixture() -> tempfile::NamedTempFile {
    let encoded = local_fixture("fixtures/domestic/domestic_b_spec.HLT.hex");
    let digits: Vec<u8> = encoded
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    assert_eq!(digits.len() % 2, 0);
    let bytes: Vec<u8> = digits
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).expect("fixture hex");
            u8::from_str_radix(text, 16).expect("fixture byte")
        })
        .collect();
    let mut file = tempfile::Builder::new().suffix(".HLT").tempfile().unwrap();
    std::io::Write::write_all(&mut file, &bytes).unwrap();
    file
}

/// 실물 국내 출원서 하나(`domestic_출원서.HLT`, 261,357 바이트,
/// SHA-256 `B6DB8C30DB169349AAD065FA97D9BC7E12519C5AC2ABC930747650EAD3C2705E`).
/// 표가 하나(11행 3열)뿐이고 파일이 작아 행과 열을 전수로 훑기에 알맞다.
pub fn application_table_fixture() -> tempfile::NamedTempFile {
    let encoded = local_fixture("fixtures/table/domestic_application_table.HLT.hex");
    let digits: Vec<u8> = encoded
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    assert_eq!(digits.len() % 2, 0);
    let bytes: Vec<u8> = digits
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).expect("fixture hex");
            u8::from_str_radix(text, 16).expect("fixture byte")
        })
        .collect();
    let mut file = tempfile::Builder::new().suffix(".HLT").tempfile().unwrap();
    std::io::Write::write_all(&mut file, &bytes).unwrap();
    file
}

/// 실물 국내 출원서 표본의 문단모양 `index` 번 레코드의 정렬 코드를 `align` 으로 바꾼 사본.
/// 정렬 상위 비트가 붙은 실물을 재현한다(실물 74건 실측: `0x8001`, `0x8800`, `0xC400`,
/// `0xC800`, `0x2800`, `0x1800` 이 143 레코드에 붙어 있다).
pub fn application_table_fixture_with_align(index: u32, align: u32) -> tempfile::NamedTempFile {
    let source = application_table_fixture();
    let document = hlt_core::inspect_path(source.path(), Default::default()).unwrap();
    let shape = document
        .shapes
        .iter()
        .find(|shape| shape.index == index)
        .unwrap_or_else(|| panic!("모양 {index} 이 없다"));
    let at = shape.source_range.start_byte as usize + 12;
    let bytes = std::fs::read(source.path()).unwrap();
    let mut input = Cursor::new(&bytes);
    let layout = read_layout(&mut input, "domestic_application_table.HLT").unwrap();
    let mut decoder =
        DeflateDecoder::new(&bytes[layout.stream_start as usize..layout.stream_end as usize]);
    let mut body = Vec::new();
    decoder.read_to_end(&mut body).unwrap();
    body[at..at + 4].copy_from_slice(&align.to_le_bytes());
    let mut output = tempfile::Builder::new().suffix(".HLT").tempfile().unwrap();
    write_hlt(&layout.header, &mut Cursor::new(body), &mut output).unwrap();
    output.flush().unwrap();
    output
}

/// 저장 블록 5000바이트 + 고정 블록(길이 3 · 거리 5000 역참조). 무결성(CRC·크기)은 멀쩡하지만
/// 통특기 4KB 창으로는 못 푸는 스트림 — 창 증명이 거부해야 하는 대조군.
pub fn far_backreference_stream() -> (Vec<u8>, Vec<u8>) {
    let data: Vec<u8> = (0..5000_u32).map(|i| (i * 7 % 251) as u8).collect();
    let mut stream = vec![0x00];
    stream.extend_from_slice(&5000_u16.to_le_bytes());
    stream.extend_from_slice(&(!5000_u16).to_le_bytes());
    stream.extend_from_slice(&data);
    // 고정 블록: BFINAL 1 · BTYPE 01 · 길이 부호 257(0000001) · 거리 부호 24(11000) + 덧비트 11개(903) · 끝 256(0000000).
    let (mut bits, mut value) = (0_usize, 0_u64);
    for (field, width) in [
        (1, 1),
        (1, 2),
        (0b1000000, 7),
        (0b00011, 5),
        (903, 11),
        (0, 7),
    ] {
        value |= field << bits;
        bits += width;
    }
    stream.extend_from_slice(&value.to_le_bytes()[..bits.div_ceil(8)]);
    let mut body = data.clone();
    body.extend_from_slice(&data[..3]);
    (body, stream)
}

/// 【청구범위】·【요약서】 앞 쪽나눔 줄(그 표제 문단의 첫 줄 = 폼피드 줄). 국내 정본에는 없고,
/// 사용자 결정(2026-10-16)으로 다시 구우면 생긴다 — 왕복 비교에서 이 줄만 뺀다(도면 사이 쪽나눔은 원래 있다).
#[allow(dead_code)]
pub fn page_break_lines(
    document: &hlt_core::InspectDocument,
) -> std::collections::BTreeSet<String> {
    document
        .paragraphs
        .iter()
        .filter(|paragraph| {
            matches!(
                paragraph.text.as_str(),
                "\x0c【청구범위】" | "\x0c【요약서】"
            )
        })
        .filter_map(|paragraph| paragraph.line_ids.first().cloned())
        .collect()
}
