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

fn pair_summary(al: &hlt_core::Alignment, id: &str) -> Vec<(Option<u32>, u32, Option<u32>, Grade)> {
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
}

#[test]
fn more_abbreviations_do_not_split() {
    let t = "See U.S. Pat. No. 5,000,000 for details. Done.";
    assert_eq!(sentences(t, Lang::En, false).len(), 2);
    let t = "As in Appl. Ser. No. 12 and Sec. 4 (Eq. 2) of vol. 3, pp. 4-5 and Ref. 7. Done.";
    assert_eq!(sentences(t, Lang::En, false).len(), 2);
}

#[test]
fn shifted_boundaries_with_equal_counts_demote_to_a_paragraph_pair() {
    // 영문은 「Prof.」 뒤에서 하나 더 끊고(+1) 「B.」 뒤에서 하나 덜 끊어(-1) 개수는 같지만 경계가 어긋난다.
    let target = [para(
        "a",
        "김 교수의 5,000 문헌을 참조한다. 장치는 층 B 위에 있다. 동작한다. 끝이다.",
        Some("0001"),
    )];
    let source = strings(&[
        "[0001] See Prof. Kim 5,000 for details. The device is on layer B. It works. Done.",
    ]);
    assert_eq!(sentences(&source[0], Lang::En, false).len(), 4);
    let al = align_inferred(&target, &source, &[]);
    assert_eq!(pair_summary(&al, "a"), [(None, 0, None, Grade::Paragraph)]);
}

#[test]
fn swapped_reference_numerals_demote_to_a_paragraph_pair() {
    let target = [para(
        "a",
        "장치(100)는 기판(110)을 포함한다. 제1 전극(120)이 있다. 제2 전극(140)이 있다.",
        Some("0001"),
    )];
    let swapped = strings(&[
        "[0001] The device 100 includes a substrate 110. The first electrode 140 is there. The second electrode 120 is there.",
    ]);
    let al = align_inferred(&target, &swapped, &[]);
    assert_eq!(pair_summary(&al, "a"), [(None, 0, None, Grade::Paragraph)]);
    let same = strings(&[
        "[0001] The device 100 includes a substrate 110. The first electrode 120 is there. The second electrode 140 is there.",
    ]);
    let al = align_inferred(&target, &same, &[]);
    assert_eq!(al.pairs.len(), 3);
    assert!(al.pairs.iter().all(|p| p.grade == Grade::Sentence));
}

#[test]
fn sentences_without_numbers_give_no_evidence_either_way() {
    let target = [para("a", "가 문장이다. 나 문장이다.", Some("0001"))];
    let al = align_inferred(&target, &strings(&["[0001] First one. Second one."]), &[]);
    assert_eq!(al.pairs.len(), 2);
    assert!(al.pairs.iter().all(|p| p.grade == Grade::Sentence));
}

#[test]
fn ordinals_written_as_digits_are_ignored_but_numerals_match() {
    let target = [para(
        "a",
        "제1 전극(120)은 제2 전극(140) 위에 있다.",
        Some("0001"),
    )];
    let source = strings(&["[0001] The first electrode 120 is on the second electrode 140."]);
    let al = align_inferred(&target, &source, &[]);
    assert_eq!(
        pair_summary(&al, "a"),
        [(Some(0), 0, Some(0), Grade::Sentence)]
    );
}

#[test]
fn claim_lines_with_different_numerals_demote() {
    let target = [
        para("h", "【청구항 3】", None),
        para("c0", "기판(110);", None),
        para("c1", "전극(120).", None),
    ];
    let source = strings(&[
        "What is claimed is:",
        "3. A substrate 110;",
        "an electrode 130.",
    ]);
    let al = align_inferred(&target, &source, &[]);
    assert_eq!(al.pairs.len(), 1);
    assert_eq!(al.pairs[0].grade, Grade::Paragraph);
}

fn claim_fixture() -> ([EditorParagraph; 5], Vec<String>) {
    let target = [
        para("h", "【청구항 3】", None),
        para("c0", "장치로서,", None),
        para("c1", "기판;", None),
        para("c2", "전극; 및", None),
        para("c3", "절연층.", None),
    ];
    let source = strings(&[
        "What is claimed is:",
        "3. A device comprising:",
        "a substrate;",
        "an electrode; and",
        "an insulating layer.",
        "a spacer.",
    ]);
    (target, source)
}

#[test]
fn a_pin_inside_a_claim_repairs_the_lines_around_it() {
    let (target, source) = claim_fixture();
    let none = align_inferred(&target, &source, &[]);
    assert_eq!(none.pairs.len(), 1);
    assert_eq!(none.pairs[0].grade, Grade::Paragraph);
    let pin = Pin {
        target_id: "c3".into(),
        target_sentence: 0,
        source_paragraph: 4,
        source_sentence: 0,
    };
    let al = align_inferred(&target, &source, &[pin]);
    let got: Vec<(&str, u32)> = al
        .pairs
        .iter()
        .map(|p| (p.target_id.as_str(), p.source_paragraph))
        .collect();
    assert_eq!(got, [("c0", 1), ("c1", 2), ("c2", 3), ("c3", 4)]);
    assert!(al.pairs.iter().all(|p| p.grade == Grade::Sentence));
    assert!(al.dropped_pins.is_empty());
}

#[test]
fn a_pin_joining_different_claims_is_dropped() {
    let target = [
        para("h3", "【청구항 3】", None),
        para("a", "가;", None),
        para("b", "나.", None),
        para("h4", "【청구항 4】", None),
        para("c", "다;", None),
        para("d", "라.", None),
    ];
    let source = strings(&["What is claimed is:", "3. Aa;", "bb.", "4. Cc;", "dd."]);
    let pin = Pin {
        target_id: "a".into(),
        target_sentence: 0,
        source_paragraph: 3,
        source_sentence: 0,
    };
    let al = align_inferred(&target, &source, std::slice::from_ref(&pin));
    assert_eq!(al.dropped_pins, [pin]);
    assert_eq!(
        pair_summary(&al, "a"),
        [(Some(0), 1, Some(0), Grade::Sentence)]
    );
}

#[test]
fn unusable_pins_are_reported_and_make_no_pair() {
    let target = [
        para("a", "가 문장이다.", None),
        para("b", "나 문장이다.", None),
        para("c", "다 문장이다.", None),
    ];
    let source = strings(&["X text.", "Y text.", "Z text.", "W text."]);
    let pin = |id: &str, ts, sp, ss| Pin {
        target_id: id.into(),
        target_sentence: ts,
        source_paragraph: sp,
        source_sentence: ss,
    };
    let good = pin("a", 0, 2, 0);
    let order_conflict = pin("b", 0, 1, 0);
    let out_of_range = pin("c", 5, 3, 0);
    let unknown = pin("zz", 0, 3, 0);
    let al = align_inferred(
        &target,
        &source,
        &[
            good.clone(),
            order_conflict.clone(),
            out_of_range.clone(),
            unknown.clone(),
        ],
    );
    assert_eq!(al.dropped_pins, [order_conflict, out_of_range, unknown]);
    assert_eq!(
        pair_summary(&al, "a"),
        [(Some(0), 2, Some(0), Grade::Sentence)]
    );
    assert!(pair_summary(&al, "b").iter().all(|p| p.1 != 1));
    assert!(pair_summary(&al, "c").iter().all(|p| p.1 != 3));
}
