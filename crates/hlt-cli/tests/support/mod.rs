#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
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

pub fn validate_success_envelope(value: &serde_json::Value) {
    let command_schema: serde_json::Value = serde_json::from_slice(
        &std::fs::read(repo_root().join("contracts/command-v1.schema.json")).unwrap(),
    )
    .unwrap();
    let document_schema: serde_json::Value = serde_json::from_slice(
        &std::fs::read(repo_root().join("contracts/document-v1.schema.json")).unwrap(),
    )
    .unwrap();
    let command_validator = jsonschema::JSONSchema::compile(&command_schema).unwrap();
    assert_valid(&command_validator, value);
    let document_validator = jsonschema::JSONSchema::compile(&document_schema).unwrap();
    assert_valid(&document_validator, &value["result"]);
}

pub fn validate_error_envelope(value: &serde_json::Value) {
    let command_schema: serde_json::Value = serde_json::from_slice(
        &std::fs::read(repo_root().join("contracts/command-v1.schema.json")).unwrap(),
    )
    .unwrap();
    let validator = jsonschema::JSONSchema::compile(&command_schema).unwrap();
    assert_valid(&validator, value);
}

pub fn validate_inspect_envelope(value: &serde_json::Value) {
    let command_schema: serde_json::Value = serde_json::from_slice(
        &std::fs::read(repo_root().join("contracts/command-v1.schema.json")).unwrap(),
    )
    .unwrap();
    let inspect_schema: serde_json::Value = serde_json::from_slice(
        &std::fs::read(repo_root().join("contracts/inspect-v1.schema.json")).unwrap(),
    )
    .unwrap();
    assert_valid(
        &jsonschema::JSONSchema::compile(&command_schema).unwrap(),
        value,
    );
    assert_valid(
        &jsonschema::JSONSchema::compile(&inspect_schema).unwrap(),
        &value["result"],
    );
}

pub fn validate_repack_envelope(value: &serde_json::Value) {
    let command_schema: serde_json::Value = serde_json::from_slice(
        &std::fs::read(repo_root().join("contracts/command-v1.schema.json")).unwrap(),
    )
    .unwrap();
    let repack_schema: serde_json::Value = serde_json::from_slice(
        &std::fs::read(repo_root().join("contracts/repack-v1.schema.json")).unwrap(),
    )
    .unwrap();
    assert_valid(
        &jsonschema::JSONSchema::compile(&command_schema).unwrap(),
        value,
    );
    assert_valid(
        &jsonschema::JSONSchema::compile(&repack_schema).unwrap(),
        &value["result"],
    );
}

pub fn inspect_schema_accepts(value: &serde_json::Value) -> bool {
    let inspect_schema: serde_json::Value = serde_json::from_slice(
        &std::fs::read(repo_root().join("contracts/inspect-v1.schema.json")).unwrap(),
    )
    .unwrap();
    jsonschema::JSONSchema::compile(&inspect_schema)
        .unwrap()
        .is_valid(value)
}

pub fn hml_inspect_schema_accepts(value: &serde_json::Value) -> bool {
    let inspect_schema: serde_json::Value = serde_json::from_slice(
        &std::fs::read(repo_root().join("contracts/hml-inspect-v1.schema.json")).unwrap(),
    )
    .unwrap();
    jsonschema::JSONSchema::compile(&inspect_schema)
        .unwrap()
        .is_valid(value)
}

pub fn kipo_xml_inspect_schema_accepts(value: &serde_json::Value) -> bool {
    let wrapper_schema: serde_json::Value = serde_json::from_slice(
        &std::fs::read(repo_root().join("contracts/kipo-xml-inspect-v1.schema.json")).unwrap(),
    )
    .unwrap();
    let page_schema: serde_json::Value = serde_json::from_slice(
        &std::fs::read(repo_root().join("contracts/hml-inspect-v1.schema.json")).unwrap(),
    )
    .unwrap();
    jsonschema::JSONSchema::compile(&wrapper_schema)
        .unwrap()
        .is_valid(value)
        && jsonschema::JSONSchema::compile(&page_schema)
            .unwrap()
            .is_valid(&value["page"])
}

pub fn docx_inspect_schema_accepts(value: &serde_json::Value) -> bool {
    let wrapper: serde_json::Value = serde_json::from_slice(
        &std::fs::read(repo_root().join("contracts/docx-inspect-v1.schema.json")).unwrap(),
    )
    .unwrap();
    let hml: serde_json::Value = serde_json::from_slice(
        &std::fs::read(repo_root().join("contracts/hml-inspect-v1.schema.json")).unwrap(),
    )
    .unwrap();
    jsonschema::JSONSchema::compile(&wrapper)
        .unwrap()
        .is_valid(value)
        && (value["page"].is_null()
            || jsonschema::JSONSchema::compile(&hml)
                .unwrap()
                .is_valid(&value["page"]))
}

pub fn command_schema_accepts(value: &serde_json::Value) -> bool {
    let command_schema: serde_json::Value = serde_json::from_slice(
        &std::fs::read(repo_root().join("contracts/command-v1.schema.json")).unwrap(),
    )
    .unwrap();
    jsonschema::JSONSchema::compile(&command_schema)
        .unwrap()
        .is_valid(value)
}

fn assert_valid(validator: &jsonschema::JSONSchema, value: &serde_json::Value) {
    if let Err(errors) = validator.validate(value) {
        let messages: Vec<_> = errors.map(|error| error.to_string()).collect();
        panic!("schema validation failed: {}", messages.join("; "));
    }
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

pub fn write_temp_container_with_far_backreference() -> tempfile::NamedTempFile {
    let (body, stream) = far_backreference_stream();
    let mut bytes = vec![0; 0x108];
    bytes[..17].copy_from_slice(b"HAN Lite Document");
    bytes.extend_from_slice(&stream);
    bytes.extend_from_slice(&crc32(&body).to_le_bytes());
    bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
    let mut file = tempfile::Builder::new().suffix(".HLT").tempfile().unwrap();
    std::io::Write::write_all(&mut file, &bytes).unwrap();
    file
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
