use hlt_core::{
    GeneratedImageAnchor, GeneratedImagePayload, GeneratedLine, GeneratedLineEnding,
    GenerationDialect, HltBodyPlan, HmlDocument, HmlImage, HmlSummary, TiffPixels,
    build_hlt_image_record, build_rgb_tiff, build_tiff, inspect_docx_path, inspect_kipo_xml_path,
    load_docx_image_payloads, load_hml_image_payloads_from_directory, plan_hlt_body,
    plan_hlt_characters, plan_hlt_images,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::PathBuf};

const PNG_2X1: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x7b, 0x40, 0xe8,
    0xdd, 0x00, 0x00, 0x00, 0x0f, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xc0,
    0xf0, 0x9f, 0x01, 0x00, 0x07, 0xff, 0x01, 0xff, 0x01, 0x7f, 0x89, 0xa7, 0x00, 0x00, 0x00, 0x00,
    0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

#[test]
fn builds_exact_rgb_tiff_and_object_record() {
    let pixels = [255, 0, 0, 0, 255, 0];
    let tiff = build_rgb_tiff(2, 1, &pixels).unwrap();
    assert_eq!(tiff.len(), 376);
    assert_eq!(
        hex(&Sha256::digest(&tiff)),
        "BDBBBC5AB9FAECC6E42065289ABA22B4B46CC073597A41FA2E5F81EA0750F975"
    );

    let record = build_hlt_image_record(
        r"C:\KIPONET\NKEDITOR\TEMP\SEOAN\pat00024.jpg",
        2,
        1,
        &pixels,
        TiffPixels::Rgb8,
        71,
        35,
        Some((53, 26)),
    )
    .unwrap();
    assert_eq!(record.len(), 760);
    assert_eq!(
        hex(&Sha256::digest(&record)),
        "9FBEDA56ACC39635294E88B1CCBE7E46CB832B53887B40DCF37F65E6DB2B0E3F"
    );
}

#[test]
fn appends_image_occurrences_in_order_and_reuses_identical_source_payloads() {
    let document = image_document();
    let body = image_body();
    let characters = plan_hlt_characters(&body).unwrap();
    let payloads = vec![
        GeneratedImagePayload {
            image_id: "image:0".into(),
            bytes: PNG_2X1.to_vec(),
        },
        GeneratedImagePayload {
            image_id: "image:1".into(),
            bytes: PNG_2X1.to_vec(),
        },
    ];

    let plan = plan_hlt_images(&document, &body, &characters, &payloads).unwrap();
    assert_eq!(plan.object_count, 26);
    assert_eq!(plan.object_table.len(), 2_405 + 760 * 2);
    assert_eq!(
        u32::from_le_bytes(plan.object_table[1..5].try_into().unwrap()),
        26
    );
    assert_eq!(
        plan.images
            .iter()
            .map(|image| (
                image.image_id.as_str(),
                image.code,
                image.payload_sha256.as_str()
            ))
            .collect::<Vec<_>>(),
        [
            (
                "image:0",
                24,
                "78253200C249D4D84293B2A1E6FC803ADEDEBA8645F38D6BC2F6734A2DFACEC9"
            ),
            (
                "image:1",
                25,
                "78253200C249D4D84293B2A1E6FC803ADEDEBA8645F38D6BC2F6734A2DFACEC9"
            ),
        ]
    );
    assert_eq!(
        plan.images
            .iter()
            .map(|image| image.record_sha256.as_str())
            .collect::<Vec<_>>(),
        [
            "9FBEDA56ACC39635294E88B1CCBE7E46CB832B53887B40DCF37F65E6DB2B0E3F",
            "EA0799E65AB36ABB34EDECFAF4DF9C1F35C990B7068390101844F068EC3175FD",
        ]
    );
    assert_eq!(words(&plan.spans), [2, 24, 0, 2, 25, 0]);

    let first = 2_405;
    assert_eq!(word(&plan.object_table, first + 48), 71);
    assert_eq!(word(&plan.object_table, first + 52), 35);
    assert_eq!(word(&plan.object_table, first + 364), 26);
    assert_eq!(word(&plan.object_table, first + 368), 53);
}

#[test]
fn plans_all_281_counterpart_image_occurrences_from_123_named_files() {
    let source = repo_root().join("work/pct_a/pct_a.xml");
    let parsed = inspect_kipo_xml_path(&source).unwrap();
    let body = plan_hlt_body(&parsed.document, GenerationDialect::PatentSpecificationKo).unwrap();
    let characters = plan_hlt_characters(&body).unwrap();
    let payloads =
        load_hml_image_payloads_from_directory(&parsed.document, source.parent().unwrap()).unwrap();
    let plan = plan_hlt_images(&parsed.document, &body, &characters, &payloads).unwrap();

    assert_eq!(payloads.len(), 281);
    assert_eq!(plan.images.len(), 281);
    assert_eq!(
        parsed
            .document
            .images
            .iter()
            .map(|image| image.name.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        123
    );
    assert_eq!(
        plan.images
            .iter()
            .map(|image| image.payload_sha256.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        118
    );
    assert_eq!(plan.object_count, characters.object_count + 281);
    assert_eq!(span_count(&plan.spans), body.lines.len());
}

#[test]
fn plans_all_69_docx_image_occurrences_from_embedded_opc_media() {
    let source = repo_root().join("work/pct_a/diagnostics/pct_a_source.docx");
    let parsed = inspect_docx_path(&source).unwrap();
    let body = plan_hlt_body(&parsed.document, GenerationDialect::PctPatent).unwrap();
    let characters = plan_hlt_characters(&body).unwrap();
    let payloads = load_docx_image_payloads(&source, &parsed).unwrap();
    let plan = plan_hlt_images(&parsed.document, &body, &characters, &payloads).unwrap();

    assert_eq!(payloads.len(), 69);
    assert_eq!(plan.images.len(), 69);
    assert_eq!(plan.object_count, characters.object_count + 69);
    assert_eq!(span_count(&plan.spans), body.lines.len());
}

fn image_document() -> HmlDocument {
    HmlDocument {
        source: "fixture.hml".into(),
        source_sha256: "0".repeat(64),
        encoding: "cp949".into(),
        title: "시험".into(),
        fonts: vec![],
        summary: HmlSummary {
            section_count: 0,
            top_level_paragraph_count: 0,
            content_paragraph_count: 0,
            table_count: 0,
            row_count: 0,
            cell_count: 0,
            image_count: 2,
        },
        sections: vec![],
        paragraphs: vec![],
        tables: vec![],
        cells: vec![],
        images: vec![image("image:1", 1), image("image:0", 0)],
    }
}

fn image(id: &str, occurrence: u32) -> HmlImage {
    HmlImage {
        id: id.into(),
        occurrence,
        name: "shared.png".into(),
        width: Some(71),
        height: Some(35),
        secondary_width: Some(53),
        secondary_height: Some(26),
        paragraph_id: Some(format!("paragraph:{}", occurrence + 1)),
        cell_id: None,
    }
}

fn image_body() -> HltBodyPlan {
    HltBodyPlan {
        section_table: vec![],
        body: vec![],
        lines: vec![image_line("paragraph:1"), image_line("paragraph:2")],
        images: vec![
            GeneratedImageAnchor {
                image_id: "image:0".into(),
                line_index: 0,
                character_index: 0,
            },
            GeneratedImageAnchor {
                image_id: "image:1".into(),
                line_index: 1,
                character_index: 0,
            },
        ],
        cell_ranges: vec![],
    }
}

fn image_line(paragraph_id: &str) -> GeneratedLine {
    GeneratedLine {
        paragraph_id: paragraph_id.into(),
        cell_id: None,
        text: "_".into(),
        shape_index: 1,
        wipo_id: Some(1),
        formats: vec![],
        ending: GeneratedLineEnding::Paragraph,
    }
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|value| format!("{value:02X}")).collect()
}

fn span_count(bytes: &[u8]) -> usize {
    let mut count = 0;
    let mut offset = 0;
    while offset < bytes.len() {
        let length = word(bytes, offset) as usize;
        offset += 4 + length * 4;
        count += 1;
    }
    assert_eq!(offset, bytes.len());
    count
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

#[test]
fn builds_grayscale_tiff_when_the_source_image_is_grayscale() {
    // 네이티브 PCT 저장본 실측: 이미지 70장이 8비트 그레이스케일 TIFF 다
    // (Photometric=1, SamplesPerPixel=1, BitsPerSample 인라인 8, StripOffsets=364,
    //  StripByteCounts = 가로 × 세로). 원본 DOCX 의 JPEG 69장도 전부 그레이스케일이라
    // 통특기는 원본의 색 모드를 보존한다. pct_a 저장본은 RGB 라 두 경로가 다 필요하다.
    let pixels = [0_u8, 255];
    let tiff = build_tiff(2, 1, &pixels, TiffPixels::Gray8).unwrap();

    assert_eq!(tiff.len(), 366);
    assert_eq!(&tiff[..4], b"II*\0");
    assert_eq!(tiff_entry(&tiff, 0x0106), Some(1)); // Photometric = BlackIsZero
    assert_eq!(tiff_entry(&tiff, 0x0115), Some(1)); // SamplesPerPixel
    assert_eq!(tiff_entry(&tiff, 0x0102), Some(8)); // BitsPerSample 인라인
    assert_eq!(tiff_entry(&tiff, 0x0111), Some(364)); // StripOffsets
    assert_eq!(tiff_entry(&tiff, 0x0117), Some(2)); // StripByteCounts
    assert_eq!(&tiff[364..], &pixels);
}

#[test]
fn real_docx_generation_keeps_grayscale_payloads() {
    // 원본 DOCX 의 JPEG 69장은 전부 8비트 그레이스케일이고 네이티브 저장본도 그대로
    // 그레이스케일 TIFF 로 담는다. 레코드 크기 = 프리픽스 384 + TIFF 헤더 364 + 가로 × 세로.
    let path = repo_root().join("work/pct_a/diagnostics/pct_a_source.docx");
    let result = inspect_docx_path(&path).unwrap();
    let payloads = load_docx_image_payloads(&path, &result).unwrap();
    let body = plan_hlt_body(&result.document, GenerationDialect::PctPatent).unwrap();
    let characters = plan_hlt_characters(&body).unwrap();
    let plan = plan_hlt_images(&result.document, &body, &characters, &payloads).unwrap();

    assert_eq!(plan.images.len(), 69);
    for object in &plan.images {
        let pixels = object.pixel_width as u64 * object.pixel_height as u64;
        assert_eq!(u64::from(object.record_bytes), 384 + 364 + pixels);
    }
}

fn tiff_entry(tiff: &[u8], wanted: u16) -> Option<u32> {
    let ifd = u32::from_le_bytes(tiff[4..8].try_into().ok()?) as usize;
    let count = u16::from_le_bytes(tiff[ifd..ifd + 2].try_into().ok()?);
    for index in 0..count as usize {
        let entry = ifd + 2 + 12 * index;
        let tag = u16::from_le_bytes(tiff[entry..entry + 2].try_into().ok()?);
        if tag != wanted {
            continue;
        }
        let field_type = u16::from_le_bytes(tiff[entry + 2..entry + 4].try_into().ok()?);
        let raw = u32::from_le_bytes(tiff[entry + 8..entry + 12].try_into().ok()?);
        return Some(if field_type == 3 { raw & 0xffff } else { raw });
    }
    None
}
