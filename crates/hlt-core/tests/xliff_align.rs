//! XLIFF 읽기 · 세그먼트 정렬(합성 글만 — 실물 글 없음).

use std::path::{Path, PathBuf};

use hlt_core::{EditorParagraph, Grade, Pin, Segment, align_xliff, read_xliff};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/align")
        .join(name)
}

fn pairs_of(a: &hlt_core::Alignment) -> Vec<(String, Option<u32>, u32, Option<u32>, Grade)> {
    a.pairs
        .iter()
        .map(|p| {
            (
                p.target_id.clone(),
                p.target_sentence,
                p.source_paragraph,
                p.source_sentence,
                p.grade,
            )
        })
        .collect()
}

fn seg(source: &str, target: &str, group: Option<&str>) -> Segment {
    Segment {
        source: source.into(),
        target: target.into(),
        group: group.map(str::to_string),
    }
}

fn para(id: &str, text: &str) -> EditorParagraph {
    EditorParagraph {
        id: id.into(),
        text: text.into(),
        wipo_id: None,
        number: None,
        has_objects: false,
        spans: Vec::new(),
    }
}

fn texts(v: &[Segment]) -> Vec<(&str, &str)> {
    v.iter()
        .map(|s| (s.source.as_str(), s.target.as_str()))
        .collect()
}

#[test]
fn xtm_shape_reads_text_only_ignores_alt_trans_and_splits_mrk() {
    let s = read_xliff(&fixture("xtm_shape.xlf")).unwrap();
    assert_eq!(
        texts(&s),
        [
            ("The first sentence.", "첫 번째 문장입니다."),
            ("Fish & chips are served.", "생선 & 감자튀김이 나옵니다."),
            ("Third.", "셋째입니다."),
            ("One.", "하나."),
            ("Two.", "둘."),
        ]
    );
    assert!(s[0].group.is_some() && s[0].group == s[1].group);
    assert_ne!(s[1].group, s[2].group);
    assert_ne!(s[2].group, s[3].group);
    assert_eq!(s[3].group, s[4].group);
}

#[test]
fn txlf_shape_drops_tag_code_and_uses_the_paragraph_group_not_the_page() {
    let s = read_xliff(&fixture("txlf_shape.txlf")).unwrap();
    assert_eq!(
        texts(&s),
        [
            ("Alpha 1 beta.", "알파 1 베타."),
            ("Gamma delta epsilon.", "감마 델타 엡실론."),
            ("Zeta.", "제타."),
        ]
    );
    assert!(s[0].group.is_some() && s[0].group == s[1].group);
    assert_ne!(s[1].group, s[2].group);
}

#[test]
fn mxliff_shape_groups_by_para_id_collapses_whitespace_and_keeps_empty_target() {
    let s = read_xliff(&fixture("mxliff_shape.mxliff")).unwrap();
    assert_eq!(
        texts(&s),
        [
            ("Eta {1}sentence.", "에타 {1}문장."),
            ("Theta sentence.", "세타 문장."),
            ("Iota.", ""),
        ]
    );
    // 묶음(group)은 번역 단위마다 따로지만 한 문단은 para-id 로 묶인다.
    assert!(s[0].group.is_some() && s[0].group == s[1].group);
    assert_ne!(s[1].group, s[2].group);
}

#[test]
fn bom_and_crlf_are_read_as_plain_text() {
    let body = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<xliff version=\"1.2\"><file original=\"a\" source-language=\"en\" target-language=\"ko\" datatype=\"x\"><body>\r\n<trans-unit id=\"1\"><source>Line one\r\nline two.</source><target>첫 줄\r\n둘째 줄.</target></trans-unit>\r\n</body></file></xliff>\r\n";
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bom.xlf");
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(body.as_bytes());
    std::fs::write(&path, bytes).unwrap();
    let s = read_xliff(&path).unwrap();
    assert_eq!(texts(&s), [("Line one line two.", "첫 줄 둘째 줄.")]);
    assert_eq!(s[0].group, None);
}

#[test]
fn not_xliff_and_zero_segments_are_korean_errors() {
    for name in ["not_xliff.xml", "empty.xlf"] {
        let e = read_xliff(&fixture(name)).unwrap_err();
        assert!(
            e.message.chars().any(|c| ('가'..='힣').contains(&c)),
            "{name}: {}",
            e.message
        );
    }
    assert!(read_xliff(&fixture("missing_file.xlf")).is_err());
}

/// 세 문단 · 세그먼트 7개 — 문서 글과 같은 쪽 구성.
fn three_paragraphs() -> (Vec<EditorParagraph>, Vec<Segment>) {
    let doc = vec![
        para("a", "가나 첫째이다. 다라 둘째이다."),
        para("b", "마바 셋째이다. 사아 넷째이다. 자차 다섯째이다."),
        para("c", "카타 여섯째이다. 파하 일곱째이다."),
    ];
    let seg7 = vec![
        seg("S1.", "가나 첫째이다.", Some("a")),
        seg("S2.", "다라 둘째이다.", Some("a")),
        seg("S3.", "마바 셋째이다.", Some("b")),
        seg("S4.", "사아 넷째이다.", Some("b")),
        seg("S5.", "자차 다섯째이다.", Some("b")),
        seg("S6.", "카타 여섯째이다.", Some("c")),
        seg("S7.", "파하 일곱째이다.", Some("c")),
    ];
    (doc, seg7)
}

fn sentence_pair(
    t: &str,
    ts: u32,
    sp: u32,
    ss: u32,
) -> (String, Option<u32>, u32, Option<u32>, Grade) {
    (t.into(), Some(ts), sp, Some(ss), Grade::Sentence)
}

#[test]
fn every_segment_matched_gives_seven_sentence_pairs() {
    let (doc, segs) = three_paragraphs();
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(
        pairs_of(&a),
        [
            sentence_pair("a", 0, 0, 0),
            sentence_pair("a", 1, 0, 1),
            sentence_pair("b", 0, 1, 0),
            sentence_pair("b", 1, 1, 1),
            sentence_pair("b", 2, 1, 2),
            sentence_pair("c", 0, 2, 0),
            sentence_pair("c", 1, 2, 1),
        ]
    );
    assert_eq!(a.source.len(), 3);
    assert_eq!(a.source[1].text, "S3. S4. S5.");
    assert_eq!(a.source[1].sentences, [(0, 3), (4, 7), (8, 11)]);
    assert_eq!(a.target_sentences.len(), 3);
    assert!(a.dropped_pins.is_empty());
}

#[test]
fn an_edited_sentence_is_interleaved_between_matched_neighbours() {
    let (mut doc, segs) = three_paragraphs();
    doc[1].text = "마바 셋째이다. 완전히 바뀐 글이다. 자차 다섯째이다.".into();
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(
        pairs_of(&a),
        [
            sentence_pair("a", 0, 0, 0),
            sentence_pair("a", 1, 0, 1),
            sentence_pair("b", 0, 1, 0),
            sentence_pair("b", 1, 1, 1),
            sentence_pair("b", 2, 1, 2),
            sentence_pair("c", 0, 2, 0),
            sentence_pair("c", 1, 2, 1),
        ]
    );
}

#[test]
fn merged_sentences_downgrade_only_that_paragraph() {
    let doc = vec![
        para("a", "가 첫째이다. 나 둘째이다."),
        para(
            "b",
            "다 셋째이다. 라 넷째이며 마 다섯째이다. 바 여섯째이다.",
        ),
        para("c", "사 일곱째이다."),
    ];
    let segs = vec![
        seg("S1.", "가 첫째이다.", Some("a")),
        seg("S2.", "나 둘째이다.", Some("a")),
        seg("S3.", "다 셋째이다.", Some("b")),
        seg("S4.", "라 넷째이다.", Some("b")),
        seg("S5.", "마 다섯째이다.", Some("b")),
        seg("S6.", "바 여섯째이다.", Some("b")),
        seg("S7.", "사 일곱째이다.", Some("c")),
    ];
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(
        pairs_of(&a),
        [
            sentence_pair("a", 0, 0, 0),
            sentence_pair("a", 1, 0, 1),
            ("b".into(), None, 1, None, Grade::Paragraph),
            sentence_pair("c", 0, 2, 0),
        ]
    );
}

#[test]
fn repeated_sentence_is_matched_in_order_not_from_the_top() {
    let doc = vec![
        para("a", "반복 문장이다. 다른 문장이다."),
        para("b", "반복 문장이다. 반복 문장이다."),
    ];
    let segs = vec![
        seg("R.", "반복 문장이다.", Some("a")),
        seg("D.", "다른 문장이다.", Some("a")),
        seg("R.", "반복 문장이다.", Some("b")),
        seg("R.", "반복 문장이다.", Some("b")),
    ];
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(
        pairs_of(&a),
        [
            sentence_pair("a", 0, 0, 0),
            sentence_pair("a", 1, 0, 1),
            sentence_pair("b", 0, 1, 0),
            sentence_pair("b", 1, 1, 1),
        ]
    );
}

#[test]
fn empty_target_and_target_equal_to_source_make_no_pair() {
    let doc = vec![para("a", "첫째이다. Part X1. 셋째이다.")];
    let segs = vec![
        seg("One.", "첫째이다.", Some("g")),
        seg("Part X1.", "Part  X1.", Some("g")),
        seg("Empty.", "", Some("g")),
        seg("Three.", "셋째이다.", Some("g")),
    ];
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(
        pairs_of(&a),
        [sentence_pair("a", 0, 0, 0), sentence_pair("a", 2, 0, 3)]
    );
    // 원문 창에는 네 개 모두 실린다.
    assert_eq!(a.source[0].sentences.len(), 4);
    // 빈 문서 · 세그먼트 0개도 죽지 않는다.
    assert!(align_xliff(&[], &segs, &[]).pairs.is_empty());
    assert!(align_xliff(&doc, &[], &[]).pairs.is_empty());
}

#[test]
fn whitespace_runs_and_fullwidth_spaces_are_normalised() {
    let doc = vec![para("a", "가나\u{3000}다라   마바.\n사아 자차.")];
    let segs = vec![
        seg("A.", " 가나 다라 마바. ", Some("g")),
        seg("B.", "사아  자차.", Some("g")),
    ];
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(
        pairs_of(&a),
        [sentence_pair("a", 0, 0, 0), sentence_pair("a", 1, 0, 1)]
    );
}

#[test]
fn a_segment_spanning_two_document_sentences_is_one_sentence() {
    let doc = vec![para("a", "가나 첫째이다. 다라 둘째이다. 마바 셋째이다.")];
    let segs = vec![
        seg("A.", "가나 첫째이다. 다라 둘째이다.", Some("g")),
        seg("B.", "마바 셋째이다.", Some("g")),
    ];
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(
        a.target_sentences,
        [("a".to_string(), vec![(0, 17), (18, 26)])]
    );
    assert_eq!(
        pairs_of(&a),
        [sentence_pair("a", 0, 0, 0), sentence_pair("a", 1, 0, 1)]
    );
}

#[test]
fn without_groups_segments_of_one_document_paragraph_share_a_source_paragraph() {
    let (doc, mut segs) = three_paragraphs();
    for s in &mut segs {
        s.group = None;
    }
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(a.source.len(), 3);
    assert_eq!(pairs_of(&a)[2], sentence_pair("b", 0, 1, 0));
    assert_eq!(a.source[1].sentences.len(), 3);
}

#[test]
fn a_far_later_identical_sentence_does_not_swallow_the_segments_between() {
    let doc = vec![
        para("a", "처음 문장이다. 바뀐 문장이다. 셋째 문장이다."),
        para("b", "이상이다."),
    ];
    let segs = vec![
        seg("S1.", "처음 문장이다.", Some("g")),
        seg("S2.", "이상이다.", Some("g")),
        seg("S3.", "셋째 문장이다.", Some("g")),
    ];
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(
        pairs_of(&a),
        [
            sentence_pair("a", 0, 0, 0),
            sentence_pair("a", 1, 0, 1),
            sentence_pair("a", 2, 0, 2),
        ]
    );
}

#[test]
fn pins_fill_unpaired_sentences_and_unusable_ones_are_reported() {
    // 둘째 문단 첫 문장이 바뀌어 앞 이웃이 다른 문단 → 짝 없음 → 손 고정으로 채운다.
    let doc = vec![
        para("a", "가 첫째이다."),
        para("b", "완전히 바뀐 글이다. 나 셋째이다."),
    ];
    let segs = vec![
        seg("S1.", "가 첫째이다.", Some("a")),
        seg("S2.", "나 둘째이다.", Some("b")),
        seg("S3.", "나 셋째이다.", Some("b")),
    ];
    let base = align_xliff(&doc, &segs, &[]);
    assert_eq!(
        pairs_of(&base),
        [sentence_pair("a", 0, 0, 0), sentence_pair("b", 1, 1, 1)]
    );
    let ok = Pin {
        target_id: "b".into(),
        target_sentence: 0,
        source_paragraph: 1,
        source_sentence: 0,
    };
    let again = Pin {
        target_id: "a".into(),
        target_sentence: 0,
        source_paragraph: 1,
        source_sentence: 0,
    };
    let inverted = Pin {
        target_id: "b".into(),
        target_sentence: 0,
        source_paragraph: 0,
        source_sentence: 0,
    };
    let unknown = Pin {
        target_id: "zz".into(),
        target_sentence: 0,
        source_paragraph: 1,
        source_sentence: 0,
    };
    let range = Pin {
        target_id: "b".into(),
        target_sentence: 9,
        source_paragraph: 1,
        source_sentence: 0,
    };
    let a = align_xliff(&doc, &segs, &[ok.clone()]);
    assert_eq!(
        pairs_of(&a),
        [
            sentence_pair("a", 0, 0, 0),
            sentence_pair("b", 0, 1, 0),
            sentence_pair("b", 1, 1, 1),
        ]
    );
    assert!(a.dropped_pins.is_empty());
    let a = align_xliff(
        &doc,
        &segs,
        &[
            again.clone(),
            inverted.clone(),
            unknown.clone(),
            range.clone(),
        ],
    );
    assert_eq!(pairs_of(&a), pairs_of(&base));
    assert_eq!(a.dropped_pins, [again, inverted, unknown, range]);
}
