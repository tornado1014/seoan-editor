use hlt_core::{ErrorCode, carry_character_codes, restore_automatic_line_breaks};

#[test]
fn carries_codes_like_python_sequence_matcher_without_autojunk() {
    let cases = [
        ("AB한C", vec![1, 2, 3, 4], "AB한C", 0, vec![1, 2, 3, 4]),
        ("ABC", vec![1, 2, 3], "ABxyC", 0, vec![1, 2, 2, 2, 3]),
        ("ABC", vec![1, 2, 3], "xyABC", 9, vec![1, 1, 1, 2, 3]),
        ("ABC", vec![1, 2, 3], "ABCxy", 0, vec![1, 2, 3, 3, 3]),
        ("ABCD", vec![1, 2, 3, 4], "AxyzD", 0, vec![1, 2, 3, 3, 4]),
        ("ABCDE", vec![1, 2, 3, 4, 5], "AXE", 0, vec![1, 2, 5]),
        ("ABCDE", vec![1, 2, 3, 4, 5], "ACE", 0, vec![1, 3, 5]),
        ("A_B", vec![1, 25, 3], "A_xB", 7, vec![1, 25, 7, 3]),
        ("_AB", vec![25, 2, 3], "x_AB", 7, vec![7, 25, 2, 3]),
        (
            "A\u{2063}B",
            vec![1, 25, 3],
            "A\u{2063}xB",
            7,
            vec![1, 25, 7, 3],
        ),
        ("A_B", vec![1, 25, 3], "AxB", 7, vec![1, 7, 3]),
    ];

    for (old, codes, new, plain, expected) in cases {
        assert_eq!(
            carry_character_codes(old, &codes, new, plain).unwrap(),
            expected,
            "{old:?} -> {new:?}"
        );
    }
}

#[test]
fn rejects_a_character_code_length_mismatch_without_panicking() {
    let error = carry_character_codes("ABC", &[1, 2], "ABC", 0).unwrap_err();

    assert_eq!(error.code, ErrorCode::ConversionFailed);
    assert_eq!(error.phase, "carry_codes");
}

#[test]
fn restores_automatic_breaks_at_python_sequence_matcher_positions() {
    let cases = [
        ("ABCDE", "ABxyCDE", "ABxyCDE"),
        ("AB\r\nCDE", "ABXCDE", "ABX\r\nCDE"),
        ("AB\r\nCDE", "AXYDE", "A\r\nXYDE"),
        ("AB\r\nCD\r\nEF", "A12BCDEF", "A12B\r\nCD\r\nEF"),
        ("ABCD\r\n", "ABXYZCD", "ABXYZCD\r\n"),
    ];

    for (old, new, expected) in cases {
        assert_eq!(
            restore_automatic_line_breaks(old, new).unwrap(),
            expected,
            "{old:?} -> {new:?}"
        );
    }
}

#[test]
fn replacement_display_text_rejects_hidden_structure_controls() {
    for replacement in ["A\rB", "A\nB", "A\0B", "A\u{0015}B"] {
        let error = restore_automatic_line_breaks("AB", replacement).unwrap_err();
        assert_eq!(error.code, ErrorCode::ConversionFailed);
        assert_eq!(error.phase, "line_breaks");
    }
}
