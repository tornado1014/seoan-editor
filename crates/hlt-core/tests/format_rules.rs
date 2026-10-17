use hlt_core::{
    CellWriteSpec, image_display_units_from_mm, is_centered_top_level_anchor,
    secondary_image_display_units,
};

#[test]
fn image_display_units_match_saved_hlt_pixel_pitch_and_half_up_secondary_size() {
    assert_eq!(image_display_units_from_mm(120), 426);
    assert_eq!(image_display_units_from_mm(22), 78);
    assert_eq!(secondary_image_display_units(78), 59);
}

#[test]
fn top_level_anchor_alignment_matches_nkeditor_saved_hlt() {
    for title in ["【발명의 설명】", "【청구범위】", "【요약서】", "【도면】"]
    {
        assert!(is_centered_top_level_anchor(title));
    }
    assert!(!is_centered_top_level_anchor("【표 1】"));
    assert!(!is_centered_top_level_anchor("【발명의 명칭】"));
}

#[test]
fn merged_cell_fields_match_saved_hlt_records() {
    let vertical_anchor = CellWriteSpec {
        start: 0,
        width: 2_987,
        base_height: 104,
        rowspan: 2,
        colspan: 1,
        continuation: false,
    }
    .fields(&[117])
    .unwrap();
    assert_eq!(vertical_anchor.start, 0);
    assert_eq!(vertical_anchor.width, 2_987);
    assert_eq!(vertical_anchor.height, 221);
    assert_eq!(vertical_anchor.span_flag, 32);
    assert_eq!(vertical_anchor.rowspan, 2);
    assert_eq!(vertical_anchor.colspan, 1);

    let horizontal_anchor = CellWriteSpec {
        start: 2_987,
        width: 4_063,
        base_height: 104,
        rowspan: 1,
        colspan: 3,
        continuation: false,
    }
    .fields(&[])
    .unwrap();
    assert_eq!(horizontal_anchor.colspan, 3);

    let continuation = CellWriteSpec {
        start: 0,
        width: 2_987,
        base_height: 117,
        rowspan: 1,
        colspan: 1,
        continuation: true,
    }
    .fields(&[])
    .unwrap();
    assert_eq!(continuation.height, 0);
    assert_eq!(continuation.span_flag, 56);
}

#[test]
fn merged_cell_rejects_missing_row_heights() {
    let error = CellWriteSpec {
        start: 0,
        width: 1_000,
        base_height: 104,
        rowspan: 3,
        colspan: 1,
        continuation: false,
    }
    .fields(&[117])
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "rowspan 3 needs 2 following row heights, got 1"
    );
}

/// 통특기 제출 XML 에서 셀 폭과 `colwidth` 를 짝지은 11쌍. **내보내기(네이티브 → colwidth)** 는 올림이다.
/// 반올림으로 쓰면 11쌍 중 3쌍이 어긋난다.
///
/// ⛔ 여기서 역방향을 같이 못 박지 마라. 이 11쌍은 **한 문서 안의 (네이티브, 그 문서가
/// 내보낸 colwidth)** 라 역이 저절로 맞는다. 통특기 **불러오기**는 그 역이 아니다
/// (아래 `nkeditor_import_widths_are_two_stage_floors`). 2026-09-07 이전에는 이 단언이
/// `× 4 / 5` 라는 틀린 들여오기 규칙을 고정하고 있었다.
#[test]
fn kipo_column_width_export_matches_the_authoritative_xml() {
    let measured = [
        (2_778_u32, 3_473_u32),
        (2_188, 2_735),
        (2_186, 2_733),
        (2_144, 2_680),
        (1_785, 2_232),
        (1_789, 2_237),
        (2_083, 2_604),
        (1_389, 1_737),
        (2_250, 2_813),
        (3_600, 4_500),
        (3_510, 4_388),
    ];
    for (native, colwidth) in measured {
        assert_eq!(
            hlt_core::cell_width_to_colwidth(native),
            colwidth,
            "네이티브 {native}"
        );
    }
}

/// 통특기(v6.3) **불러오기** 실측 39쌍. `colwidth` → 네이티브 셀 폭은 두 단계 내림이다:
/// `floor(floor(colwidth * 97 / 100) * 5 / 6)`.
///
/// 표본: XML 들여오기 · 통특기가 낸 XML 되들여오기 · 폭을 흔든 탐색판(폭 600~7856
/// 탐색판, 같은 폭을 문서 안 세 자리에 두어 자리 무관·열 독립도 확인). 한 단계
/// `floor(colwidth * 97 / 120)` 은 39쌍 중 8쌍이 어긋난다 — 중간 내림이 있어야 맞는다.
/// 가로병합 행의 열은 이 규칙에서 +2~+5 벗어나므로(pct_a 표 1) 여기 넣지 않았다.
#[test]
fn nkeditor_import_widths_are_two_stage_floors() {
    let measured = [
        (600_u32, 485_u32),
        (700, 565),
        (800, 646),
        (900, 727),
        (1_000, 808),
        (1_100, 889),
        (1_200, 970),
        (1_300, 1_050),
        (1_400, 1_131),
        (1_401, 1_131),
        (1_443, 1_165),
        (1_457, 1_177),
        (1_487, 1_201),
        (1_500, 1_212),
        (1_523, 1_230),
        (1_530, 1_236),
        (1_560, 1_260),
        (1_590, 1_285),
        (1_600, 1_293),
        (1_620, 1_309),
        (1_667, 1_346),
        (1_700, 1_374),
        (1_736, 1_402),
        (1_757, 1_420),
        (1_800, 1_455),
        (2_000, 1_616),
        (2_500, 2_020),
        (2_880, 2_327),
        (3_000, 2_425),
        (3_161, 2_555),
        (3_162, 2_555),
        (3_173, 2_564),
        (3_415, 2_760),
        (3_417, 2_761),
        (3_428, 2_770),
        (3_497, 2_826),
        (4_628, 3_740),
        (4_629, 3_741),
        (7_856, 6_350),
    ];
    for (colwidth, native) in measured {
        assert_eq!(
            hlt_core::colwidth_to_cell_width(colwidth),
            native,
            "colwidth {colwidth}"
        );
    }
    // 통특기 자기 산출물의 왕복도 제자리로 안 돌아온다(noop 표 실측).
    assert_eq!(hlt_core::cell_width_to_colwidth(2_797), 3_497);
    assert_eq!(hlt_core::colwidth_to_cell_width(3_497), 2_826);
}
