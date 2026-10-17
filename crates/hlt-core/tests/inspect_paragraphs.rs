use std::path::{Path, PathBuf};

use hlt_core::{InspectOptions, inspect_path};

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

#[test]
fn exposes_title_content_and_raw_first_line_indent() {
    for relative in ["fixtures/align/align_a.HLT", "work/pct_a/pct_a_full_v5.HLT"] {
        let document = inspect_path(&repo(relative), InspectOptions::default()).unwrap();
        let title = &document.paragraphs[2];
        assert!(!title.text_truncated);
        assert!(title.text.len() <= 16 * 1024);
        assert_eq!(title.id, "paragraph:2");
        assert_eq!(title.section_id.as_deref(), Some("section:2"));
        assert_eq!(digest(&title.text), "31c7039b7d2a1da6");
        assert_eq!(title.indent_raw, 800);
        assert_eq!(title.align, "left");

        let representative = &document.paragraphs[4];
        assert_eq!(representative.indent_raw, 800);
        assert_eq!(representative.align, "justified");
        assert!(representative.source_range.start_byte < representative.source_range.end_byte);
        assert!(!representative.line_ids.is_empty());
        assert!(document.summary.text_index_bytes <= 4 * 1024 * 1024);
        assert!(!document.summary.text_index_truncated);
    }
}

#[test]
fn makes_the_missing_v5_wipo_identifier_axis_visible() {
    let source = inspect_path(
        &repo("fixtures/align/align_a.HLT"),
        InspectOptions::default(),
    )
    .unwrap();
    let generated = inspect_path(
        &repo("work/pct_a/pct_a_full_v5.HLT"),
        InspectOptions::default(),
    )
    .unwrap();

    assert_eq!(source.summary.paragraph_wipoid_nonzero, 2_708);
    assert_eq!(source.summary.paragraph_wipoid_distinct, 874);
    assert_eq!(generated.summary.paragraph_wipoid_nonzero, 0);
    assert_eq!(generated.summary.paragraph_wipoid_distinct, 0);
}

mod support;

#[test]
fn reads_an_opinion_whose_section_table_is_empty() {
    let fixture = support::opinion_without_section_table_fixture();
    let document = inspect_path(fixture.path(), InspectOptions::default()).unwrap();

    assert!(document.sections.is_empty());
    assert_eq!(document.paragraphs[0].text, "[의견내용]");
    assert_eq!(document.paragraphs[0].source_range.start_byte, 111);
    assert!(document.paragraphs[1].text.starts_with("Ⅰ. 통지내용"));
}

/// 실물 문서의 글은 저장소에 싣지 않는다 — SHA-256 앞 16자로 대조한다.
#[allow(dead_code)]
fn digest(text: &str) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(text.as_bytes()))[..16].to_string()
}

/// 글 앞 `chars` 글자가 그 SHA-256(앞 16자)인가.
#[allow(dead_code)]
fn opens_with(text: &str, chars: usize, expected: &str) -> bool {
    digest(&text.chars().take(chars).collect::<String>()) == expected
}

/// 실물 본문 첫 문단(기술분야)과 둘째 문단(배경)의 첫머리 — 글 대신 지문.
#[allow(dead_code)]
const TECH_FIELD: &str = "4230137345539271";
#[allow(dead_code)]
const BACKGROUND: &str = "1a352eee416dc74b";
