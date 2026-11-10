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

// ---- 실물 정답지(XLIFF)와 견주다 나온 꼴 ----

#[test]
fn a_claim_numbered_ten_or_more_pairs_by_lines_despite_its_own_number() {
    // 원문 청구항 첫 줄의 「12.」 는 항 번호이지 숫자 증거가 아니다 — 역문에는 없다.
    let target = [
        para("h", "【청구항 12】", None),
        para("c0", "장치로서,", None),
        para("c1", "기판(110);", None),
    ];
    let source = strings(&[
        "What is claimed is:",
        "12. A device comprising:",
        "a substrate 110;",
    ]);
    // 번호 열쇠가 1 부터 이어져야 12 번이 잡히므로, 앞 11 개 항을 채운다.
    let mut full = vec!["What is claimed is:".to_string()];
    for n in 1..12 {
        full.push(format!("{n}. A thing."));
    }
    full.extend(source[1..].iter().cloned());
    let mut tgt: Vec<EditorParagraph> = Vec::new();
    for n in 1..12 {
        tgt.push(para(&format!("h{n}"), &format!("【청구항 {n}】"), None));
        tgt.push(para(&format!("b{n}"), "물건.", None));
    }
    tgt.extend(target);
    let al = align_inferred(&tgt, &full, &[]);
    assert_eq!(
        pair_summary(&al, "c0"),
        [(Some(0), 12, Some(0), Grade::Sentence)]
    );
    assert_eq!(pair_summary(&al, "c1").len(), 1);
}

#[test]
fn a_claim_numbered_ten_or_more_still_demotes_on_different_numerals() {
    let mut full = vec!["What is claimed is:".to_string()];
    for n in 1..12 {
        full.push(format!("{n}. A thing."));
    }
    full.push("12. A device comprising:".into());
    full.push("a substrate 130;".into());
    let mut tgt: Vec<EditorParagraph> = Vec::new();
    for n in 1..12 {
        tgt.push(para(&format!("h{n}"), &format!("【청구항 {n}】"), None));
        tgt.push(para(&format!("b{n}"), "물건.", None));
    }
    tgt.push(para("h", "【청구항 12】", None));
    tgt.push(para("c0", "장치로서,", None));
    tgt.push(para("c1", "기판(110);", None));
    let al = align_inferred(&tgt, &full, &[]);
    assert!(
        al.pairs
            .iter()
            .filter(|p| p.target_id == "c0" || p.target_id == "c1")
            .all(|p| p.grade == Grade::Paragraph)
    );
}

#[test]
fn leading_figure_numbers_must_agree_before_a_sentence_pair() {
    // 한 칸 밀린 도면 설명(한 자리 번호는 숫자 증거가 아니다)이 문장 짝으로 둔갑하면 안 된다.
    let target = [
        para("a", "도 1은 장치의 도면이다.", Some("0001")),
        para("b", "도 2는 시스템의 도면이다.", Some("0002")),
    ];
    let shifted = strings(&[
        "[0001] FIG. 2 shows a view of the device.",
        "[0002] FIG. 3 shows a view of the system.",
    ]);
    let al = align_inferred(&target, &shifted, &[]);
    assert!(al.pairs.iter().all(|p| p.grade != Grade::Sentence));
    let right = strings(&[
        "[0001] FIG. 1 shows a view of the device.",
        "[0002] FIG. 2 shows a view of the system.",
    ]);
    let al = align_inferred(&target, &right, &[]);
    assert_eq!(al.pairs.len(), 2);
    assert!(al.pairs.iter().all(|p| p.grade == Grade::Sentence));
}

// ---- 고침 1차 보완 ----

#[test]
fn only_the_first_unit_of_a_claim_loses_its_leading_number() {
    // 한 문단 안 둘째 줄 맨 앞 「13.」 은 본문 — 증거로 남아 역문(없음)과 어긋나면 내려간다.
    let mut full = vec!["What is claimed is:".to_string()];
    for n in 1..12 {
        full.push(format!("{n}. A thing."));
    }
    full.push("12. A device comprising:\n13. a layer;".into());
    let mut tgt: Vec<EditorParagraph> = Vec::new();
    for n in 1..12 {
        tgt.push(para(&format!("h{n}"), &format!("【청구항 {n}】"), None));
        tgt.push(para(&format!("b{n}"), "물건.", None));
    }
    tgt.push(para("h", "【청구항 12】", None));
    tgt.push(para("c0", "장치로서,", None));
    tgt.push(para("c1", "층;", None));
    let al = align_inferred(&tgt, &full, &[]);
    assert!(
        al.pairs
            .iter()
            .filter(|p| p.target_id == "c0" || p.target_id == "c1")
            .all(|p| p.grade == Grade::Paragraph)
    );
}

#[test]
fn a_label_on_only_one_side_does_not_demote() {
    // 「도 1을 참조하면」 ↔ 「Referring to FIG. 1」 — 원문은 낱말로 시작하지 않는다.
    let target = [para("a", "도 1을 참조하면 장치가 보인다.", Some("0001"))];
    let al = align_inferred(
        &target,
        &strings(&["[0001] Referring to FIG. 1, the device is seen."]),
        &[],
    );
    assert_eq!(al.pairs[0].grade, Grade::Sentence);
    // 반대: 원문만 「FIG. 2」 로 시작하고 역문은 낱말이 아니다.
    let target = [para("a", "장치가 도시되어 있다.", Some("0001"))];
    let al = align_inferred(&target, &strings(&["[0001] FIG. 2 shows the device."]), &[]);
    assert_eq!(al.pairs[0].grade, Grade::Sentence);
}

#[test]
fn a_figure_label_without_its_number_demotes() {
    let target = [para("a", "도 1은 장치의 도면이다.", Some("0001"))];
    let al = align_inferred(
        &target,
        &strings(&["[0001] FIG. shows a view of the device."]),
        &[],
    );
    assert!(al.pairs.iter().all(|p| p.grade != Grade::Sentence));
    let al = align_inferred(
        &target,
        &strings(&["[0001] FIGS. and show views of the device."]),
        &[],
    );
    assert!(al.pairs.iter().all(|p| p.grade != Grade::Sentence));
}

#[test]
fn fig_is_a_whole_word_check() {
    let target = [para("a", "도 1은 비유적 표현이다.", Some("0001"))];
    let al = align_inferred(
        &target,
        &strings(&["[0001] FIGURATIVE language is used."]),
        &[],
    );
    assert_eq!(al.pairs[0].grade, Grade::Sentence);
}

#[test]
fn claim_lines_are_exempt_from_the_label_check() {
    let target = [
        para("h", "【청구항 3】", None),
        para("c0", "장치로서,", None),
        para("c1", "도 1의 기판;", None),
    ];
    let source = strings(&[
        "What is claimed is:",
        "3. A device comprising:",
        "fig. 2 a substrate;",
    ]);
    let al = align_inferred(&target, &source, &[]);
    assert_eq!(al.pairs.len(), 2);
    assert!(al.pairs.iter().all(|p| p.grade == Grade::Sentence));
}

fn gap_fixture() -> ([EditorParagraph; 5], Vec<String>) {
    // 표제 닻 둘 사이에 문단 셋씩 — 문장 수는 전부 둘, 숫자 · 원문 식별번호 없음.
    let mut target = vec![para("h1", "【기술분야】", None)];
    let mut source = vec!["Technical Field".to_string()];
    for id in ["x", "y", "z"] {
        target.push(para(id, "가 문장이다. 나 문장이다.", None));
        source.push("First sentence. Second sentence.".to_string());
    }
    target.push(para("h2", "【배경기술】", None));
    source.push("Background".to_string());
    (target.try_into().ok().unwrap(), source)
}

#[test]
fn order_filled_paragraphs_without_numerals_keep_sentence_pairs() {
    // 닻 사이를 차례로 채운 문단은 문장 수가 같고 문장별 숫자 관문을 지나면(증거가 비어도) 문장 짝.
    let (target, source) = gap_fixture();
    let al = align_inferred(&target, &source, &[]);
    for id in ["x", "y", "z"] {
        let p = pair_summary(&al, id);
        assert_eq!(p.len(), 2, "{id}");
        assert!(p.iter().all(|q| q.3 == Grade::Sentence), "{id}");
    }
}

#[test]
fn order_filled_paragraphs_with_the_same_numerals_get_sentence_pairs() {
    let target = [
        para("h1", "【기술분야】", None),
        para("x", "장치(110)가 있다. 층이 있다.", None),
        para("y", "기판(120)이 있다. 전극이 있다.", None),
        para("h2", "【배경기술】", None),
    ];
    let source = strings(&[
        "Technical Field",
        "A device 110 exists. A layer exists.",
        "A substrate 120 exists. An electrode exists.",
        "Background",
    ]);
    let al = align_inferred(&target, &source, &[]);
    for id in ["x", "y"] {
        let p = pair_summary(&al, id);
        assert_eq!(p.len(), 2, "{id}");
        assert!(p.iter().all(|q| q.3 == Grade::Sentence), "{id}");
    }
    // 한쪽이 비었거나 다르면 문단 짝.
    let other = strings(&[
        "Technical Field",
        "A device 110 exists. A layer exists.",
        "A substrate 130 exists. An electrode exists.",
        "Background",
    ]);
    let al = align_inferred(&target, &other, &[]);
    assert_eq!(pair_summary(&al, "x").len(), 2);
    assert_eq!(pair_summary(&al, "y"), [(None, 2, None, Grade::Paragraph)]);
}

#[test]
fn directly_anchored_paragraphs_keep_sentence_pairs_without_numerals() {
    let target = [
        para("a", "가 문장이다. 나 문장이다.", Some("0001")),
        para("b", "다 문장이다. 라 문장이다.", Some("0002")),
    ];
    let source = strings(&[
        "[0001] First one. Second one.",
        "[0002] Third one. Fourth one.",
    ]);
    let al = align_inferred(&target, &source, &[]);
    assert_eq!(al.pairs.len(), 4);
    assert!(al.pairs.iter().all(|p| p.grade == Grade::Sentence));
}

#[test]
fn a_pinned_paragraph_keeps_sentence_pairs_without_numerals() {
    let (target, mut source) = gap_fixture();
    source.insert(1, "Extra.".into()); // 개수가 어긋나 차례 채움이 안 선다 — 핀이 닻
    let pin = Pin {
        target_id: "x".into(),
        target_sentence: 0,
        source_paragraph: 2,
        source_sentence: 0,
    };
    let al = align_inferred(&target, &source, &[pin]);
    let p = pair_summary(&al, "x");
    assert_eq!(p.len(), 2);
    assert!(p.iter().all(|q| q.3 == Grade::Sentence));
}

// ---- 차례로 매긴 짝 이름 닻: 구간별 개수가 같을 때만 직접 닻 ----

/// 상세설명 본문 문단(식별번호 저장 없음 → 짝 이름은 차례) + 원문 문단(`[000N]` 인쇄).
fn body(ids: &[&str], texts: &[&str], stored: bool) -> Vec<EditorParagraph> {
    let mut v = vec![para("d", "【발명의 설명】", None)];
    for (i, (id, t)) in ids.iter().zip(texts).enumerate() {
        v.push(para(
            id,
            t,
            stored.then(|| format!("{:04}", i + 1)).as_deref(),
        ));
    }
    v
}

fn printed(texts: &[&str]) -> Vec<String> {
    texts
        .iter()
        .enumerate()
        .map(|(i, t)| format!("[{:04}] {t}", i + 1))
        .collect()
}

const TWO: &str = "가 문장이다. 나 문장이다.";
const TWO_EN: &str = "First one. Second one.";

#[test]
fn ordinal_keys_are_weak_where_the_counts_differ() {
    // 편집 문서 본문 6, 원문 인쇄 번호 5 — 차례 번호가 어디선가 밀렸다.
    let ids = ["a", "b", "c", "d2", "e", "f"];
    let t = body(
        &ids,
        &[TWO, TWO, "장치(110)가 있다. 층이 있다.", TWO, TWO, TWO],
        false,
    );
    let s = printed(&[
        TWO_EN,
        TWO_EN,
        "A device 110 exists. A layer exists.",
        TWO_EN,
        TWO_EN,
    ]);
    let al = align_inferred(&t, &s, &[]);
    assert_eq!(pair_summary(&al, "a"), [(None, 0, None, Grade::Paragraph)]);
    assert_eq!(pair_summary(&al, "b"), [(None, 1, None, Grade::Paragraph)]);
    // 같은 도면부호가 문단 양쪽에 있으면 문장 짝.
    assert_eq!(pair_summary(&al, "c").len(), 2);
    assert!(
        pair_summary(&al, "c")
            .iter()
            .all(|q| q.3 == Grade::Sentence)
    );
}

#[test]
fn ordinal_keys_stay_direct_where_the_counts_agree() {
    let ids = ["a", "b", "c", "d2", "e"];
    let t = body(&ids, &[TWO; 5], false);
    let al = align_inferred(&t, &printed(&[TWO_EN; 5]), &[]);
    for id in ids {
        assert_eq!(pair_summary(&al, id).len(), 2, "{id}");
        assert!(
            pair_summary(&al, id).iter().all(|q| q.3 == Grade::Sentence),
            "{id}"
        );
    }
}

#[test]
fn stored_identification_numbers_are_always_direct() {
    let ids = ["a", "b", "c", "d2", "e", "f"];
    let t = body(&ids, &[TWO; 6], true);
    let al = align_inferred(&t, &printed(&[TWO_EN; 5]), &[]);
    for id in ["a", "b", "c", "d2", "e"] {
        assert!(
            pair_summary(&al, id).iter().all(|q| q.3 == Grade::Sentence),
            "{id}"
        );
        assert_eq!(pair_summary(&al, id).len(), 2, "{id}");
    }
}

#[test]
fn only_the_span_whose_counts_differ_is_weakened() {
    // 유일 표제가 구간을 가른다 — 앞 구간은 3 ↔ 3, 뒤 구간은 4 ↔ 3.
    let mut t = vec![para("d", "【발명의 설명】", None)];
    for id in ["a1", "a2", "a3"] {
        t.push(para(id, TWO, None));
    }
    t.push(para("h", "【도면의 간단한 설명】", None));
    for id in ["b1", "b2", "b3", "b4"] {
        t.push(para(id, TWO, None));
    }
    let mut s: Vec<String> = Vec::new();
    for n in 1..=3 {
        s.push(format!("[{n:04}] {TWO_EN}"));
    }
    s.push("Brief Description of Drawings".into());
    for n in 4..=6 {
        s.push(format!("[{n:04}] {TWO_EN}"));
    }
    let al = align_inferred(&t, &s, &[]);
    for id in ["a1", "a2", "a3"] {
        assert!(
            pair_summary(&al, id).iter().all(|q| q.3 == Grade::Sentence),
            "{id}"
        );
        assert_eq!(pair_summary(&al, id).len(), 2, "{id}");
    }
    for id in ["b1", "b2", "b3"] {
        assert_eq!(pair_summary(&al, id).len(), 1, "{id}");
        assert_eq!(pair_summary(&al, id)[0].3, Grade::Paragraph, "{id}");
    }
}
