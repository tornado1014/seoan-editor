//! 번호 다시 매기기: 청구항(【청구항 N】 · 「제N항」 · 「청구항 N」)과 도면(【도 N】 · 「도 N」)을 나온 차례로 1부터.
//! 바꿀 문단 목록(전 · 후 글)만 낸다 — 미리 보여 준 뒤 [`crate::EditorSession::replace_texts`] 로 한 번에 건다.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::EditorParagraph;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenumberEdit {
    pub id: String,
    pub before: String,
    pub after: String,
}

/// 번호 뒤에 이어 쓰는 연결어(「제1항 내지 제3항」 · 「도 2a 및 2b」).
const LINKS: [&str; 9] = [",", "또는", "내지", "~", "및", "과", "와", "-", "·"];

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Claim,
    Figure,
}

/// 청구항: 【청구항 N】 표제를 나온 차례로 1..k, 청구항 본문의 인용을 같은 대응으로. 이미 차례면 빈 목록.
/// 대응에 없는 번호(지운 항 · 없는 항)는 그대로 둔다(인용 검사가 잡는다).
pub fn claim_renumber(paragraphs: &[EditorParagraph]) -> Vec<RenumberEdit> {
    let texts: Vec<&str> = paragraphs.iter().map(|p| p.text.as_str()).collect();
    let blocks = crate::editor::claim_blocks(&texts);
    let mut map = BTreeMap::new();
    for (k, (number, ..)) in blocks.iter().enumerate() {
        map.entry(*number).or_insert(k as u32 + 1);
    }
    if blocks
        .iter()
        .enumerate()
        .all(|(k, (number, ..))| *number == k as u32 + 1)
    {
        return Vec::new();
    }
    let mut edits = Vec::new();
    for (k, (_, heading, end)) in blocks.iter().enumerate() {
        let paragraph = &paragraphs[*heading];
        let after = format!("【청구항 {}】", k + 1);
        if paragraph.text.trim() != after {
            edits.push(edit(paragraph, after));
        }
        for paragraph in &paragraphs[heading + 1..*end] {
            let after = rewrite(&paragraph.text, &map, Kind::Claim);
            if after != paragraph.text {
                edits.push(edit(paragraph, after));
            }
        }
    }
    edits
}

/// 도면: 【도면】 절 【도 N(a)】 표제의 기본 번호를 나온 차례로 1..k(붙은 글자는 그대로 · 도 2a · 2b → 3a · 3b),
/// 문서 전체의 「도 N」 언급을 같은 대응으로. 이미 차례면 빈 목록.
pub fn figure_renumber(paragraphs: &[EditorParagraph]) -> Vec<RenumberEdit> {
    let Some(drawings) = paragraphs.iter().position(|p| p.text.trim() == "【도면】") else {
        return Vec::new();
    };
    let mut map = BTreeMap::new();
    for paragraph in &paragraphs[drawings + 1..] {
        let Some(name) = paragraph
            .text
            .trim()
            .strip_prefix("【도 ")
            .and_then(|rest| rest.strip_suffix('】'))
        else {
            continue;
        };
        let digits: String = name.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(number) = digits.parse::<u32>() {
            let next = map.len() as u32 + 1;
            map.entry(number).or_insert(next);
        }
    }
    if map.iter().all(|(old, new)| old == new) {
        return Vec::new();
    }
    paragraphs
        .iter()
        .filter_map(|paragraph| {
            let after = rewrite(&paragraph.text, &map, Kind::Figure);
            (after != paragraph.text).then(|| edit(paragraph, after))
        })
        .collect()
}

fn edit(paragraph: &EditorParagraph, after: String) -> RenumberEdit {
    RenumberEdit {
        id: paragraph.id.clone(),
        before: paragraph.text.clone(),
        after,
    }
}

fn starts(chars: &[char], at: usize, word: &str) -> bool {
    word.chars()
        .enumerate()
        .all(|(k, ch)| chars.get(at + k) == Some(&ch))
}

fn spaces(chars: &[char], mut at: usize) -> usize {
    while chars.get(at).is_some_and(|ch| *ch == ' ') {
        at += 1;
    }
    at
}

/// 숫자 자리(시작 · 끝) — 숫자가 없으면 `None`.
fn digits(chars: &[char], at: usize) -> Option<(usize, usize)> {
    let end = (at..chars.len())
        .find(|k| !chars[*k].is_ascii_digit())
        .unwrap_or(chars.len());
    (end > at).then_some((at, end))
}

fn hangul(ch: char) -> bool {
    ('가'..='힣').contains(&ch)
}

/// 인용 첫머리: 청구항 = 「제 N 항」 · 「청구항 N」, 도면 = 앞이 한글 음절 · 글자가 아닌 「도 N」. 숫자 자리와 그 뒤 자리.
fn anchor(chars: &[char], at: usize, kind: Kind) -> Option<((usize, usize), usize)> {
    match kind {
        Kind::Claim if starts(chars, at, "청구항") => {
            let number = digits(chars, spaces(chars, at + 3))?;
            Some((number, after_claim(chars, number.1)))
        }
        Kind::Claim if chars[at] == '제' => {
            let number = digits(chars, spaces(chars, at + 1))?;
            let end = spaces(chars, number.1);
            (chars.get(end) == Some(&'항')).then_some((number, end + 1))
        }
        Kind::Figure
            if chars[at] == '도'
                && !at
                    .checked_sub(1)
                    .and_then(|before| chars.get(before))
                    .is_some_and(|ch| hangul(*ch) || ch.is_alphanumeric()) =>
        {
            let number = digits(chars, spaces(chars, at + 1))?;
            Some((number, after_figure(chars, number.1)))
        }
        _ => None,
    }
}

/// 청구항 번호 뒤 「항」(있으면).
fn after_claim(chars: &[char], end: usize) -> usize {
    let next = spaces(chars, end);
    if chars.get(next) == Some(&'항') {
        next + 1
    } else {
        end
    }
}

/// 도면 번호 뒤 붙은 글자(3a · 3A).
fn after_figure(chars: &[char], end: usize) -> usize {
    (end..chars.len())
        .find(|k| !chars[*k].is_ascii_alphabetic())
        .unwrap_or(chars.len())
}

/// 글 안 인용 번호를 대응대로 바꾼다(첫머리 + 연결어로 이어진 번호들).
fn rewrite(text: &str, map: &BTreeMap<u32, u32>, kind: Kind) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut numbers: Vec<(usize, usize)> = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        let Some((number, mut end)) = anchor(&chars, at, kind) else {
            at += 1;
            continue;
        };
        numbers.push(number);
        // 이어진 번호: 연결어 · (「제」 · 「청구항」 · 「도」) · 번호.
        loop {
            let mut next = spaces(&chars, end);
            let Some(link) = LINKS.iter().find(|link| starts(&chars, next, link)) else {
                break;
            };
            next = spaces(&chars, next + link.chars().count());
            next = match kind {
                Kind::Claim if chars.get(next) == Some(&'제') => spaces(&chars, next + 1),
                Kind::Claim if starts(&chars, next, "청구항") => spaces(&chars, next + 3),
                Kind::Figure if chars.get(next) == Some(&'도') => spaces(&chars, next + 1),
                _ => next,
            };
            let Some(more) = digits(&chars, next) else {
                break;
            };
            numbers.push(more);
            end = match kind {
                Kind::Claim => after_claim(&chars, more.1),
                Kind::Figure => after_figure(&chars, more.1),
            };
        }
        at = end.max(at + 1);
    }
    let mut out = String::new();
    let mut last = 0;
    for (start, end) in numbers {
        out.extend(&chars[last..start]);
        let old: String = chars[start..end].iter().collect();
        last = end;
        let Some(new) = old.parse::<u32>().ok().and_then(|n| map.get(&n)) else {
            out.push_str(&old);
            continue;
        };
        out.push_str(&new.to_string());
        // 번호 바로 뒤 조사는 새 번호 끝소리에 맞춘다(「도 3과」 → 「도 2와」).
        if let Some((length, particle)) = particle(&chars, end, *new) {
            out.push_str(particle);
            last = end + length;
        }
    }
    out.extend(&chars[last..]);
    out
}

/// 숫자 읽기 끝소리: (받침이 있다, 그 받침이 ㄹ). 일(ㄹ) 이 삼 사 오 육 칠(ㄹ) 팔(ㄹ) 구 · 십 · 백 · 천.
fn final_sound(number: u32) -> (bool, bool) {
    match number % 10 {
        0 => (true, false),
        1 | 7 | 8 => (true, true),
        3 | 6 => (true, false),
        _ => (false, false),
    }
}

/// `at` 의 조사(받침 짝이 있는 것)를 새 번호에 맞춘 꼴로: (옛 조사 글자 수, 새 조사). 「이」 · 「가」는 뒤가 끊길 때만
/// (「도 3이다」의 「이」는 서술격이라 그대로).
fn particle(chars: &[char], at: usize, number: u32) -> Option<(usize, &'static str)> {
    let (batchim, rieul) = final_sound(number);
    let ends = |k: usize| chars.get(k).is_none_or(|ch| !hangul(*ch));
    let pairs: [(&str, &str, &str); 4] = [
        ("으로", "로", if batchim && !rieul { "으로" } else { "로" }),
        ("은", "는", if batchim { "은" } else { "는" }),
        ("을", "를", if batchim { "을" } else { "를" }),
        ("과", "와", if batchim { "과" } else { "와" }),
    ];
    for (with, without, right) in pairs {
        for word in [with, without] {
            if starts(chars, at, word) {
                return Some((word.chars().count(), right));
            }
        }
    }
    for word in ["이", "가"] {
        if starts(chars, at, word) && ends(at + 1) {
            return Some((1, if batchim { "이" } else { "가" }));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(texts: &[&str]) -> Vec<EditorParagraph> {
        texts
            .iter()
            .enumerate()
            .map(|(at, text)| EditorParagraph {
                id: format!("p{at}"),
                text: text.to_string(),
                wipo_id: None,
                number: None,
                has_objects: false,
                spans: Vec::new(),
            })
            .collect()
    }

    fn after(edits: &[RenumberEdit], id: &str) -> Option<String> {
        edits.iter().find(|e| e.id == id).map(|e| e.after.clone())
    }

    #[test]
    fn claims_follow_heading_order_and_rewrite_citations() {
        let paragraphs = doc(&[
            "【청구범위】",
            "【청구항 1】",
            "장치.",
            "【청구항 3】",
            "제1항에 있어서, 부재.",
            "【청구항 4】",
            "제1항 내지 제3항 중 어느 한 항에 있어서, 청구항 3 및 4의 부재.",
            "【청구항 5】",
            "청구항 3 내지 4 중 어느 하나, 또는 제9항에 있어서, 제5단계.",
        ]);
        let edits = claim_renumber(&paragraphs);
        assert_eq!(after(&edits, "p3").as_deref(), Some("【청구항 2】"));
        assert_eq!(after(&edits, "p4"), None, "제1항은 그대로");
        assert_eq!(after(&edits, "p5").as_deref(), Some("【청구항 3】"));
        assert_eq!(
            after(&edits, "p6").as_deref(),
            Some("제1항 내지 제2항 중 어느 한 항에 있어서, 청구항 2 및 3의 부재.")
        );
        assert_eq!(
            after(&edits, "p8").as_deref(),
            Some("청구항 2 내지 3 중 어느 하나, 또는 제9항에 있어서, 제5단계."),
            "없는 항(9) · 「항」이 없는 「제5단계」는 그대로"
        );
        assert_eq!(after(&edits, "p7").as_deref(), Some("【청구항 4】"));
        // 이미 차례면 바꿀 것이 없다.
        let ordered = doc(&["【청구항 1】", "장치.", "【청구항 2】", "제1항에 있어서."]);
        assert!(claim_renumber(&ordered).is_empty());
    }

    #[test]
    fn figures_keep_letters_skip_words_and_follow_links() {
        let paragraphs = doc(&[
            "도 1 및 3a는 온도 10에서 본 도 3b이다(도 5 참조).",
            "도 1 내지 3과 같이, 도 5는 도 3a이다. 도 5가 보인다. 온도 3에서 도 3이다.",
            "【대표도】",
            "도 3a",
            "【도면】",
            "【도 1】",
            "_",
            "【도 3a】",
            "【도 3b】",
            "【도 5】",
        ]);
        let edits = figure_renumber(&paragraphs);
        assert_eq!(
            after(&edits, "p0").as_deref(),
            Some("도 1 및 2a는 온도 10에서 본 도 2b이다(도 3 참조)."),
            "「온도 10」은 낱말"
        );
        assert_eq!(
            after(&edits, "p1").as_deref(),
            Some("도 1 내지 2와 같이, 도 3은 도 2a이다. 도 3이 보인다. 온도 3에서 도 2이다."),
            "조사는 새 번호 끝소리로 · 붙은 글자 뒤 · 서술격 「이다」는 그대로"
        );
        assert_eq!(after(&edits, "p3").as_deref(), Some("도 2a"));
        assert_eq!(after(&edits, "p7").as_deref(), Some("【도 2a】"));
        assert_eq!(after(&edits, "p9").as_deref(), Some("【도 3】"));
        assert_eq!(after(&edits, "p5"), None);
        let ordered = doc(&["도 1", "【도면】", "【도 1】", "【도 2a】", "【도 2b】"]);
        assert!(figure_renumber(&ordered).is_empty());
    }
}
