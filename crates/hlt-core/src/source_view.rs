//! 원문과 나란히 보기: 영문 원문(.docx) 문단과 편집 중인 문서 문단에 같은 짝 이름(`p:0001` · `claim:3`)을 단다.
//! 상세설명은 식별번호(원문 `[0001]`), 청구항은 번호로 짝짓는다. 짝 이름이 없는 문단은 앞 문단을 따른다(화면 몫).

use std::path::Path;

use serde::Serialize;

use crate::{CoreError, EditorParagraph};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceParagraph {
    pub key: Option<String>,
    pub text: String,
}

/// 영문 원문 .docx → 문단 + 짝 이름.
pub fn source_paragraphs(path: &Path) -> Result<Vec<SourceParagraph>, CoreError> {
    let document = crate::inspect_docx_path(path)?.document;
    let texts: Vec<&str> = document
        .paragraphs
        .iter()
        .map(|p| p.text.as_str())
        .collect();
    Ok(source_keys(&texts)
        .into_iter()
        .zip(&texts)
        .map(|(key, text)| SourceParagraph {
            key,
            text: text.to_string(),
        })
        .collect())
}

/// 원문 문단 글 → 짝 이름.
/// - `[0001]` · `[001]`(앞 0 을 채워 넷) · `[0012a]` → `p:0001`.
/// - 청구항 절(「Claims」 · 「What is claimed is:」 …) 안에서 「1.」로 시작하거나(번호), 대문자로 시작하는 문단이 새 항
///   (자동 번호 목록은 글에 번호가 없다 — 이어지는 줄은 소문자로 시작한다). 「Abstract」에서 끝난다.
pub fn source_keys(texts: &[&str]) -> Vec<Option<String>> {
    const CLAIMS: [&str; 7] = [
        "claims",
        "claim",
        "what is claimed is",
        "we claim",
        "i claim",
        "the invention claimed is",
        "claimed is",
    ];
    let (mut in_claims, mut next) = (false, 1_u32);
    texts
        .iter()
        .map(|text| {
            let text = text.trim();
            let lower = text.trim_end_matches(':').to_lowercase();
            if CLAIMS.contains(&lower.as_str()) {
                (in_claims, next) = (true, 1);
                return None;
            }
            if lower.starts_with("abstract") && text.len() < 40 {
                in_claims = false;
                return None;
            }
            if in_claims {
                if lower.starts_with("what is claimed") || lower.starts_with("we claim") {
                    return None;
                }
                let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
                let after = text[digits.len()..].chars().next();
                if !digits.is_empty() && matches!(after, Some('.' | ')')) {
                    next = digits.parse().unwrap_or(next);
                } else if !text.chars().next().is_some_and(char::is_uppercase) {
                    return None;
                }
                next += 1;
                return Some(format!("claim:{}", next - 1));
            }
            let inner = text.strip_prefix('[')?.split_once(']')?.0;
            let digits: String = inner.chars().take_while(char::is_ascii_digit).collect();
            let rest = &inner[digits.len()..];
            ((3..=5).contains(&digits.len()) && rest.chars().all(|c| c.is_ascii_lowercase()))
                .then(|| format!("p:{digits:0>4}{rest}"))
        })
        .collect()
}

/// 편집 중인 문서 문단 → 짝 이름: 청구항(표제 · 본문 줄) = `claim:N`, 상세설명 = 식별번호(최종본) 또는
/// 통특기가 매길 차례(그냥 HLT — 번호는 제출파일을 만들 때 매긴다).
pub fn paragraph_keys(paragraphs: &[EditorParagraph]) -> Vec<Option<String>> {
    let mut keys = vec![None; paragraphs.len()];
    if paragraphs.iter().any(|p| p.number.is_some()) {
        for (key, paragraph) in keys.iter_mut().zip(paragraphs) {
            *key = paragraph
                .number
                .as_deref()
                .filter(|number| *number != "+")
                .map(|number| format!("p:{number}"));
        }
    } else {
        for (ordinal, at) in crate::amend_marks::description_body(paragraphs)
            .into_iter()
            .enumerate()
        {
            keys[at] = Some(format!("p:{:04}", ordinal + 1));
        }
    }
    let texts: Vec<&str> = paragraphs.iter().map(|p| p.text.as_str()).collect();
    for (number, heading, end) in crate::editor::claim_blocks(&texts) {
        for key in &mut keys[heading..end] {
            *key = Some(format!("claim:{number}"));
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_keys_read_bracket_numbers_and_claim_starts() {
        let texts = [
            "TITLE",
            "[0001]Embodiments of the invention.",
            "[002] Three digits.",
            "[0012a] Lettered.",
            "[Note] not a number",
            "CLAIMS",
            "What is claimed as new is:",
            "A process comprising:",
            "heating a feed; and",
            "The process of claim 1, wherein.",
            "",
            "5. The process of claim 1.",
            "The process of claim 5.",
            "ABSTRACT",
            "A process is disclosed.",
        ];
        let owned = source_keys(&texts);
        let keys: Vec<Option<&str>> = owned.iter().map(|k| k.as_deref()).collect();
        assert_eq!(
            keys,
            [
                None,
                Some("p:0001"),
                Some("p:0002"),
                Some("p:0012a"),
                None,
                None,
                None,
                Some("claim:1"),
                None,
                Some("claim:2"),
                None,
                Some("claim:5"),
                Some("claim:6"),
                None,
                None,
            ]
        );
    }

    #[test]
    fn paragraph_keys_follow_numbers_or_planned_order_and_claims() {
        let paragraph = |text: &str, number: Option<&str>| EditorParagraph {
            id: String::new(),
            text: text.into(),
            wipo_id: None,
            number: number.map(str::to_string),
            has_objects: false,
            spans: Vec::new(),
        };
        let plain = [
            paragraph("【발명의 설명】", None),
            paragraph("【발명의 명칭】", None),
            paragraph("장치", None),
            paragraph("【기술분야】", None),
            paragraph("가", None),
            paragraph("나", None),
            paragraph("【청구범위】", None),
            paragraph("【청구항 1】", None),
            paragraph("장치.", None),
            paragraph("둘째 줄", None),
        ];
        let keys: Vec<Option<String>> = paragraph_keys(&plain);
        let keys: Vec<Option<&str>> = keys.iter().map(|k| k.as_deref()).collect();
        assert_eq!(
            keys,
            [
                None,
                None,
                None,
                None,
                Some("p:0001"),
                Some("p:0002"),
                None,
                Some("claim:1"),
                Some("claim:1"),
                Some("claim:1")
            ]
        );
        let numbered = [paragraph("가", Some("0007")), paragraph("새", Some("+"))];
        let keys = paragraph_keys(&numbered);
        assert_eq!(keys, [Some("p:0007".to_string()), None]);
    }
}
