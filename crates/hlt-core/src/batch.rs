//! 폴더 일괄 처리: 폴더 안 워드 · XML · HML 을 하나씩 HLT 로 만들기 · HLT · 최종본마다 제출 전 점검.
//! 파일은 차례로 하나씩(세션은 바로 놓는다). `step(차례, 전체, 파일 이름)` 이 `false` 를 주면 멈춘다(남은 파일은 「멈춤」).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::{EditorSession, GenerationDialect, SubmitKind, WriteOptions};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BatchRow {
    pub file: String,
    /// 만듦 · 있음(같은 이름 HLT 가 이미 있어 건너뜀) · 실패 · 멈춤.
    pub status: String,
    /// 만든 HLT 이름 · 바꾼 글자 수 · 실패 까닭.
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BatchCheckRow {
    pub file: String,
    /// 종류별 걸린 수(없는 종류는 빠진다).
    pub counts: BTreeMap<SubmitKind, usize>,
    /// 열지 못했거나 멈췄으면 그 까닭.
    pub error: Option<String>,
}

/// 폴더에서 이 확장자들(대소문자 무시)인 파일, 이름 차례.
fn files(folder: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            path.extension().is_some_and(|ext| {
                extensions
                    .iter()
                    .any(|wanted| ext.eq_ignore_ascii_case(wanted))
            })
        })
        .collect();
    out.sort();
    out
}

fn name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn reason(error: &crate::CoreError) -> String {
    match error.details.get("reason").map(String::as_str) {
        // XML 은 그림을 옆 파일로 부른다(실물: XML 만 옮기면 이렇게 실패한다).
        Some("image_source") => "그림 파일을 찾지 못함(XML 옆에 그림 파일이 있어야 한다)".into(),
        Some(reason) => format!("{} · {reason}", error.code),
        None => error.code.to_string(),
    }
}

/// 폴더 안 `.docx` · `.xml` · `.hml` 을 같은 이름 `.HLT` 로 옆에 만든다. 같은 이름 HLT 가 있으면 덮지 않고 「있음」.
/// 쓸 수 없는 글자는 자동 대응으로 바꾸고 그 수를 적는다(대응 없는 글자가 있으면 「실패」).
pub fn batch_generate_hlt(
    folder: &Path,
    dialect: GenerationDialect,
    mut step: impl FnMut(usize, usize, &str) -> bool,
) -> Vec<BatchRow> {
    let inputs = files(folder, &["docx", "xml", "hml"]);
    let total = inputs.len();
    let mut stopped = false;
    inputs
        .iter()
        .enumerate()
        .map(|(at, input)| {
            let file = name(input);
            stopped = stopped || !step(at, total, &file);
            let row = |status: &str, detail: String| BatchRow {
                file: file.clone(),
                status: status.into(),
                detail,
            };
            if stopped {
                return row("멈춤", String::new());
            }
            let output = input.with_extension("HLT");
            if output.exists() {
                return row("있음", name(&output));
            }
            let made =
                crate::load_generation_input(input).and_then(|(document, payloads, replaced)| {
                    crate::save_generated_hlt_as_new(
                        input,
                        &document,
                        dialect,
                        &payloads,
                        &output,
                        WriteOptions::default(),
                    )
                    .map(|_| replaced.len())
                });
            match made {
                Ok(0) => row("만듦", name(&output)),
                Ok(replaced) => row(
                    "만듦",
                    format!("{} · 글자 {replaced}개 바꿈", name(&output)),
                ),
                Err(error) => row("실패", reason(&error)),
            }
        })
        .collect()
}

/// 폴더 안 `.HLT` · `.fin` 마다 제출 전 점검(파일은 사본으로 연다 — 원본은 안 건드린다).
pub fn batch_submission_checks(
    folder: &Path,
    mut step: impl FnMut(usize, usize, &str) -> bool,
) -> Vec<BatchCheckRow> {
    let inputs = files(folder, &["hlt", "fin"]);
    let total = inputs.len();
    let mut stopped = false;
    inputs
        .iter()
        .enumerate()
        .map(|(at, input)| {
            let file = name(input);
            stopped = stopped || !step(at, total, &file);
            let mut row = BatchCheckRow {
                file,
                counts: BTreeMap::new(),
                error: None,
            };
            if stopped {
                row.error = Some("멈춤".into());
                return row;
            }
            match EditorSession::open(input) {
                Ok(session) => {
                    for issue in session.submission_checks() {
                        *row.counts.entry(issue.kind).or_default() += 1;
                    }
                }
                Err(error) => row.error = Some(reason(&error)),
            }
            row
        })
        .collect()
}
