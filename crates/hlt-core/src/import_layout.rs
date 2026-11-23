//! 들여온 Word(.docx) · HML 원고의 본문 문단에 KIPO 표준 문단 모양(첫 줄 들여쓰기 + 양쪽 정렬)을 입힌다.
//!
//! 사용자 요청(2026-11-23): 출원서 서식(【발명의 설명】 · 【청구범위】 …)은 갖췄지만 들여쓰기나
//! 양쪽 정렬이 빠진 워드 · 한글 원고도 불러올 때 표준 모양으로 맞춘다.
//!
//! 표준 값은 KIPO XML 입력([`crate::inspect_kipo_xml_path`])이 본문에 주는 값과 같다:
//! 양쪽 정렬 · 들여쓰기 1000(워드 첫 줄 800 dxa · HLT 파일 값 800).
//!
//! 손대지 않는 것:
//! - 서식을 알 수 없는 원고(출원서 표제가 하나도 없음) — 일반 문서는 그대로 둔다.
//! - 표제 문단(【…】) — 절 표제 모양은 [`crate::plan_hlt_body`] 가 정한다.
//! - 표를 품은 문단 · 표 칸 — 표 칸 모양은 따로 정한다.
//! - 가운데 · 오른쪽 정렬 문단 — 수식 · 도면 · 「관련 출원」 같은 소제목은 일부러 그렇게 둔 것이다.
//! - 이미 들여쓰기(또는 내어쓰기 · 왼쪽 여백)가 있는 문단의 들여쓰기 값 — 정렬만 양쪽으로 맞춘다.
//! - 그림만 든 문단의 정렬 — KIPO XML 도 그림 문단은 왼쪽 정렬로 둔다(들여쓰기만 맞춘다).

use crate::HmlDocument;

/// KIPO 본문 문단의 첫 줄 들여쓰기(서안 단위). KIPO XML 입력의 본문 값과 같다.
pub const KIPO_BODY_INDENT_RAW: i32 = 1000;

/// 출원서 서식으로 볼 표제들(괄호 뺀 이름).
const KIPO_FORM_HEADINGS: &[&str] = &[
    "발명의 설명",
    "고안의 설명",
    "명세서",
    "발명의 명칭",
    "고안의 명칭",
    "기술분야",
    "청구범위",
    "요약서",
];

/// 원고가 출원서 서식을 따르는지 — 표준 표제가 하나라도 절로 잡혔는지 본다.
pub fn looks_like_kipo_form(document: &HmlDocument) -> bool {
    document.sections.iter().any(|section| {
        let title = section.title.trim().trim_matches(['【', '】', ' ']);
        KIPO_FORM_HEADINGS.contains(&title)
    })
}

/// 출원서 서식 원고의 본문 문단에 표준 들여쓰기 · 양쪽 정렬을 입힌다. 바꾼 문단 수를 돌려준다.
/// 서식이 아니면 아무것도 바꾸지 않는다(0).
pub fn apply_kipo_body_layout(document: &mut HmlDocument) -> usize {
    if !looks_like_kipo_form(document) {
        return 0;
    }
    let mut changed = 0;
    for paragraph in &mut document.paragraphs {
        if paragraph.marker.is_some()
            || paragraph.table_id.is_some()
            || paragraph.section_id.is_none()
            || !paragraph.source_line_bytes.is_empty()
        {
            continue;
        }
        if matches!(paragraph.align.as_str(), "center" | "right") {
            continue;
        }
        let image_only = !paragraph.image_ids.is_empty() && paragraph.text.trim().is_empty();
        let mut touched = false;
        if !image_only && paragraph.align != "justified" {
            paragraph.align = "justified".to_string();
            touched = true;
        }
        if paragraph.indent_raw == 0 && paragraph.lmargin_raw == 0 {
            paragraph.indent_raw = KIPO_BODY_INDENT_RAW;
            touched = true;
        }
        if touched {
            changed += 1;
        }
    }
    changed
}
