//! XLIFF 읽기 · 세그먼트 정렬(합성 글만 — 실물 글 없음).

use std::path::{Path, PathBuf};

use hlt_core::{EditorParagraph, Grade, Pin, Segment, align_xliff, read_xliff};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/align")
        .join(name)
}

fn pairs_of(a: &hlt_core::Alignment) -> Vec<P> {
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
            ("Eta sentence.", "에타 문장."),
            ("Theta sentence.", "세타 문장."),
            ("Iota.", ""),
        ]
    );
    // 묶음(group)은 번역 단위마다 따로지만 한 문단은 para-id 로 묶인다.
    assert!(s[0].group.is_some() && s[0].group == s[1].group);
    assert_ne!(s[1].group, s[2].group);
}

#[test]
fn mxliff_placeholders_are_stripped_and_locked_or_untranslatable_units_lose_their_target() {
    let s = read_xliff(&fixture("mxliff_tags.mxliff")).unwrap();
    assert_eq!(
        texts(&s),
        [
            (
                "Alpha beta bold it und both sup tag end.",
                "알파 베타 굵게 기울 밑줄 둘 위 태그 끝."
            ),
            ("Locked one.", ""),
            ("Not translated.", ""),
        ]
    );
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

const T: [&str; 7] = [
    "가나다 첫째 문장이다.",
    "라마바 둘째 문장이다.",
    "사아자 셋째 문장이다.",
    "차카타 넷째 문장이다.",
    "파하가 다섯째 문장이다.",
    "나다라 여섯째 문장이다.",
    "마바사 일곱째 문장이다.",
];

/// 세 문단 · 세그먼트 7개 — 문서 글과 같은 쪽 구성.
fn three_paragraphs() -> (Vec<EditorParagraph>, Vec<Segment>) {
    let doc = vec![
        para("a", &format!("{} {}", T[0], T[1])),
        para("b", &format!("{} {} {}", T[2], T[3], T[4])),
        para("c", &format!("{} {}", T[5], T[6])),
    ];
    let groups = ["a", "a", "b", "b", "b", "c", "c"];
    let segs = (0..7)
        .map(|i| seg(&format!("S{}.", i + 1), T[i], Some(groups[i])))
        .collect();
    (doc, segs)
}

type P = (String, Option<u32>, u32, Option<u32>, Grade);

fn sentence_pair(t: &str, ts: u32, sp: u32, ss: u32) -> P {
    (t.into(), Some(ts), sp, Some(ss), Grade::Sentence)
}

fn paragraph_pair(t: &str, sp: u32) -> P {
    (t.into(), None, sp, None, Grade::Paragraph)
}

fn seven_pairs() -> Vec<P> {
    vec![
        sentence_pair("a", 0, 0, 0),
        sentence_pair("a", 1, 0, 1),
        sentence_pair("b", 0, 1, 0),
        sentence_pair("b", 1, 1, 1),
        sentence_pair("b", 2, 1, 2),
        sentence_pair("c", 0, 2, 0),
        sentence_pair("c", 1, 2, 1),
    ]
}

#[test]
fn every_segment_matched_gives_seven_sentence_pairs() {
    let (doc, segs) = three_paragraphs();
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(pairs_of(&a), seven_pairs());
    assert_eq!(a.source.len(), 3);
    assert_eq!(a.source[1].text, "S3. S4. S5.");
    assert_eq!(a.source[1].sentences, [(0, 3), (4, 7), (8, 11)]);
    assert_eq!(a.target_sentences.len(), 3);
    assert!(a.dropped_pins.is_empty());
}

#[test]
fn an_edited_sentence_pairs_by_the_gap_rule_and_neighbours_keep_theirs() {
    let (mut doc, segs) = three_paragraphs();
    doc[1].text = format!("{} 완전히 바뀐 글이다. {}", T[2], T[4]);
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(pairs_of(&a), seven_pairs());
}

#[test]
fn merged_sentences_add_a_paragraph_pair_and_keep_the_neighbours_sentence_pairs() {
    let (mut doc, mut segs) = three_paragraphs();
    // 문서가 문단 b 의 둘째 · 셋째 문장(S4 · S5)을 하나로 합쳤다.
    doc[1].text = format!(
        "{} 차카타 넷째이며 이어서 다섯째로 이어진 문장이다. {}",
        T[2], T[5]
    );
    segs.truncate(5);
    segs.push(seg("S6.", T[5], Some("b")));
    let a = align_xliff(&doc[..2], &segs, &[]);
    assert_eq!(
        pairs_of(&a),
        [
            sentence_pair("a", 0, 0, 0),
            sentence_pair("a", 1, 0, 1),
            paragraph_pair("b", 1),
            sentence_pair("b", 0, 1, 0),
            sentence_pair("b", 2, 1, 3),
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
fn empty_and_unmatched_equal_targets_make_no_pair_and_do_not_count_in_a_run() {
    // 원문과 같은 글이 커서 자리에 있으면 참 짝이다.
    let doc = vec![para(
        "a",
        "첫째 문장이 여기 있다. Part X1. 셋째 문장이 여기 있다.",
    )];
    let segs = vec![
        seg("One.", "첫째 문장이 여기 있다.", Some("g")),
        seg("Part X1.", "Part  X1.", Some("g")),
        seg("Empty.", "", Some("g")),
        seg("Three.", "셋째 문장이 여기 있다.", Some("g")),
    ];
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(
        pairs_of(&a),
        [
            sentence_pair("a", 0, 0, 0),
            sentence_pair("a", 1, 0, 1),
            sentence_pair("a", 2, 0, 3)
        ]
    );
    assert_eq!(a.source[0].sentences.len(), 4);
    // 커서 자리에 없는 같은 글 · 빈 target 은 틈의 칸 수에 들지 않는다 → 고쳐 쓴 한 문장이 문장 짝이 된다.
    let doc = vec![para(
        "a",
        "첫째 문장이 여기 있다. 완전히 바뀐 글이 여기 있다. 셋째 문장이 여기 있다.",
    )];
    let segs = vec![
        seg("One.", "첫째 문장이 여기 있다.", Some("g")),
        seg("Name X1", "Name  X1", Some("g")),
        seg("Empty.", "", Some("g")),
        seg("Two.", "원래 둘째 문장이 여기 있다.", Some("g")),
        seg("Three.", "셋째 문장이 여기 있다.", Some("g")),
    ];
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(
        pairs_of(&a),
        [
            sentence_pair("a", 0, 0, 0),
            sentence_pair("a", 1, 0, 3),
            sentence_pair("a", 2, 0, 4)
        ]
    );
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
fn ranges_index_the_original_text_with_double_spaces_and_tabs() {
    let doc = vec![para("a", "가나다  첫째 문장이다.\t라마바 둘째 문장이다.")];
    let segs = vec![
        seg("A.", "가나다 첫째 문장이다.", Some("g")),
        seg("B.", "라마바 둘째 문장이다.", Some("g")),
    ];
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(
        a.target_sentences,
        [("a".to_string(), vec![(0, 13), (14, 26)])]
    );
}

#[test]
fn a_segment_spanning_two_splitter_sentences_is_one_sentence() {
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
fn a_claim_the_tool_split_into_five_segments_gets_five_pairs() {
    let parts = [
        "장치를 구비하고",
        "기판을 포함하고",
        "전극을 포함하며",
        "제어부를 포함하고",
        "전원을 포함하는 시스템",
    ];
    let doc = vec![para("c", &parts.join("; "))];
    let segs: Vec<Segment> = parts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            seg(
                &format!("P{i}"),
                &format!("{p}{}", if i < 4 { ";" } else { "" }),
                Some("g"),
            )
        })
        .collect();
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(a.pairs.len(), 5);
    assert!(a.pairs.iter().all(|p| p.grade == Grade::Sentence));
    assert_eq!(a.target_sentences[0].1.len(), 5);
}

#[test]
fn one_tool_segment_over_three_splitter_sentences_is_one_pair_for_the_whole_paragraph() {
    let text = "첫째 문장이다. 둘째 문장이다. 셋째 문장이다.";
    let doc = vec![para("a", text)];
    let segs = vec![seg("All.", text, Some("g"))];
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(pairs_of(&a), [sentence_pair("a", 0, 0, 0)]);
    assert_eq!(
        a.target_sentences,
        [("a".to_string(), vec![(0, text.chars().count() as u32)])]
    );
}

#[test]
fn a_tool_split_the_splitter_would_not_make_pairs_both_halves() {
    let doc = vec![para(
        "a",
        "앞 문장이 여기 있다. 참조 번호 FIG. 3 설명이다. 다음 문장이 여기 있다.",
    )];
    let segs = vec![
        seg("1.", "앞 문장이 여기 있다.", Some("g")),
        seg("2.", "참조 번호 FIG.", Some("g")),
        seg("3.", "3 설명이다.", Some("g")),
        seg("4.", "다음 문장이 여기 있다.", Some("g")),
    ];
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(
        pairs_of(&a),
        [
            sentence_pair("a", 0, 0, 0),
            sentence_pair("a", 1, 0, 1),
            sentence_pair("a", 2, 0, 2),
            sentence_pair("a", 3, 0, 3)
        ]
    );
    assert_eq!(a.target_sentences[0].1.len(), 4);
}

#[test]
fn an_edited_sentence_is_not_attached_to_a_far_repeat() {
    let mut doc = vec![para(
        "p0",
        "처음 문장이 여기 있다. 바뀐 문장이 여기 있다. 셋째 문장이 여기 있다.",
    )];
    for i in 1..=10 {
        doc.push(para(
            &format!("p{i}"),
            &format!("끼움 문단 번호 {i} 입니다."),
        ));
    }
    doc.push(para("p11", "원래 둘째 문장이 여기 있다."));
    let segs = vec![
        seg("1.", "처음 문장이 여기 있다.", Some("g")),
        seg("2.", "원래 둘째 문장이 여기 있다.", Some("g")),
        seg("3.", "셋째 문장이 여기 있다.", Some("g")),
    ];
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(
        pairs_of(&a),
        [
            sentence_pair("p0", 0, 0, 0),
            sentence_pair("p0", 1, 0, 1),
            sentence_pair("p0", 2, 0, 2)
        ]
    );
}

#[test]
fn a_short_repeated_target_away_from_the_cursor_is_never_jumped_to() {
    let doc = vec![
        para(
            "a",
            "처음 문장이 여기 있다. 바뀐 글이 여기 있다. 셋째 문장이 여기 있다.",
        ),
        para("b", "네."),
    ];
    let segs = vec![
        seg("1.", "처음 문장이 여기 있다.", Some("g")),
        seg("2.", "네.", Some("g")),
        seg("3.", "셋째 문장이 여기 있다.", Some("g")),
    ];
    let a = align_xliff(&doc, &segs, &[]);
    assert!(a.pairs.iter().all(|p| p.target_id != "b"));
    assert_eq!(
        pairs_of(&a),
        [
            sentence_pair("a", 0, 0, 0),
            sentence_pair("a", 1, 0, 1),
            sentence_pair("a", 2, 0, 2)
        ]
    );
}

#[test]
fn a_whole_group_with_no_hit_pairs_as_a_paragraph_between_hit_neighbours() {
    let doc = vec![
        para("a", "첫 문단 문장이 여기 있다."),
        para("b", "완전히 새로 쓴 문단이다 그렇다."),
        para("c", "셋째 문단 문장이 여기 있다."),
    ];
    let segs = vec![
        seg("1.", "첫 문단 문장이 여기 있다.", Some("a")),
        seg("2.", "원래 둘째 문단 문장이 여기 있다.", Some("b")),
        seg("3.", "셋째 문단 문장이 여기 있다.", Some("c")),
    ];
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(
        pairs_of(&a),
        [
            sentence_pair("a", 0, 0, 0),
            paragraph_pair("b", 1),
            sentence_pair("c", 0, 2, 0)
        ]
    );
    // 이웃이 p-1 · p+1 이 아니면(문단이 하나 더 끼면) 짝 없음.
    let mut doc = doc;
    doc.insert(2, para("b2", "또 새로 쓴 문단이다 그렇다."));
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(
        pairs_of(&a),
        [sentence_pair("a", 0, 0, 0), sentence_pair("c", 0, 2, 0)]
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
fn pins_fill_unpaired_sentences_and_unusable_ones_are_reported() {
    // 바뀐 문단이 문단 경계를 넘는 틈이라 짝이 없다 → 손 고정으로 채운다.
    let doc = vec![
        para("a", "가나다 첫째 문장이다."),
        para("b", "완전히 바뀐 글이다."),
        para("c", "마바사 셋째 문장이다."),
    ];
    let segs = vec![
        seg("S1.", "가나다 첫째 문장이다.", Some("x")),
        seg("S2.", "라마바 둘째 문장이다.", Some("x")),
        seg("S3.", "마바사 셋째 문장이다.", Some("y")),
    ];
    let base = align_xliff(&doc, &segs, &[]);
    assert_eq!(
        pairs_of(&base),
        [sentence_pair("a", 0, 0, 0), sentence_pair("c", 0, 1, 0)]
    );
    let pin = |t: &str, ts, sp, ss| Pin {
        target_id: t.into(),
        target_sentence: ts,
        source_paragraph: sp,
        source_sentence: ss,
    };
    let ok = pin("b", 0, 0, 1);
    let a = align_xliff(&doc, &segs, &[ok]);
    assert_eq!(
        pairs_of(&a),
        [
            sentence_pair("a", 0, 0, 0),
            sentence_pair("b", 0, 0, 1),
            sentence_pair("c", 0, 1, 0)
        ]
    );
    assert!(a.dropped_pins.is_empty());
    let bad = [
        pin("a", 0, 0, 1),
        pin("b", 0, 1, 0),
        pin("zz", 0, 1, 0),
        pin("b", 9, 1, 0),
    ];
    let a = align_xliff(&doc, &segs, &bad);
    assert_eq!(pairs_of(&a), pairs_of(&base));
    assert_eq!(a.dropped_pins, bad);
}
