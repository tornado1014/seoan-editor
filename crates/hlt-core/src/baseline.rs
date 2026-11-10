//! 비교 기준으로 쓰는 워드(.docx) — HLT 로 굽지 않고 문단 글만 읽는다.

use std::path::Path;

use crate::{CoreError, EditorParagraph, inspect_docx_path};

/// 워드를 비교 기준 문단으로 — 글머리의 `[0001]` · `【0001】` 은 떼어 `number` 로(청구항 번호는 글 그대로).
/// 그림 서명은 없다(워드는 그림 바이트를 HLT 와 같은 길로 못 견준다).
pub fn docx_baseline(path: &Path) -> Result<Vec<EditorParagraph>, CoreError> {
    let document = inspect_docx_path(path)?;
    Ok(document
        .document
        .paragraphs
        .iter()
        .enumerate()
        .map(|(at, paragraph)| {
            let (number, text) = split_number(&paragraph.text);
            EditorParagraph {
                id: format!("word{at}"),
                text,
                wipo_id: None,
                number,
                has_objects: !paragraph.image_ids.is_empty() || paragraph.table_id.is_some(),
                spans: Vec::new(),
            }
        })
        .collect())
}

fn split_number(text: &str) -> (Option<String>, String) {
    let trimmed = text.trim_start();
    for (open, close) in [('[', ']'), ('【', '】')] {
        let Some(rest) = trimmed.strip_prefix(open) else {
            continue;
        };
        let Some((digits, tail)) = rest.split_once(close) else {
            continue;
        };
        if digits.len() == 4 && digits.chars().all(|c| c.is_ascii_digit()) {
            return (Some(digits.to_string()), tail.trim_start().to_string());
        }
    }
    (None, text.to_string())
}
