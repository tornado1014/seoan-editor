use thiserror::Error;

/// Convert XML millimetres to the native HLT image display unit measured in `align_a.HLT`.
///
/// The saved-HLT pixel pitch is 0.282 mm, represented without floating point as
/// `(millimetres * 1000 + 141) / 282`.
pub fn image_display_units_from_mm(millimetres: u32) -> u64 {
    (u64::from(millimetres) * 1_000 + 141) / 282
}

/// Derive the secondary image display field with saved-HLT half-up rounding.
pub fn secondary_image_display_units(native_units: u64) -> u64 {
    let quotient = native_units / 4;
    let remainder = native_units % 4;
    quotient * 3 + (3 * remainder + 2) / 4
}

/// Only the four top-level document anchors are centered in NKEditor-saved HLT.
/// 네이티브 셀 폭 → KIPO XML `colwidth`. 11쌍이 전부 **올림**이다.
/// ⛔ 반올림으로 쓰면 1785 가 2231 로 나와 정본의 2232 와 어긋난다.
pub fn cell_width_to_colwidth(width: u32) -> u32 {
    u64::from(width)
        .saturating_mul(5)
        .div_ceil(4)
        .try_into()
        .unwrap_or(u32::MAX)
}

/// KIPO XML `colwidth` → 네이티브 셀 폭. 통특기 **불러오기**가 실제로 하는 일이고,
/// 위 `cell_width_to_colwidth` 의 역이 **아니다**. 두 단계 내림이다.
///
/// 통특기(v6.3) 39쌍(폭 600~7856)으로 유일하게 확정했다.
///
/// ⛔ 예전 상수 `× 4 / 5` 는 **내보내기의 역**이었다. 앞의 11쌍은 한 문서 안의
/// (네이티브, 그 문서가 내보낸 colwidth) 이라 역방향이 저절로 맞았을 뿐이다. 통특기는
/// 자기 산출물을 다시 들여와도 제자리로 안 돌아온다(noop 네이티브 2797 → colwidth 3497
/// → 들여오기 2826, 약 1.04% 확대). 이 규칙은 그 증가를 그대로 재현한다.
///
/// ⚠️ 가로병합이 있는 행의 열은 여기서 +2~+5 벗어난다. 미해결.
pub fn colwidth_to_cell_width(colwidth: u32) -> u32 {
    let shrunk = u64::from(colwidth).saturating_mul(97) / 100;
    (shrunk.saturating_mul(5) / 6)
        .try_into()
        .unwrap_or(u32::MAX)
}

pub fn is_centered_top_level_anchor(title: &str) -> bool {
    matches!(
        title,
        "【발명의 설명】" | "【청구범위】" | "【요약서】" | "【도면】"
    )
}

/// Input needed to populate the measured merge-related fields of one 80-byte cell record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellWriteSpec {
    pub start: u32,
    pub width: u32,
    pub base_height: u32,
    pub rowspan: u32,
    pub colspan: u32,
    pub continuation: bool,
}

/// Saved-HLT values for fields 4, 5, 12, 16, 17, and 18 of a cell record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellWriteFields {
    pub start: u32,
    pub width: u32,
    pub height: u32,
    pub span_flag: u32,
    pub rowspan: u32,
    pub colspan: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FormatRuleError {
    #[error("rowspan and colspan must both be at least 1")]
    EmptySpan,
    #[error("rowspan {rowspan} needs {needed} following row heights, got {actual}")]
    MissingRowHeights {
        rowspan: u32,
        needed: usize,
        actual: usize,
    },
    #[error("merged cell height exceeds the HLT u32 field")]
    HeightOverflow,
}

impl CellWriteSpec {
    /// Apply the saved-HLT merge contract without serializing a complete HLT document.
    ///
    /// A continuation record uses flag 56 and height 0. An anchor uses flag 32; its
    /// height is its own height plus the following `rowspan - 1` row heights. HLT tables
    /// terminate through row linkage (`next = 0xffffffff`), not an extra sentinel row.
    pub fn fields(self, following_row_heights: &[u32]) -> Result<CellWriteFields, FormatRuleError> {
        if self.rowspan == 0 || self.colspan == 0 {
            return Err(FormatRuleError::EmptySpan);
        }
        let needed = self.rowspan.saturating_sub(1) as usize;
        if following_row_heights.len() < needed {
            return Err(FormatRuleError::MissingRowHeights {
                rowspan: self.rowspan,
                needed,
                actual: following_row_heights.len(),
            });
        }
        let height = if self.continuation {
            0
        } else {
            following_row_heights[..needed]
                .iter()
                .try_fold(self.base_height, |height, row_height| {
                    height.checked_add(*row_height)
                })
                .ok_or(FormatRuleError::HeightOverflow)?
        };
        Ok(CellWriteFields {
            start: self.start,
            width: self.width,
            height,
            span_flag: if self.continuation { 56 } else { 32 },
            rowspan: self.rowspan,
            colspan: self.colspan,
        })
    }
}
