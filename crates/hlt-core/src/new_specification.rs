//! 새 명세서(통특기 「국내출원 새문서」): 종류(특허 · 실용신안 · PCT특허 국내진입) + 선택사항(도면 포함 · 청구범위 제출유예 ·
//! 산업상 이용가능성). 바탕은 통특기 2026.11.05 판이 새문서로 만든 빈 명세서 그대로이고, 선택사항은 통특기와 같게
//! 표제 묶음을 빼거나(도면 · 청구범위) 작성지침 자리에 넣는다(산업상 이용가능성).

use std::path::Path;

use crate::{CoreError, EditorSession, WriteOptions};

const PATENT: &[u8] = include_bytes!("../assets/forms/NewPatent.HLT");
const UTILITY: &[u8] = include_bytes!("../assets/forms/NewUtility.HLT");
const PCT: &[u8] = include_bytes!("../assets/forms/NewPct.HLT");

/// 새 명세서 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecKind {
    Patent,
    Utility,
    Pct,
}

/// 새문서 선택사항(통특기 체크 상자). 실용신안은 도면이 늘 있고 PCT 는 제출유예가 없다(통특기와 같다 — 무시한다).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
pub struct SpecOptions {
    pub drawings: bool,
    pub deferred: bool,
    pub industrial: bool,
}

/// 도면을 빼면 같이 빠지는 표제(통특기 실측 — 【부호의 설명】도 함께).
const DRAWING_HEADINGS: [&str; 4] = ["도면의 간단한 설명", "부호의 설명", "대표도", "도면"];

/// `output` 에 새 명세서를 굽는다(있으면 거부).
pub fn save_new_specification(
    output: &Path,
    kind: SpecKind,
    options: SpecOptions,
) -> Result<(), CoreError> {
    let drawings = options.drawings || kind == SpecKind::Utility;
    let deferred = options.deferred && kind != SpecKind::Pct;
    let work = tempfile::tempdir().map_err(|_| new_error("work"))?;
    let form = work.path().join("form.HLT");
    // 서식으로 막 구운 새 문서는 종류 0 이다 — 통특기가 새문서에 쓰는 종류를 다시 단다(특허 140 · 실용신안 142 · PCT 113).
    let (bytes, document_kind) = match kind {
        SpecKind::Patent => (PATENT, 140),
        SpecKind::Utility => (UTILITY, 142),
        SpecKind::Pct => (PCT, 113),
    };
    std::fs::write(&form, bytes).map_err(|_| new_error("form"))?;
    if drawings && !deferred && !options.industrial {
        return crate::save::save_new_document_with_kind(
            &form,
            output,
            WriteOptions::default(),
            Some(document_kind),
        )
        .map(|_| ());
    }
    let base = work.path().join("base.HLT");
    crate::save::save_new_document_with_kind(
        &form,
        &base,
        WriteOptions::default(),
        Some(document_kind),
    )?;
    let mut session = EditorSession::open(&base)?;
    let mut removed: Vec<&str> = Vec::new();
    if !drawings {
        removed.extend(DRAWING_HEADINGS);
        removed.push("도");
    }
    if deferred {
        removed.extend(["청구범위", "청구항"]);
    }
    if !removed.is_empty() {
        session.remove_headings(&removed)?;
    }
    if options.industrial {
        session.insert_standard_heading("산업상 이용가능성")?;
    }
    session.save_as(output)
}

fn new_error(reason: &str) -> CoreError {
    let mut error = CoreError::new(crate::ErrorCode::IoError, "new_specification", false);
    error.details.insert("reason".into(), reason.into());
    error
}
