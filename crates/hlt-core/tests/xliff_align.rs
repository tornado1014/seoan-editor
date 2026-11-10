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
            ("Middleword.", "가나다"),
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

#[test]
fn absurdly_deep_nesting_is_refused_without_a_panic() {
    let nest = |depth: usize| {
        format!(
            "<xliff version=\"1.2\"><file><body><trans-unit id=\"1\"><source>{o}글{c}</source><target>{o}글{c}</target></trans-unit></body></file></xliff>",
            o = "<g>".repeat(depth),
            c = "</g>".repeat(depth)
        )
    };
    let dir = tempfile::tempdir().unwrap();
    let deep = dir.path().join("deep.xlf");
    std::fs::write(&deep, nest(1000)).unwrap();
    let e = read_xliff(&deep).unwrap_err();
    assert!(
        e.message.chars().any(|c| ('가'..='힣').contains(&c)),
        "{}",
        e.message
    );
    // 보통 깊이는 그대로 읽는다.
    let fine = dir.path().join("fine.xlf");
    std::fs::write(&fine, nest(20)).unwrap();
    assert_eq!(read_xliff(&fine).unwrap().len(), 1);
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

fn single(doc: &str, seg_text: &str) -> hlt_core::Alignment {
    align_xliff(&[para("a", doc)], &[seg("S.", seg_text, Some("g"))], &[])
}

#[test]
fn a_match_must_end_on_a_boundary() {
    // 다른 문장의 앞 조각 · 조사가 붙은 말 중간은 맞춤이 아니다.
    // (2단계가 한 문단 ↔ 한 묶음을 차례로 채워 문장 짝을 낼 수는 있다 — 1단계 적중 범위(글 앞 조각)가 아니라
    // 문서 가르기 범위 전체여야 한다.)
    let whole = |doc: &str| {
        let a = single(
            doc,
            if doc.starts_with("FIG") {
                "FIG. 1"
            } else {
                "도 1"
            },
        );
        a.target_sentences[0].1 == [(0, doc.chars().count() as u32)]
    };
    assert!(whole("도 1A는 평면도이다."));
    assert!(whole("도 1은 평면도이다."));
    assert!(whole("FIG. 1A shows a device."));
    // 다음 글자가 문장부호이면 맞춤.
    let a = single("A는 B이다. C는 D이다.", "A는 B이다");
    assert_eq!(pairs_of(&a), [sentence_pair("a", 0, 0, 0)]);
    assert_eq!(a.target_sentences[0].1[0], (0, 6));
    // 맞춘 글이 부호로 끝나면 뒤에 공백이 없어도 맞춤.
    let a = single("A는 B이다.C는 D이다.", "A는 B이다.");
    assert_eq!(pairs_of(&a), [sentence_pair("a", 0, 0, 0)]);
}

#[test]
fn a_searched_match_must_start_on_a_boundary() {
    let doc = vec![
        para("p0", "첫 문단 문장이 여기 있다."),
        para("p1", "둘째 문단 문장이 여기 있다."),
        para("p2", "앞에 붙은 말이 있는 끼어 있는 이상한 꼬리 문장이다."),
    ];
    let segs = vec![
        seg("1.", "첫 문단 문장이 여기 있다.", Some("a")),
        seg("X.", "끼어 있는 이상한 꼬리 문장이다.", Some("b")),
        seg("2.", "둘째 문단 문장이 여기 있다.", Some("c")),
    ];
    let a = align_xliff(&doc, &segs, &[]);
    let got: Vec<(String, Option<u32>)> = a
        .pairs
        .iter()
        .map(|p| (p.target_id.clone(), p.target_sentence))
        .collect();
    assert_eq!(
        got,
        [("p0".to_string(), Some(0)), ("p1".to_string(), Some(0))]
    );
}

// ---- 2단계: 글자가 안 맞는 문단은 XLIFF 묶음 · 세그먼트로 추정 정렬 ----

fn numbered(id: &str, text: &str, number: &str) -> EditorParagraph {
    EditorParagraph {
        number: Some(number.into()),
        ..para(id, text)
    }
}

fn of(a: &hlt_core::Alignment, id: &str) -> Vec<P> {
    pairs_of(a).into_iter().filter(|p| p.0 == id).collect()
}

/// 묶음 하나: `[000N]` 식별번호로 시작하는 원문 + 세그먼트 번역(문서와 글자가 조금씩 다르다).
fn group(n: u32, parts: &[(&str, &str)]) -> Vec<Segment> {
    parts
        .iter()
        .enumerate()
        .map(|(i, (s, t))| {
            let source = if i == 0 {
                format!("[{n:04}] {s}")
            } else {
                s.to_string()
            };
            seg(&source, t, Some(&format!("g{n}")))
        })
        .collect()
}

/// 문단 다섯: 세그먼트 번역은 문서 글과 낱말 하나씩 다르다(1단계 0짝). 문단 3 만 문장 수가 다르다.
fn edited_five() -> (Vec<EditorParagraph>, Vec<Segment>) {
    let doc = vec![
        numbered("p1", "장치가 도시되었다. 층이 있다.", "0001"),
        numbered("p2", "기판이 도시되었다. 전극이 있다.", "0002"),
        numbered("p3", "제어부가 도시되었다. 전원이 있다.", "0003"),
        numbered("p4", "센서가 도시되었다.", "0004"),
        numbered("p5", "모터가 도시되었다. 축이 있다.", "0005"),
    ];
    let mut segs = Vec::new();
    segs.extend(group(
        1,
        &[
            ("The device is shown.", "장치가 도시된다."),
            ("A layer exists.", "층이 존재한다."),
        ],
    ));
    segs.extend(group(
        2,
        &[
            ("The substrate is shown.", "기판이 도시된다."),
            ("An electrode exists.", "전극이 존재한다."),
        ],
    ));
    segs.extend(group(
        3,
        &[
            ("The controller is shown.", "제어부가 도시된다."),
            ("A power source exists.", "전원이 존재한다."),
            ("Another thing exists.", "다른 것이 존재한다."),
        ],
    ));
    segs.extend(group(4, &[("The sensor is shown.", "센서가 도시된다.")]));
    segs.extend(group(
        5,
        &[
            ("The motor is shown.", "모터가 도시된다."),
            ("A shaft exists.", "축이 존재한다."),
        ],
    ));
    (doc, segs)
}

#[test]
fn stage_two_pairs_unmatched_paragraphs_by_ids_and_segment_boundaries() {
    let (doc, segs) = edited_five();
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(
        of(&a, "p1"),
        [sentence_pair("p1", 0, 0, 0), sentence_pair("p1", 1, 0, 1)]
    );
    assert_eq!(
        of(&a, "p2"),
        [sentence_pair("p2", 0, 1, 0), sentence_pair("p2", 1, 1, 1)]
    );
    // 문장 수(2) ≠ 세그먼트 수(3) → 문단 짝 하나.
    assert_eq!(of(&a, "p3"), [paragraph_pair("p3", 2)]);
    assert_eq!(of(&a, "p4"), [sentence_pair("p4", 0, 3, 0)]);
    assert_eq!(
        of(&a, "p5"),
        [sentence_pair("p5", 0, 4, 0), sentence_pair("p5", 1, 4, 1)]
    );
    // 문장 범위는 문서의 `sentences` 가르기.
    assert_eq!(
        a.target_sentences
            .iter()
            .find(|t| t.0 == "p1")
            .unwrap()
            .1
            .len(),
        2
    );
}

#[test]
fn a_stage_one_match_keeps_its_range_and_stage_two_fills_only_the_rest() {
    let (mut doc, segs) = edited_five();
    // 문단 2 를 글자까지 맞춘다 — 세그먼트 하나가 문서의 두 가르기 문장을 덮는다(1단계 적중 범위).
    doc[1] = numbered("p2", "기판이 도시된다. 전극이 존재한다.", "0002");
    let mut segs = segs;
    segs.splice(
        2..4,
        [seg(
            "[0002] The substrate is shown. An electrode exists.",
            "기판이 도시된다. 전극이 존재한다.",
            Some("g2"),
        )],
    );
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(of(&a, "p2"), [sentence_pair("p2", 0, 1, 0)]);
    let range = &a.target_sentences.iter().find(|t| t.0 == "p2").unwrap().1;
    assert_eq!(range, &[(0, 19)]);
    // 나머지는 2단계.
    assert_eq!(of(&a, "p1").len(), 2);
    assert_eq!(of(&a, "p5").len(), 2);
}

#[test]
fn order_filled_stage_two_paragraphs_without_numerals_keep_sentence_pairs() {
    let doc = vec![
        para("h1", "【기술분야】"),
        para("x", "장치가 도시되었다. 층이 있다."),
        para("y", "기판이 도시되었다. 전극이 있다."),
        para("h2", "【배경기술】"),
    ];
    let mut segs = vec![seg("Technical Field", "기술 분야", Some("h1"))];
    segs.extend([
        seg("The device is shown.", "장치가 도시된다.", Some("x")),
        seg("A layer exists.", "층이 존재한다.", Some("x")),
        seg("The substrate is shown.", "기판이 도시된다.", Some("y")),
        seg("An electrode exists.", "전극이 존재한다.", Some("y")),
        seg("Background", "배경 기술", Some("h2")),
    ]);
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(
        of(&a, "x"),
        [sentence_pair("x", 0, 1, 0), sentence_pair("x", 1, 1, 1)]
    );
    assert_eq!(
        of(&a, "y"),
        [sentence_pair("y", 0, 2, 0), sentence_pair("y", 1, 2, 1)]
    );
    // 같은 도면부호가 양쪽에 있으면 문장 짝.
    let doc = vec![
        para("h1", "【기술분야】"),
        para("x", "장치(110)가 도시되었다. 층이 있다."),
        para("h2", "【배경기술】"),
    ];
    let segs = vec![
        seg("Technical Field", "기술 분야", Some("h1")),
        seg(
            "The device 110 is shown.",
            "장치(110)가 도시된다.",
            Some("x"),
        ),
        seg("A layer exists.", "층이 존재한다.", Some("x")),
        seg("Background", "배경 기술", Some("h2")),
    ];
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(
        of(&a, "x"),
        [sentence_pair("x", 0, 1, 0), sentence_pair("x", 1, 1, 1)]
    );
}

#[test]
fn a_claim_of_four_segments_pairs_its_four_component_lines() {
    let doc = vec![
        para("h", "【청구항 3】"),
        para("c0", "장치로서,"),
        para("c1", "기판(110)을 포함하고;"),
        para("c2", "층(120)을 포함하며;"),
        para("c3", "전극(130)을 포함하는, 장치."),
    ];
    let segs = vec![
        seg("What is claimed is:", "", Some("g0")),
        seg("3. A device comprising:", "장치를 포함하는:", Some("g1")),
        seg("a substrate 110;", "기판(110);", Some("g1")),
        seg("a layer 120; and", "층(120); 및", Some("g1")),
        seg("an electrode 130.", "전극(130).", Some("g1")),
    ];
    let a = align_xliff(&doc, &segs, &[]);
    let got: Vec<(String, u32, Option<u32>)> = a
        .pairs
        .iter()
        .map(|p| (p.target_id.clone(), p.source_paragraph, p.source_sentence))
        .collect();
    assert_eq!(
        got,
        [
            ("c0".to_string(), 1, Some(0)),
            ("c1".to_string(), 1, Some(1)),
            ("c2".to_string(), 1, Some(2)),
            ("c3".to_string(), 1, Some(3)),
        ]
    );
    assert!(a.pairs.iter().all(|p| p.grade == Grade::Sentence));
}

#[test]
fn a_group_whose_segments_stage_one_used_gets_no_second_use() {
    // 묶음 g 의 첫 세그먼트는 1단계가 문단 a 에 맞췄다 — 나머지 글을 가진 문단 b 는 같은 묶음의 세그먼트를 다시 못 쓴다.
    let doc = vec![
        numbered("a", "첫 문장이다.", "0001"),
        numbered("b", "둘째가 도시되었다. 셋째가 있다.", "0002"),
    ];
    let segs = vec![
        seg("[0001] First.", "첫 문장이다.", Some("g")),
        seg("Second.", "둘째가 도시된다.", Some("g")),
        seg("Third.", "셋째가 존재한다.", Some("g")),
    ];
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(of(&a, "a"), [sentence_pair("a", 0, 0, 0)]);
    assert!(of(&a, "b").iter().all(|p| p.4 != Grade::Sentence));
    // 같은 원문 칸이 두 번 나오지 않는다.
    let mut used: Vec<(u32, Option<u32>)> = a
        .pairs
        .iter()
        .map(|p| (p.source_paragraph, p.source_sentence))
        .collect();
    used.sort();
    used.dedup();
    assert_eq!(used.len(), a.pairs.len());
}

#[test]
fn stage_two_sentence_pairs_need_every_sentences_numerals_to_agree() {
    let doc = vec![
        numbered(
            "p1",
            "장치(110)가 있다. 기판(120)이 있다. 전극(130)이 있다.",
            "0001",
        ),
        numbered(
            "p2",
            "장치(110)가 있다. 기판(120)이 있다. 전극(130)이 있다.",
            "0002",
        ),
    ];
    let mut segs = group(
        1,
        &[
            ("A device 110 exists.", "장치(110)가 존재한다."),
            ("A substrate 140 exists.", "기판(140)이 존재한다."),
            ("An electrode 150 exists.", "전극(150)이 존재한다."),
        ],
    );
    segs.extend(group(
        2,
        &[
            ("A device 110 exists.", "장치(110)가 존재한다."),
            ("A substrate 120 exists.", "기판(120)이 존재한다."),
            ("An electrode 130 exists.", "전극(130)이 존재한다."),
        ],
    ));
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(of(&a, "p1"), [paragraph_pair("p1", 0)]);
    assert_eq!(of(&a, "p2").len(), 3);
    assert!(of(&a, "p2").iter().all(|p| p.4 == Grade::Sentence));
}

#[test]
fn a_group_split_over_two_document_paragraphs_keeps_both_stage_one_pairs() {
    // 묶음 g 의 세그먼트가 문서 문단 둘에 맞았다 — 둘째 문단은 닻이 못 되지만 2단계 후보도 아니다.
    // 그래서 다음 문단 b 는 다음 묶음 h 와 차례로 짝지어진다.
    let doc = vec![
        para("a1", "첫째 문단 글이 여기 있다."),
        para("a2", "둘째 문단 글이 여기 있다."),
        para("b", "다른 문장이다."),
    ];
    let segs = vec![
        seg("One.", "첫째 문단 글이 여기 있다.", Some("g")),
        seg("Two.", "둘째 문단 글이 여기 있다.", Some("g")),
        seg("Other.", "다른 문장이 다.", Some("h")),
    ];
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(of(&a, "a1"), [sentence_pair("a1", 0, 0, 0)]);
    assert_eq!(of(&a, "a2"), [sentence_pair("a2", 0, 0, 1)]);
    assert_eq!(of(&a, "b"), [sentence_pair("b", 0, 1, 0)]);
}

fn plain_body(n: usize) -> Vec<EditorParagraph> {
    let mut doc = vec![para("d", "【발명의 설명】")];
    for i in 1..=n {
        doc.push(para(&format!("p{i}"), "장치가 도시되었다. 층이 있다."));
    }
    doc
}

fn three_groups() -> Vec<Segment> {
    (1..=3)
        .flat_map(|n| {
            group(
                n,
                &[
                    ("The device is shown.", "장치가 도시된다."),
                    ("A layer exists.", "층이 존재한다."),
                ],
            )
        })
        .collect()
}

#[test]
fn stage_two_ordinal_keys_are_trusted_only_where_the_counts_agree() {
    // 번호 저장이 없는 문서: 본문 4 ↔ 묶음(인쇄 번호) 3 → 차례가 밀렸으니 약한 닻 — 증거 없으면 문단 짝.
    let a = align_xliff(&plain_body(4), &three_groups(), &[]);
    for (id, g) in [("p1", 0), ("p2", 1), ("p3", 2)] {
        assert_eq!(of(&a, id), [paragraph_pair(id, g)], "{id}");
    }
    // 4 ↔ 4 가 아니라 3 ↔ 3 이면 직접 닻 — 문장 짝.
    let a = align_xliff(&plain_body(3), &three_groups(), &[]);
    assert_eq!(
        of(&a, "p2"),
        [sentence_pair("p2", 0, 1, 0), sentence_pair("p2", 1, 1, 1)]
    );
}

#[test]
fn a_group_split_over_two_paragraphs_bounds_what_lies_between_them() {
    // 묶음 g 가 문서 문단 t1 · t2 에 맞았다. 그 사이 짝 없는 tx 는 g 뒤 묶음 h 와 차례 채움이 되면 t2 의 짝을 가로지른다.
    // 정답: 가로지르지 않는다 — tx 는 짝 없음(g 는 이미 쓴 묶음이라 문장 짝 불가 · tx 의 원문은 모른다).
    let doc = vec![
        para("t1", "첫째 문단 글이 여기 있다."),
        para("tx", "아무 묶음에도 안 맞는 글이다."),
        para("t2", "둘째 문단 글이 여기 있다."),
    ];
    let segs = vec![
        seg("One.", "첫째 문단 글이 여기 있다.", Some("g")),
        seg("Two.", "둘째 문단 글이 여기 있다.", Some("g")),
        seg("Other.", "다른 문장이 다.", Some("h")),
    ];
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(of(&a, "t1"), [sentence_pair("t1", 0, 0, 0)]);
    assert_eq!(of(&a, "t2"), [sentence_pair("t2", 0, 0, 1)]);
    assert!(of(&a, "tx").is_empty(), "{:?}", of(&a, "tx"));
}

#[test]
fn a_number_anchor_into_a_used_group_gives_a_paragraph_pair_at_most() {
    // t1 이 묶음 g1 · g2 에 맞았다(g2 는 최소가 아닌 쓴 묶음). 문단 b 가 식별번호로 g2 에 닻이면 문단 짝까지만.
    let doc = vec![
        numbered("t1", "첫째 글이 있다. 둘째 글이 있다.", "0001"),
        numbered("b", "전혀 다른 글이다.", "0002"),
    ];
    let segs = vec![
        seg("[0001] One.", "첫째 글이 있다.", Some("g1")),
        seg("[0002] Two.", "둘째 글이 있다.", Some("g2")),
    ];
    let a = align_xliff(&doc, &segs, &[]);
    assert_eq!(of(&a, "t1").len(), 2);
    assert!(
        of(&a, "b").iter().all(|p| p.4 == Grade::Paragraph),
        "{:?}",
        of(&a, "b")
    );
    let mut used: Vec<(u32, Option<u32>)> = a
        .pairs
        .iter()
        .filter(|p| p.grade == Grade::Sentence)
        .map(|p| (p.source_paragraph, p.source_sentence))
        .collect();
    used.sort();
    let n = used.len();
    used.dedup();
    assert_eq!(used.len(), n);
}

/// 역문에서 청구항 하나를 지우고 번호를 다시 매겼다: 1단계가 (역문 청3 ↔ 원문 청4)에 맞추면,
/// 번호만 같은 (역문 청4 ↔ 원문 청4)는 그 1단계 짝을 가로지르므로 짝을 내지 않는다. 1단계 짝은 남는다.
#[test]
fn a_renumbered_claim_never_pairs_across_a_stage_one_pair() {
    let doc = vec![
        para("h1", "【청구항 1】"),
        para("c1", "장치로서 부품이 있다."),
        para("h2", "【청구항 2】"),
        para("c2", "제1항에 있어서 층이 있다."),
        para("h3", "【청구항 3】"),
        para("c3", "제1항에 있어서 전극을 갖는다."),
        para("h4", "【청구항 4】"),
        para("c4", "제1항에 있어서 모터를 별도로 갖는다."),
    ];
    let segs = vec![
        seg("What is claimed is:", "", Some("g0")),
        seg(
            "1. A device having a part.",
            "장치로서 부품이 있다.",
            Some("g1"),
        ),
        seg(
            "2. The device of claim 1 having a layer.",
            "제1항에 있어서 층이 있다.",
            Some("g2"),
        ),
        seg(
            "3. The device of claim 1 having a gear.",
            "제1항에 있어서 기어가 있다.",
            Some("g3"),
        ),
        seg(
            "4. The device of claim 1 having an electrode.",
            "제1항에 있어서 전극을 갖는다.",
            Some("g4"),
        ),
        seg(
            "5. The device of claim 1 having a motor.",
            "제1항에 있어서 모터가 있다.",
            Some("g5"),
        ),
    ];
    let a = align_xliff(&doc, &segs, &[]);
    let pairs = pairs_of(&a);
    let source_of = |id: &str| -> Vec<(u32, Grade)> {
        pairs
            .iter()
            .filter(|p| p.0 == id)
            .map(|p| (p.2, p.4))
            .collect()
    };
    // 1단계: c3 ↔ 원문 청4 묶음(문단 4).
    assert!(source_of("c3").iter().any(|&(s, _)| s == 4), "{pairs:?}");
    // c4 는 번호만 같은 원문 청4(문단 4)와 짝지어지면 안 된다(교차).
    assert!(source_of("c4").iter().all(|&(s, _)| s != 4), "{pairs:?}");
    // 교차하지 않는 앞 청구항(1단계 짝)은 종전대로 남는다.
    assert!(
        !source_of("c1").is_empty() && !source_of("c2").is_empty(),
        "{pairs:?}"
    );
}
