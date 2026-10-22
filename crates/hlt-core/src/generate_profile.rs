use std::borrow::Cow;

use crate::HLT_HEADER_LEN;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationDialect {
    PatentSpecificationKo,
    PctPatent,
}

#[derive(Debug, Clone)]
pub struct GenerationProfile {
    pub header: [u8; HLT_HEADER_LEN as usize],
    pub pre_body_gap: Cow<'static, [u8]>,
    pub shapes: Cow<'static, [u8]>,
    pub shape_order: Cow<'static, [u8]>,
    pub geometry: Cow<'static, [u8]>,
    pub second_shape_order: Cow<'static, [u8]>,
    pub style_table: Cow<'static, [u8]>,
    pub ending: Cow<'static, [u8]>,
    pub shape_count: u32,
    pub fallback_shape: u32,
    /// 원본 틀 프로필(`source_generation_profile`)만 싣는다: 원본 문단모양 (들여쓰기, 정렬, 줄 간격, 번호).
    /// 있으면 생성기가 내장 지도로 고른 모양을 이 지도의 같은 모양으로 바꿔 단다.
    pub shape_map: Option<Vec<(i32, u32, u32, u32)>>,
}

const ENDING: &[u8] = &[
    0xbf, 0x03, 0x00, 0x00, 0x00, 0x04, 0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0xbf, 0x02, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0xbf, 0x91, 0x48, 0x04, 0x41, 0x2c,
    0x16, 0x3b, 0x41, 0x00, 0x20, 0xb1, 0x44, 0x00, 0xa0, 0x8d, 0x44, 0x00, 0xc0, 0x0d, 0x45, 0x00,
    0xa0, 0x8d, 0x44, 0xba,
];

pub fn generation_profile(dialect: GenerationDialect) -> GenerationProfile {
    let mut header = *include_bytes!("../assets/generation/builtin_header.bin");
    match dialect {
        GenerationDialect::PatentSpecificationKo => {
            // ⛔ 내장 헤더는 우리 탐침 파일에서 왔고 종류에 맨 140 이 들어 있다.
            // 실물 국내 출원서 44건은 `0x8000` 표시를 달고 있고, 그 44건이
            // 쓰는 값은 `0x8000 + 140` = 32908 이다.
            header[0x20..0x24].copy_from_slice(&32_908_u32.to_le_bytes());
            GenerationProfile {
                header,
                pre_body_gap: Cow::Borrowed(include_bytes!(
                    "../assets/generation/builtin_pre_body_gap.bin"
                )),
                shapes: Cow::Borrowed(include_bytes!("../assets/generation/builtin_shapes.bin")),
                shape_order: Cow::Borrowed(include_bytes!(
                    "../assets/generation/builtin_shape_order.bin"
                )),
                geometry: Cow::Borrowed(include_bytes!(
                    "../assets/generation/builtin_geometry.bin"
                )),
                second_shape_order: Cow::Borrowed(include_bytes!(
                    "../assets/generation/builtin_second_shape_order.bin"
                )),
                style_table: Cow::Borrowed(include_bytes!(
                    "../assets/generation/builtin_style_table.bin"
                )),
                ending: Cow::Borrowed(ENDING),
                // 2026-09-06: 모양 12 = 표 안 가운데(0, 1, 130, field7=2). 통특기 실물
                // center_cell.HLT(noop_v15 표 1 셀 가운데 정렬 저장)가 만든 레코드 그대로.
                shape_count: 13,
                fallback_shape: 2,
                shape_map: None,
            }
        }
        GenerationDialect::PctPatent => {
            header[0x20..0x24].copy_from_slice(&32_881_u32.to_le_bytes());
            GenerationProfile {
                header,
                pre_body_gap: Cow::Borrowed(include_bytes!(
                    "../assets/generation/pct_pre_body_gap.bin"
                )),
                shapes: Cow::Borrowed(include_bytes!("../assets/generation/pct_shapes.bin")),
                shape_order: Cow::Borrowed(include_bytes!(
                    "../assets/generation/pct_shape_order.bin"
                )),
                geometry: Cow::Borrowed(include_bytes!("../assets/generation/pct_geometry.bin")),
                second_shape_order: Cow::Borrowed(include_bytes!(
                    "../assets/generation/pct_second_shape_order.bin"
                )),
                style_table: Cow::Borrowed(include_bytes!(
                    "../assets/generation/pct_style_table.bin"
                )),
                ending: Cow::Borrowed(ENDING),
                // 2026-09-06 밤: 모양 22 = 표 안 가운데(0, 1, 130). 통특기는 셀을 가운데 정렬할 때
                // 그 셀의 모양을 복사해 정렬만 바꾼다(실물 2건: noop f7=2 상속, pct_a f7=0 상속) →
                // 이 프로필의 셀 모양 14 를 복사했다.
                shape_count: 23,
                fallback_shape: 4,
                shape_map: None,
            }
        }
    }
}

/// 문서 종류 111 = 최종본을 불러온 보정 명세서(통특기 「최종본 불러오기」 · 국내). 식별번호(`<p num>`)를 HLT 에 싣는다.
pub const AMENDMENT_DOCUMENT_TYPE: u32 = 111;

/// 보정 명세서 틀: 국내 내장 틀 + 종류 111(`0x8000 + 111` = 32879 · 통특기 「최종본 불러오기」 산출과 같은 값).
/// 생성기는 이 종류일 때만 식별번호를 쓴다(통특기가 내장 틀 문서에 매달린 번호를 「문단 정보가 없습니다」로 거절했다 —
/// 종류 111 문서는 통특기가 경고 없이 연다 · 2026-10-22 실측).
pub fn amendment_profile() -> GenerationProfile {
    let mut profile = generation_profile(GenerationDialect::PatentSpecificationKo);
    profile.header[0x20..0x24].copy_from_slice(&(0x8000 + AMENDMENT_DOCUMENT_TYPE).to_le_bytes());
    profile
}
