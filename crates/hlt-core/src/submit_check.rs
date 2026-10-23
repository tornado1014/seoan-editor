//! 제출 전 점검: 제출파일을 만들기 전에 사람이 눈으로 볼 것 — 빈 식별번호 · 도면 언급과 【도 N】 대응 ·
//! 【부호의 설명】에만 있는 부호 · 청구항 인용. 문단은 큰 절 표제로 나눈다(화면 `sectionOf` 와 같은 규칙).

use serde::Serialize;

use crate::{CitationIssue, CitationKind, EditorParagraph};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmitKind {
    /// 번호를 받아야 할 상세설명 단락에 식별번호가 없다.
    Unnumbered,
    /// 본문이 「도 N」을 말하는데 【도 N】 도면이 없다.
    FigureMissing,
    /// 【도 N】 도면이 있는데 본문이 한 번도 말하지 않는다.
    FigureUnmentioned,
    /// 【부호의 설명】에 있는데 상세설명 본문에 한 번도 안 나온다.
    SignUnused,
    /// 청구항 인용 문제(`claim_citation_issues`).
    Citation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SubmitIssue {
    pub kind: SubmitKind,
    /// 가 볼 문단(없으면 문서 전체의 일).
    pub paragraph_id: Option<String>,
    pub detail: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Description,
    Claims,
    Drawings,
    Other,
}

fn heading(text: &str) -> Option<&str> {
    let text = text.trim();
    text.strip_prefix('【')?
        .strip_suffix('】')
        .filter(|inner| !inner.contains('】'))
}

fn sections(paragraphs: &[EditorParagraph]) -> Vec<Section> {
    let mut section = Section::Other;
    paragraphs
        .iter()
        .map(|paragraph| {
            match heading(&paragraph.text) {
                Some("발명의 설명") => section = Section::Description,
                Some("청구범위" | "특허청구범위") => section = Section::Claims,
                Some("도면") => section = Section::Drawings,
                Some("요약서") => section = Section::Other,
                _ => {}
            }
            section
        })
        .collect()
}

/// 제출 전 점검(문서 차례대로 · 종류별).
pub fn submission_checks(
    paragraphs: &[EditorParagraph],
    citations: &[CitationIssue],
) -> Vec<SubmitIssue> {
    let sections = sections(paragraphs);
    let description: Vec<&EditorParagraph> = paragraphs
        .iter()
        .zip(&sections)
        .filter(|(_, section)| **section == Section::Description)
        .map(|(paragraph, _)| paragraph)
        .collect();
    let mut out = unnumbered(&description);
    out.extend(figure_issues(paragraphs, &sections, &description));
    out.extend(unused_signs(&description));
    out.extend(citations.iter().map(|issue| {
        let id = paragraphs
            .iter()
            .find(|p| heading(&p.text) == Some(format!("청구항 {}", issue.claim).as_str()))
            .map(|p| p.id.clone());
        SubmitIssue {
            kind: SubmitKind::Citation,
            paragraph_id: id,
            detail: citation_detail(issue),
        }
    }));
    out
}

fn citation_detail(issue: &CitationIssue) -> String {
    let cited = issue
        .cited
        .iter()
        .map(|n| format!("제{n}항"))
        .collect::<Vec<_>>()
        .join(", ");
    let what = match issue.kind {
        CitationKind::MisReference => "자기 항 · 뒤 항 · 없는 항을 인용",
        CitationKind::Removed => "지운 · 없는 청구항을 인용",
        CitationKind::DuplicateMultiple => "다중 인용 항을 다시 다중 인용(중복 다중참조)",
        CitationKind::NoAlternative => "「내지」 범위 뒤에 「어느」가 없다(택일어 미사용)",
        CitationKind::RangeMisReference => "범위 안에 자기 항 · 뒤 항 · 없는 항",
        CitationKind::AndCitation => "「및 · 과 · 와」로 묶어 인용(택일적이 아니다)",
        CitationKind::RangeNoAlternative => "범위 인용 뒤에 「어느」가 없다",
    };
    format!("청구항 {}: {what}({cited})", issue.claim)
}

/// 빈 식별번호: 상세설명의 본문 단락(표제 · 빈 문단 · 【발명의 명칭】 글 · 표 · 그림 문단 · 새 문단 「+」 말고) 가운데 번호가 없는 것.
/// 번호가 하나도 없으면 한 줄(문단번호 넣기를 아직 안 했다).
fn unnumbered(description: &[&EditorParagraph]) -> Vec<SubmitIssue> {
    let numbered = |p: &EditorParagraph| {
        p.number.as_deref().is_some_and(|n| !n.is_empty()) || p.wipo_id.is_some_and(|id| id > 0)
    };
    if !description.iter().any(|p| numbered(p)) {
        return description
            .iter()
            .any(|p| heading(&p.text).is_none() && !p.text.trim().is_empty())
            .then(|| SubmitIssue {
                kind: SubmitKind::Unnumbered,
                paragraph_id: None,
                detail: "식별번호가 하나도 없습니다 — 「문단번호 넣기」를 누르세요".into(),
            })
            .into_iter()
            .collect();
    }
    // 번호 값(최종본 번호 · 문단번호의 wipo_id) — 앞뒤 번호가 건너뛰면(0067 → 0069) 그 사이 문단은 번호를 가졌다
    // (실물: 표 글 문단 — 표 번호는 끝 줄에 있어 문단에 안 붙는다).
    let value = |p: &EditorParagraph| {
        p.number
            .as_deref()
            .and_then(|n| n.parse::<u32>().ok())
            .or(p.wipo_id.filter(|id| *id > 0))
    };
    let values: Vec<Option<u32>> = description.iter().map(|p| value(p)).collect();
    let gap = |at: usize| {
        let before = values[..at].iter().rev().find_map(|v| *v);
        let after = values[at + 1..].iter().find_map(|v| *v);
        matches!((before, after), (Some(a), Some(b)) if b > a.saturating_add(1))
    };
    let mut out = Vec::new();
    let (mut in_title, mut in_citations) = (false, false);
    for (at, p) in description.iter().enumerate() {
        if let Some(name) = heading(&p.text) {
            in_title = name == "발명의 명칭";
            // 【특허문헌】 · 【비특허문헌】 목록은 첫 줄만 번호를 받는다(실물 제출본 그대로).
            in_citations = matches!(name, "특허문헌" | "비특허문헌");
            continue;
        }
        if in_title
            || in_citations
            || gap(at)
            || p.text.trim().is_empty()
            || p.has_objects
            || p.number.as_deref() == Some("+")
            || numbered(p)
        {
            continue;
        }
        out.push(SubmitIssue {
            kind: SubmitKind::Unnumbered,
            paragraph_id: Some(p.id.clone()),
            detail: format!("식별번호 없음: {}", preview(&p.text)),
        });
    }
    out
}

fn preview(text: &str) -> String {
    let text = text.trim();
    let short: String = text.chars().take(30).collect();
    if short.len() < text.len() {
        format!("{short}…")
    } else {
        short
    }
}

/// 도면: 【도면】 절의 【도 N】 ↔ 상세설명의 「도 N」(목록 · 범위 포함). 【도면】 절이 없으면 건너뛴다.
fn figure_issues(
    paragraphs: &[EditorParagraph],
    sections: &[Section],
    description: &[&EditorParagraph],
) -> Vec<SubmitIssue> {
    let drawn: Vec<(String, &EditorParagraph)> = paragraphs
        .iter()
        .zip(sections)
        .filter(|(_, section)| **section == Section::Drawings)
        .filter_map(|(p, _)| {
            let name = heading(&p.text)?.strip_prefix("도 ")?;
            Some((name.trim().to_lowercase(), p))
        })
        .collect();
    if !sections.contains(&Section::Drawings) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut mentioned: Vec<String> = Vec::new();
    // 「도 3」은 【도 3a】 · 【도 3b】 … 묶음을, 「도 3a」는 한 장 【도 3】의 a 칸을 말한다(실물 둘 다).
    let sub = |long: &str, short: &str| {
        long.strip_prefix(short)
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_lowercase()))
    };
    let covers =
        |mention: &str, drawn: &str| mention == drawn || sub(drawn, mention) || sub(mention, drawn);
    for p in description {
        if heading(&p.text).is_some() {
            continue;
        }
        for name in figure_mentions(&p.text) {
            if !drawn.iter().any(|(drawn, _)| covers(&name, drawn))
                && !out.iter().any(|issue: &SubmitIssue| {
                    issue.detail
                        == format!("「도 {name}」을 말하는데 【도 {name}】 도면이 없습니다")
                })
            {
                out.push(SubmitIssue {
                    kind: SubmitKind::FigureMissing,
                    paragraph_id: Some(p.id.clone()),
                    detail: format!("「도 {name}」을 말하는데 【도 {name}】 도면이 없습니다"),
                });
            }
            mentioned.push(name);
        }
    }
    for (name, p) in &drawn {
        if !mentioned.iter().any(|mention| covers(mention, name)) {
            out.push(SubmitIssue {
                kind: SubmitKind::FigureUnmentioned,
                paragraph_id: Some(p.id.clone()),
                detail: format!("【도 {name}】을 본문에서 한 번도 말하지 않습니다"),
            });
        }
    }
    out
}

/// 「도 N」 언급(앞이 한글이 아닌 「도」 · 소문자) — 뒤따르는 목록 · 범위도: 「도 3a 및 3b」 · 「도 3 내지 5」 · 「도 1~3」 · 「도 2와 도 4」.
fn figure_mentions(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let token = |at: usize| -> (String, usize) {
        let name: String = chars[at.min(chars.len())..]
            .iter()
            .take_while(|ch| ch.is_ascii_alphanumeric())
            .collect();
        let len = name.chars().count();
        (name.to_lowercase(), at + len)
    };
    let skip_spaces = |mut at: usize| {
        while chars.get(at).is_some_and(|ch| ch.is_whitespace()) {
            at += 1;
        }
        at
    };
    let starts = |at: usize, word: &str| {
        word.chars()
            .enumerate()
            .all(|(k, ch)| chars.get(at + k) == Some(&ch))
    };
    let mut out = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        if chars[at] != '도' || at.checked_sub(1).is_some_and(|prev| is_hangul(chars[prev])) {
            at += 1;
            continue;
        }
        let (first, mut end) = token(skip_spaces(at + 1));
        if !first.starts_with(|ch: char| ch.is_ascii_digit()) {
            at += 1;
            continue;
        }
        out.push(first.clone());
        let mut last = first;
        // 목록 · 범위: 구분 말 + (「도」) + 번호.
        loop {
            let next = skip_spaces(end);
            let (range, sep_len) = if starts(next, "내지") {
                (true, 2)
            } else if matches!(chars.get(next), Some('~')) {
                (true, 1)
            } else if matches!(chars.get(next), Some(',' | '·' | '및' | '와' | '과')) {
                (false, 1)
            } else if starts(next, "또는") {
                (false, 2)
            } else {
                break;
            };
            let mut after = skip_spaces(next + sep_len);
            if chars.get(after) == Some(&'도') {
                after = skip_spaces(after + 1);
            }
            let (name, name_end) = token(after);
            if !name.starts_with(|ch: char| ch.is_ascii_digit()) {
                break;
            }
            if range {
                out.extend(expand(&last, &name));
            }
            out.push(name.clone());
            last = name;
            end = name_end;
        }
        at = end.max(at + 1);
    }
    out
}

/// 범위 사이 이름들(양 끝 말고): 「3 내지 5」 → 4 · 「3a 내지 3c」 → 3b. 모르는 꼴이면 없음.
fn expand(from: &str, to: &str) -> Vec<String> {
    if let (Ok(a), Ok(b)) = (from.parse::<u32>(), to.parse::<u32>()) {
        return (a + 1..b).take(200).map(|n| n.to_string()).collect();
    }
    let split = |s: &str| {
        let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
        let rest: String = s.chars().skip(digits.len()).collect();
        (digits, rest)
    };
    let ((da, ra), (db, rb)) = (split(from), split(to));
    let (ra, rb): (Vec<char>, Vec<char>) = (ra.chars().collect(), rb.chars().collect());
    match (ra.as_slice(), rb.as_slice()) {
        (&[a], &[b]) if da == db && a.is_ascii_lowercase() && a < b => (a as u8 + 1..b as u8)
            .map(|c| format!("{da}{}", c as char))
            .collect(),
        _ => Vec::new(),
    }
}

fn is_hangul(ch: char) -> bool {
    ('가'..='힣').contains(&ch)
}

/// 【부호의 설명】 아래 「10: 몸체」 꼴의 부호 가운데 상세설명의 다른 본문에 안 나오는 것.
fn unused_signs(description: &[&EditorParagraph]) -> Vec<SubmitIssue> {
    let mut signs: Vec<(String, &EditorParagraph)> = Vec::new();
    let mut body: Vec<&str> = Vec::new();
    let mut in_signs = false;
    for p in description {
        if let Some(name) = heading(&p.text) {
            in_signs = name == "부호의 설명";
            continue;
        }
        if in_signs {
            for sign in sign_labels(&p.text) {
                if !signs.iter().any(|(known, _)| *known == sign) {
                    signs.push((sign, p));
                }
            }
        } else {
            body.push(&p.text);
        }
    }
    signs
        .into_iter()
        .filter(|(sign, _)| !body.iter().any(|text| contains_sign(text, sign)))
        .map(|(sign, p)| SubmitIssue {
            kind: SubmitKind::SignUnused,
            paragraph_id: Some(p.id.clone()),
            detail: format!("부호 {sign}: 본문에 한 번도 나오지 않습니다"),
        })
        .collect()
}

/// 「10: 몸체 20 : 덮개 30a: …」 → 10 · 20 · 30a(콜론 앞 번호 · 앞이 숫자 · 영문이 아닌 것).
fn sign_labels(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    for (at, ch) in chars.iter().enumerate() {
        if !matches!(ch, ':' | '：') {
            continue;
        }
        let mut end = at;
        while end > 0 && chars[end - 1].is_whitespace() {
            end -= 1;
        }
        let mut start = end;
        while start > 0 && (chars[start - 1].is_ascii_alphanumeric() || chars[start - 1] == '\'') {
            start -= 1;
        }
        let sign: String = chars[start..end].iter().collect();
        if sign.starts_with(|c: char| c.is_ascii_digit()) {
            out.push(sign);
        }
    }
    out
}

/// 글에 부호가 따로 나오는가(앞뒤가 숫자 · 영문 · 「'」가 아니다 — 「100」 · 「10a」 안의 「10」은 아니다).
fn contains_sign(text: &str, sign: &str) -> bool {
    let boundary =
        |ch: Option<char>| !ch.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '\'');
    text.match_indices(sign).any(|(at, _)| {
        boundary(text[..at].chars().next_back()) && boundary(text[at + sign.len()..].chars().next())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(rows: &[(&str, Option<&str>)]) -> Vec<EditorParagraph> {
        rows.iter()
            .enumerate()
            .map(|(i, (text, number))| EditorParagraph {
                id: format!("p{i}"),
                text: (*text).into(),
                wipo_id: None,
                number: number.map(Into::into),
                has_objects: false,
                spans: Vec::new(),
            })
            .collect()
    }

    fn kinds(issues: &[SubmitIssue], kind: SubmitKind) -> Vec<(&str, &str)> {
        issues
            .iter()
            .filter(|i| i.kind == kind)
            .map(|i| (i.paragraph_id.as_deref().unwrap_or("-"), i.detail.as_str()))
            .collect()
    }

    #[test]
    fn figure_mentions_read_lists_and_ranges_but_not_words_ending_in_do() {
        let all = |text: &str| figure_mentions(text);
        assert_eq!(all("도 3a 및 3b에 보인"), ["3a", "3b"]);
        assert_eq!(all("도 3 내지 5는"), ["3", "4", "5"]);
        assert_eq!(all("도 3a 내지 3c"), ["3a", "3b", "3c"]);
        assert_eq!(all("도 1~3"), ["1", "2", "3"]);
        assert_eq!(all("도2와 도 4, 6을"), ["2", "4", "6"]);
        assert_eq!(all("도 1 또는 도 2"), ["1", "2"]);
        assert_eq!(all("온도 3에서 정도 5만큼"), Vec::<String>::new());
        assert_eq!(all("도 3A의"), ["3a"]);
    }

    #[test]
    fn signs_are_read_after_the_heading_and_matched_as_whole_numbers() {
        assert_eq!(
            sign_labels("10: 몸체 20 : 덮개, 30a：바퀴"),
            ["10", "20", "30a"]
        );
        assert!(contains_sign("몸체(10)는", "10"));
        assert!(contains_sign("몸체 10 은", "10"));
        assert!(!contains_sign("몸체(100)는", "10"));
        assert!(!contains_sign("몸체(10a)는", "10"));
        assert!(!contains_sign("몸체(110)는", "10"));
    }

    #[test]
    fn checks_a_numbered_specification() {
        let paragraphs = doc(&[
            ("【발명의 설명】", None),
            ("【발명의 명칭】", None),
            ("시험 장치", None),
            ("【기술분야】", None),
            ("본 장치는 몸체(10)를 갖는다(도 1 및 2 참조).", Some("0001")),
            ("덮개를 더 갖는다(도 5).", None),
            ("", None),
            ("새로 넣은 문단", Some("+")),
            ("【부호의 설명】", None),
            ("10: 몸체 20: 덮개", Some("0002")),
            ("【청구범위】", None),
            ("【청구항 1】", None),
            ("몸체를 포함하는 장치.", None),
            ("【청구항 2】", None),
            ("제2항에 있어서, 덮개.", None),
            ("【도면】", None),
            ("【도 1】", None),
            ("【도 2】", None),
            ("【도 3】", None),
        ]);
        let citations = crate::claim_citation_issues(&[
            (1, "몸체를 포함하는 장치.".into()),
            (2, "제2항에 있어서, 덮개.".into()),
        ]);
        let issues = submission_checks(&paragraphs, &citations);
        // 명칭 글 · 빈 문단 · 새 문단 「+」 · 표제는 빼고, 번호 없는 본문만.
        assert_eq!(
            kinds(&issues, SubmitKind::Unnumbered),
            [("p5", "식별번호 없음: 덮개를 더 갖는다(도 5).")]
        );
        assert_eq!(
            kinds(&issues, SubmitKind::FigureMissing),
            [("p5", "「도 5」을 말하는데 【도 5】 도면이 없습니다")]
        );
        assert_eq!(
            kinds(&issues, SubmitKind::FigureUnmentioned),
            [("p18", "【도 3】을 본문에서 한 번도 말하지 않습니다")]
        );
        assert_eq!(
            kinds(&issues, SubmitKind::SignUnused),
            [("p9", "부호 20: 본문에 한 번도 나오지 않습니다")]
        );
        assert_eq!(kinds(&issues, SubmitKind::Citation).len(), 1);
        assert_eq!(kinds(&issues, SubmitKind::Citation)[0].0, "p13");
    }

    #[test]
    fn an_unnumbered_document_gets_one_line_and_no_drawings_section_skips_figures() {
        let paragraphs = doc(&[
            ("【발명의 설명】", None),
            ("【기술분야】", None),
            ("가나다(도 1).", None),
            ("라마바", None),
        ]);
        let issues = submission_checks(&paragraphs, &[]);
        assert_eq!(
            kinds(&issues, SubmitKind::Unnumbered),
            [(
                "-",
                "식별번호가 하나도 없습니다 — 「문단번호 넣기」를 누르세요"
            )]
        );
        assert!(kinds(&issues, SubmitKind::FigureMissing).is_empty());
        // 문단번호 넣기가 매긴 번호(wipo_id)도 번호다.
        let mut numbered = paragraphs.clone();
        numbered[2].wipo_id = Some(1);
        numbered[3].wipo_id = Some(2);
        assert!(submission_checks(&numbered, &[]).is_empty());
    }

    #[test]
    fn rules_learned_from_submitted_finals() {
        let paragraphs = doc(&[
            ("【발명의 설명】", None),
            ("【특허문헌】", None),
            ("(특허문헌 1) 가", Some("0001")),
            ("(특허문헌 2) 나", None),
            ("【발명을 실시하기 위한 구체적인 내용】", None),
            ("표 1 실시예(도 3 및 도 4a, 4b).", Some("0002")),
            ("구분실시예 1", None),
            ("다음 단락", Some("0004")),
            ("【도면】", None),
            ("【도 3a】", None),
            ("【도 3b】", None),
            ("【도 4】", None),
        ]);
        // 특허문헌 목록 둘째 줄 · 번호가 건너뛴 자리(표 글)는 빈 식별번호가 아니다.
        // 「도 3」은 【도 3a】 · 【도 3b】를, 「도 4a, 4b」는 한 장 【도 4】를 덮는다.
        assert_eq!(submission_checks(&paragraphs, &[]), []);
    }
}
