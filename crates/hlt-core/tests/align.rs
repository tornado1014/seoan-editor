//! 문장 가르기 · 추정 정렬(합성 글만).

use hlt_core::{EditorParagraph, Grade, Lang, Pin, align_inferred, sentences};

fn chars(text: &str, ranges: &[(u32, u32)]) -> Vec<String> {
    let c: Vec<char> = text.chars().collect();
    ranges
        .iter()
        .map(|&(s, e)| c[s as usize..e as usize].iter().collect())
        .collect()
}

fn para(id: &str, text: &str, number: Option<&str>) -> EditorParagraph {
    EditorParagraph {
        id: id.into(),
        text: text.into(),
        wipo_id: None,
        number: number.map(str::to_string),
        has_objects: false,
        spans: Vec::new(),
    }
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn english_splits_keep_abbreviations_numbers_and_parens() {
    let t = "Referring to FIG. 3, the device 100 includes a substrate 110. The electrode is 0.5 mm thick (see, e.g., FIG. 4). Done.";
    let r = sentences(t, Lang::En, false);
    assert_eq!(
        chars(t, &r),
        [
            "Referring to FIG. 3, the device 100 includes a substrate 110.",
            "The electrode is 0.5 mm thick (see, e.g., FIG. 4).",
            "Done."
        ]
    );
}

#[test]
fn english_single_capital_and_us_do_not_split() {
    let t = "Made by J. Smith in the U.S. last year. Next one.";
    assert_eq!(sentences(t, Lang::En, false).len(), 2);
}

#[test]
fn korean_splits_and_offsets_are_chars() {
    let t = "도 3을 참조하면, 장치(100)는 기판(110)을 포함한다. 두께는 0.5 mm이다. 끝.";
    let r = sentences(t, Lang::Ko, false);
    assert_eq!(r.len(), 3);
    assert_eq!(r[0].0, 0);
    assert_eq!(r[2].1 as usize, t.chars().count());
    assert_eq!(chars(t, &r)[2], "끝.");
    assert_eq!(sentences("가나다", Lang::Ko, false), [(0, 3)]);
}

#[test]
fn claim_units_are_component_lines() {
    let t = "기판;\n상기 기판 상의 제1 전극; 및\n절연층을 포함하는, 장치.";
    let r = sentences(t, Lang::Ko, true);
    assert_eq!(
        chars(t, &r),
        [
            "기판;",
            "상기 기판 상의 제1 전극; 및",
            "절연층을 포함하는, 장치."
        ]
    );
}

#[test]
fn empty_text_has_no_sentences() {
    assert!(sentences("", Lang::Ko, false).is_empty());
    assert!(sentences("  \n ", Lang::En, true).is_empty());
}

#[test]
fn differing_sentence_count_gives_one_paragraph_pair() {
    let target = [
        para("a", "가 문장이다. 나 문장이다.", Some("0001")),
        para("b", "하나뿐이다.", Some("0002")),
        para("c", "첫째다. 둘째다.", Some("0003")),
    ];
    let source = strings(&[
        "[0001] First one. Second one.",
        "[0002] One here. And another.",
        "[0003] Alpha. Beta.",
    ]);
    let al = align_inferred(&target, &source, &[]);
    let of = |id: &str| -> Vec<_> {
        al.pairs
            .iter()
            .filter(|p| p.target_id == id)
            .map(|p| {
                (
                    p.target_sentence,
                    p.source_paragraph,
                    p.source_sentence,
                    p.grade,
                )
            })
            .collect()
    };
    assert_eq!(
        of("a"),
        [
            (Some(0), 0, Some(0), Grade::Sentence),
            (Some(1), 0, Some(1), Grade::Sentence)
        ]
    );
    assert_eq!(of("b"), [(None, 1, None, Grade::Paragraph)]);
    assert_eq!(of("c").len(), 2);
    assert_eq!(al.target_sentences.len(), 3);
    assert_eq!(al.source.len(), 3);
    assert_eq!(al.source[1].sentences.len(), 2);
}

#[test]
fn heading_anchors_pair_by_order_when_counts_match() {
    let target = [
        para("h1", "【기술분야】", None),
        para("a", "첫째 글이다.", None),
        para("b", "둘째 글이다.", None),
        para("h2", "【배경기술】", None),
        para("c", "셋째 글이다.", None),
    ];
    let source = strings(&[
        "TECHNICAL FIELD",
        "First text.",
        "Second text.",
        "Background Art:",
        "Third text.",
    ]);
    let al = align_inferred(&target, &source, &[]);
    let got: Vec<(&str, u32)> = al
        .pairs
        .iter()
        .map(|p| (p.target_id.as_str(), p.source_paragraph))
        .collect();
    assert_eq!(got, [("a", 1), ("b", 2), ("c", 4)]);
}

#[test]
fn unequal_gap_pairs_only_the_unique_number_match() {
    let target = [
        para("h1", "【배경기술】", None),
        para("a", "가 구성이다.", None),
        para("b", "도 7은 흐름도이다.", None),
        para("c", "다 구성이다.", None),
        para("h2", "【발명의 내용】", None),
    ];
    let source = strings(&[
        "Background",
        "Intro text.",
        "FIG. 7 is a flowchart.",
        "More text.",
        "Even more text.",
        "Summary",
    ]);
    let al = align_inferred(&target, &source, &[]);
    let got: Vec<(&str, u32)> = al
        .pairs
        .iter()
        .map(|p| (p.target_id.as_str(), p.source_paragraph))
        .collect();
    assert_eq!(got, [("b", 2)]);
}

#[test]
fn a_pin_realigns_a_shifted_span() {
    // 원문에 문단이 하나 더 있어 개수가 어긋난다 — 핀 하나로 그 뒤가 다시 맞는다.
    let target = [
        para("h1", "【기술분야】", None),
        para("a", "가 문장이다.", None),
        para("b", "나 문장이다.", None),
        para("c", "다 문장이다.", None),
    ];
    let source = strings(&["Technical Field", "Extra.", "A text.", "B text.", "C text."]);
    let none = align_inferred(&target, &source, &[]);
    assert!(none.pairs.is_empty());
    let pin = Pin {
        target_id: "a".into(),
        target_sentence: 0,
        source_paragraph: 2,
        source_sentence: 0,
    };
    let al = align_inferred(&target, &source, &[pin]);
    let got: Vec<(&str, u32)> = al
        .pairs
        .iter()
        .map(|p| (p.target_id.as_str(), p.source_paragraph))
        .collect();
    assert_eq!(got, [("a", 2), ("b", 3), ("c", 4)]);
}

#[test]
fn claim_lines_pair_when_counts_match() {
    let target = [
        para("h", "【청구항 3】", None),
        para("c0", "장치로서,", None),
        para("c1", "기판;", None),
        para("c2", "상기 기판 상의 전극; 및", None),
        para("c3", "절연층을 포함하는, 장치.", None),
    ];
    let source = strings(&[
        "What is claimed is:",
        "A device comprising:",
        "a substrate;",
        "an electrode; and",
        "an insulating layer.",
    ]);
    // 원문 청구항 번호는 1 부터 매겨지므로 3 번 항으로 만들려면 번호를 적는다.
    let source = {
        let mut s = source;
        s[1] = "3. A device comprising:".into();
        s
    };
    let al = align_inferred(&target, &source, &[]);
    assert_eq!(al.pairs.len(), 4);
    assert!(al.pairs.iter().all(|p| p.grade == Grade::Sentence));
    let first = &al.pairs[0];
    assert_eq!(
        (first.target_id.as_str(), first.source_paragraph),
        ("c0", 1)
    );

    let mut fewer = source.clone();
    fewer.pop();
    let al = align_inferred(&target, &fewer, &[]);
    assert_eq!(al.pairs.len(), 1);
    assert_eq!(al.pairs[0].grade, Grade::Paragraph);
}

#[test]
fn object_only_paragraphs_get_nothing() {
    let mut figure = para("f", "_", None);
    figure.has_objects = true;
    let target = [figure, para("a", "글이다.", Some("0001"))];
    let al = align_inferred(&target, &strings(&["[0001] Text."]), &[]);
    assert_eq!(al.target_sentences.len(), 1);
    assert_eq!(al.target_sentences[0].0, "a");
}
