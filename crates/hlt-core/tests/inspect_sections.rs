use std::path::{Path, PathBuf};

use hlt_core::{InspectOptions, inspect_path};

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

#[test]
fn links_literal_sections_to_anchor_lines_and_paragraphs() {
    for relative in ["fixtures/align/align_a.HLT", "work/pct_a/pct_a_full_v5.HLT"] {
        let document = inspect_path(&repo(relative), InspectOptions::default()).unwrap();
        assert_eq!(document.sections.len(), 60);
        let title = &document.sections[1];
        assert_eq!(title.id, "section:2");
        assert_eq!(title.title, "【발명의 명칭】");
        assert_eq!(title.anchor_line_id.as_deref(), Some("line:1"));
        assert_eq!(title.anchor_paragraph_id.as_deref(), Some("paragraph:1"));
        assert!(title.source_range.start_byte < title.source_range.end_byte);

        assert_eq!(document.sections[8].title, "【표 1】");
        assert!(document.sections[8].anchor_paragraph_id.is_some());
        assert_eq!(document.sections[9].title, "【표 2】");
        assert!(document.sections[9].anchor_paragraph_id.is_some());
    }
}

#[test]
fn stable_ids_and_ranges_repeat_across_reads() {
    let path = repo("work/pct_a/pct_a_full_v5.HLT");
    let first = inspect_path(&path, InspectOptions::default()).unwrap();
    let second = inspect_path(&path, InspectOptions::default()).unwrap();
    assert_eq!(first.sections, second.sections);
    assert_eq!(first.paragraphs, second.paragraphs);
    assert_eq!(first.lines, second.lines);
}

#[test]
fn summary_exposes_document_title_and_fixed_block_fingerprints() {
    let cases = [
        (
            "fixtures/align/align_a.HLT",
            "FFD362235603853338E1A836858A45D3025F1F306A4E4FC97EAA0BD3D2112726",
            "154EB50FCC86957AF4E38004D42DA4B9873B15E8CCE36E4E33F5D6B1D6342010",
            "DA9E81F81C02A3F7439D6482B56C91B6C61B7D89EB0AD158D81D100D8D30851D",
        ),
        (
            "work/pct_a/pct_a_full_v5.HLT",
            "C779E26C9F6783D104B0974AAC2432C84CB6E193D0CAE379A8C695C17217B64A",
            "DB67DD49C9A40477E2BBEC5137008AF099E489C54C8FC443DCCAFC4CA15106BF",
            "7A8333966E717DF4805BCE62291FF9CEAC48355B617BF1FC2EA07DD2E25D5511",
        ),
    ];
    for (relative, shapes, shape_order, style_table) in cases {
        let document = inspect_path(&repo(relative), InspectOptions::default()).unwrap();
        assert_eq!(document.summary.document_title, "【발명의 설명】");
        assert_eq!(document.summary.fixed_blocks.shapes_sha256, shapes);
        assert_eq!(
            document.summary.fixed_blocks.shape_order_sha256,
            shape_order
        );
        assert_eq!(
            document.summary.fixed_blocks.second_shape_order_sha256,
            "D0DCEE996BA9BC55A3A4B1D496490DFC62742451B6F00DB736A27D5B84F0E44B"
        );
        assert_eq!(
            document.summary.fixed_blocks.style_table_sha256,
            style_table
        );
    }
}

#[test]
fn ranges_match_independently_derived_offsets() {
    let cases = [
        (
            "fixtures/align/align_a.HLT",
            (4220_u64, 4348_u64),
            (4222_u64, 4292_u64),
            [(173185_u64, 173911_u64), (173996_u64, 174073_u64)],
            (7162_u64, 7163_u64),
        ),
        (
            "work/pct_a/pct_a_full_v5.HLT",
            (4220_u64, 4348_u64),
            (4222_u64, 4300_u64),
            [(172529_u64, 173273_u64), (173362_u64, 173451_u64)],
            (7160_u64, 7161_u64),
        ),
    ];
    for (relative, paragraph, first_line, tables, image) in cases {
        let document = inspect_path(&repo(relative), InspectOptions::default()).unwrap();
        assert_eq!(
            (
                document.sections[1].table_range.start_byte,
                document.sections[1].table_range.end_byte
            ),
            (111, 178)
        );
        assert_eq!(
            (
                document.sections[1].source_range.start_byte,
                document.sections[1].source_range.end_byte
            ),
            (111, 126)
        );
        assert_eq!(
            (
                document.paragraphs[4].source_range.start_byte,
                document.paragraphs[4].source_range.end_byte
            ),
            paragraph
        );
        let line = document
            .lines
            .iter()
            .find(|line| line.id == "line:4")
            .unwrap();
        assert_eq!(
            (line.source_range.start_byte, line.source_range.end_byte),
            first_line
        );
        for (table, expected) in document.tables.iter().zip(tables) {
            assert_eq!(
                (table.source_range.start_byte, table.source_range.end_byte),
                expected
            );
        }
        let first_image = &document.images[0];
        assert_eq!(
            (
                first_image.source_range.start_byte,
                first_image.source_range.end_byte
            ),
            image
        );
    }
}
