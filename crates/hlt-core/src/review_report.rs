//! OA 검토보고서(docx) 읽기 — 「보정안 N」 표의 청구항(추적 변경 = 보정 전 · 후)과 본문 문장의 오기 정정 · 도면 보정 후보.
//!
//! 보고서 구조(실물 국내 보고서): 「4. 보정안」 아래 표마다 1행 2칸 — 왼칸 `보정안 1`, 오른칸 = 청구항 전문(문단 하나 = 청구항 한 줄 ·
//! `【청구항 N】` 표제 문단). 바뀐 곳은 추적 변경(`w:ins` · `w:del`), 지운 항은 표제부터 통째 `w:del`.
//! 상세설명 · 도면 보정은 표가 아니라 문장이다(「【0011】 … 의 “가나”는 … “가다”의 명백한 오기」 · 「단락 [0070]의 'X'」 ·
//! 「도 5의 'teh'(the의 오기, 도면 보정 필요)」) — 후보로만 뽑고 사람이 고른다.

use std::{io::Read, path::Path};

use quick_xml::{Reader, events::Event};
use serde::{Deserialize, Serialize};

use crate::{CoreError, ErrorCode};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReviewReport {
    pub proposals: Vec<Proposal>,
    pub notes: Vec<DescriptionNote>,
    pub figures: Vec<FigureNote>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Proposal {
    /// 표 왼칸 그대로(예: 「보정안 1」 · 「보정안(제1안)」).
    pub label: String,
    pub claims: Vec<ProposedClaim>,
    /// 추적 변경이 있는가 — 없으면 보고서 글은 보정 후 전문뿐이라 보정 전 대조를 할 수 없다(`before` = `after`).
    pub tracked: bool,
}

/// 청구항 하나의 보정 전 · 후 줄. `after` 가 비면 삭제항 · `before` 가 비면 새 항.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedClaim {
    pub number: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

/// 상세설명 오기 정정 후보: 문단 번호(`0011`) 안의 `wrong` → `right`(보고서에 바른 말이 없으면 `None`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DescriptionNote {
    pub paragraphs: Vec<String>,
    pub wrong: String,
    pub right: Option<String>,
    pub sentence: String,
    /// 「뒤의 “제1”을 “제2”로」처럼 문단 안 한 자리만 고치라는 정정 — 바꾸기는 모든 자리를 바꾸니 기본으로 걸지 않는다.
    #[serde(default)]
    pub positional: bool,
}

/// 도면 보정 할 일(새 그림이 있어야 한다 — 자동으로 고치지 않는다).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FigureNote {
    pub number: String,
    pub sentence: String,
}

#[derive(Debug, Clone, Default)]
struct Line {
    before: String,
    after: String,
}

enum Block {
    Line(Line),
    Table(Vec<Vec<Vec<Line>>>),
}

fn report_error(reason: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::StructureInvalid, "review_report", false);
    error.details.insert("reason".into(), reason.into());
    error
}

pub fn read_review_report(path: &Path) -> Result<ReviewReport, CoreError> {
    let file = std::fs::File::open(path).map_err(|_| report_error("open"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| report_error("docx_zip"))?;
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|_| report_error("document_part"))?
        .read_to_string(&mut xml)
        .map_err(|_| report_error("document_part"))?;
    review_from_document_xml(&xml)
}

/// `word/document.xml` → 보고서. 표 밖 문장과, 보정안이 아닌 표의 문장에서 오기 후보를 찾는다.
pub fn review_from_document_xml(xml: &str) -> Result<ReviewReport, CoreError> {
    let blocks = blocks(xml)?;
    let mut proposals = Vec::new();
    let mut sentences = Vec::new();
    for block in &blocks {
        match block {
            Block::Line(line) => sentences.push(line.after.clone()),
            Block::Table(rows) => {
                let label = rows
                    .first()
                    .and_then(|row| row.first())
                    .map(|cell| {
                        cell.iter()
                            .map(|line| line.after.as_str())
                            .collect::<String>()
                    })
                    .unwrap_or_default();
                let body = rows.first().and_then(|row| row.get(1));
                let claims = body
                    .filter(|_| is_proposal_label(&label))
                    .map(|body| proposed_claims(body))
                    .unwrap_or_default();
                match body {
                    // 「보정안 | 별도의 보정 없이 …」 처럼 청구항이 없는 칸은 보정안이 아니다.
                    Some(_) if !claims.is_empty() => proposals.push(Proposal {
                        label: label.trim().to_string(),
                        tracked: claims.iter().any(|claim| claim.before != claim.after),
                        claims,
                    }),
                    _ => sentences.extend(
                        rows.iter()
                            .flatten()
                            .flatten()
                            .map(|line| line.after.clone()),
                    ),
                }
            }
        }
    }
    let mut notes = Vec::new();
    let mut figures = Vec::new();
    for text in &sentences {
        for sentence in split_sentences(text) {
            scan_sentence(&sentence, &mut notes, &mut figures);
        }
    }
    // 같은 권고가 보고서 두 곳에 실리기도 한다 — 같은 후보는 하나로.
    let mut seen = std::collections::BTreeSet::new();
    notes.retain(|note| {
        seen.insert((
            note.paragraphs.clone(),
            note.wrong.clone(),
            note.right.clone(),
        ))
    });
    let mut seen = std::collections::BTreeSet::new();
    figures.retain(|note| seen.insert(note.number.clone()));
    Ok(ReviewReport {
        proposals,
        notes,
        figures,
    })
}

/// 표 왼칸이 보정안 이름인가: `보정안` · `보정안 1` · `보정안(제1안)` · `제1안` · `1안`.
fn is_proposal_label(label: &str) -> bool {
    let compact: String = label.chars().filter(|ch| !ch.is_whitespace()).collect();
    if compact.starts_with("보정안") {
        return true;
    }
    let rest = compact
        .strip_suffix('안')
        .unwrap_or("")
        .trim_start_matches('제');
    !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit())
}

/// 오른칸 줄들을 `【청구항 N】` 로 끊는다 — 보정 전 · 후를 따로 읽고(통째 지운 항 · 통째 넣은 줄) 번호로 맞춘다.
fn proposed_claims(cell: &[Line]) -> Vec<ProposedClaim> {
    let split = |side: &dyn Fn(&Line) -> &str| {
        let mut claims: Vec<(String, Vec<String>)> = Vec::new();
        for line in cell {
            let text = side(line).trim();
            if text.is_empty() {
                continue;
            }
            if let Some((number, rest)) = claim_heading(text) {
                // 표제와 첫 줄이 한 문단에 붙은 보고서도 있다(「【청구항 1】가상 발전소 …」).
                let first = (!rest.is_empty()).then(|| rest.to_string());
                claims.push((number, first.into_iter().collect()));
            } else if let Some((_, lines)) = claims.last_mut() {
                lines.push(text.to_string());
            }
        }
        claims
    };
    let before = split(&|line: &Line| line.before.as_str());
    let after = split(&|line: &Line| line.after.as_str());
    let mut numbers: Vec<&String> = before.iter().map(|(number, _)| number).collect();
    for (number, _) in &after {
        if !numbers.contains(&number) {
            numbers.push(number);
        }
    }
    let lines = |claims: &[(String, Vec<String>)], number: &str| {
        claims
            .iter()
            .find(|(n, _)| n == number)
            .map(|(_, lines)| lines.clone())
            .unwrap_or_default()
    };
    numbers
        .into_iter()
        .map(|number| {
            let mut after = lines(&after, number);
            // 「【청구항 7】 삭제」 꼴도 삭제항이다.
            if after.len() == 1 && after[0] == "삭제" {
                after.clear();
            }
            ProposedClaim {
                number: number.clone(),
                before: lines(&before, number),
                after,
            }
        })
        .collect()
}

/// 문단 맨 앞 `【청구항 N】` → (번호, 뒤에 붙은 글).
fn claim_heading(text: &str) -> Option<(String, &str)> {
    let (number, rest) = text.strip_prefix("【청구항")?.split_once('】')?;
    let number = number.trim();
    (!number.is_empty() && number.chars().all(|ch| ch.is_ascii_alphanumeric()))
        .then(|| (number.to_string(), rest.trim()))
}

/// 문장으로 나눈다(「…다. 」 · 「; 」).
fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (index, ch) in chars.iter().enumerate() {
        current.push(*ch);
        let next_is_space = chars.get(index + 1).is_none_or(|next| next.is_whitespace());
        let end = (*ch == '.' && index > 0 && chars[index - 1] == '다' && next_is_space)
            || (*ch == ';' && next_is_space);
        if end {
            out.push(std::mem::take(&mut current).trim().to_string());
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Paragraph(String),
    Figure(String),
    Quote(String),
    /// 따옴표 사이 글.
    Gap(String),
}

const OPEN_QUOTES: [(char, char); 5] = [
    ('“', '”'),
    ('‘', '’'),
    ('"', '"'),
    ('\'', '\''),
    ('「', '」'),
];

/// 문장을 문단 번호 · 도 번호 · 따옴표 말 · 그 사이 글로 나눈다.
fn tokens(sentence: &str) -> Vec<Token> {
    let chars: Vec<char> = sentence.chars().collect();
    let mut out = Vec::new();
    let mut gap = String::new();
    let mut index = 0;
    let flush = |gap: &mut String, out: &mut Vec<Token>| {
        if !gap.is_empty() {
            out.push(Token::Gap(std::mem::take(gap)));
        }
    };
    while index < chars.len() {
        let ch = chars[index];
        // 【0011】 · [0011] — 네 자리 숫자만(범위 [0050-0051] 은 참조 인용이라 안 본다).
        if matches!(ch, '【' | '[') {
            let close = if ch == '【' { '】' } else { ']' };
            let digits: String = chars[index + 1..]
                .iter()
                .take_while(|ch| ch.is_ascii_digit())
                .collect();
            if digits.len() == 4 && chars.get(index + 5) == Some(&close) {
                flush(&mut gap, &mut out);
                out.push(Token::Paragraph(digits));
                index += 6;
                continue;
            }
        }
        // 도 5 · 도 10a
        if ch == '도'
            && chars.get(index + 1) == Some(&' ')
            && chars.get(index + 2).is_some_and(|ch| ch.is_ascii_digit())
            && (index == 0 || !chars[index - 1].is_alphanumeric() || chars[index - 1] == ' ')
        {
            let number: String = chars[index + 2..]
                .iter()
                .take_while(|ch| ch.is_ascii_alphanumeric())
                .collect();
            flush(&mut gap, &mut out);
            out.push(Token::Figure(number.clone()));
            index += 2 + number.chars().count();
            continue;
        }
        if let Some((_, close)) = OPEN_QUOTES.iter().find(|(open, _)| *open == ch)
            && let Some(length) = chars[index + 1..].iter().position(|ch| ch == close)
            && length > 0
            && length < 200
        {
            flush(&mut gap, &mut out);
            out.push(Token::Quote(
                chars[index + 1..index + 1 + length].iter().collect(),
            ));
            index += length + 2;
            continue;
        }
        gap.push(ch);
        index += 1;
    }
    flush(&mut gap, &mut out);
    out
}

/// 한 문장에서 오기 정정 후보와 도면 할 일을 뽑는다. 「오기」 · 「정정」 이 없는 문장은 참조 인용이라 후보를 내지 않는다
/// (도면은 「도면 보정」 문장이거나 그 도면 글에 「…의 오기」 가 붙을 때만).
fn scan_sentence(sentence: &str, notes: &mut Vec<DescriptionNote>, figures: &mut Vec<FigureNote>) {
    let corrective = sentence.contains("오기") || sentence.contains("정정");
    let drawing = sentence.contains("도면 보정") || sentence.contains("도면보정");
    let tokens = tokens(sentence);
    let mut paragraphs: Vec<String> = Vec::new();
    let mut figure: Option<String> = None;
    let mut index = 0;
    while index < tokens.len() {
        match &tokens[index] {
            Token::Paragraph(number) => {
                if figure.is_some() {
                    paragraphs.clear();
                    figure = None;
                }
                paragraphs.push(number.clone());
            }
            Token::Figure(number) => {
                paragraphs.clear();
                figure = Some(number.clone());
            }
            Token::Quote(wrong) => {
                let gap_after = |at: usize| match tokens.get(at) {
                    Some(Token::Gap(gap)) => gap.trim_start(),
                    _ => "",
                };
                let ends_to = |after: &str| after.starts_with('로') || after.starts_with("으로");
                // 「'teh'(the의 오기」 — 괄호 안 바른 말.
                let bracketed = (|| {
                    let rest = gap_after(index + 1).strip_prefix('(')?;
                    let (right, tail) = rest.split_once('의')?;
                    (tail.trim_start().starts_with("오기") && !right.trim().is_empty())
                        .then(|| right.trim().to_string())
                })();
                // 「도 5의 'X'」 — 이 따옴표가 도 번호에 바로 딸렸나(아니면 도 번호는 근거 인용이다).
                let on_figure = index >= 2
                    && matches!(&tokens[index - 1], Token::Gap(gap) if gap.trim() == "의")
                    && matches!(&tokens[index - 2], Token::Figure(_));
                if let Some(number) = figure.take()
                    && on_figure
                {
                    if drawing || bracketed.is_some() {
                        figures.push(FigureNote {
                            number,
                            sentence: sentence.to_string(),
                        });
                    }
                    index += 1;
                    continue;
                }
                // 이 따옴표 뒤 바른 말 찾기 — 절(「오기」 낱말 · 「“Y”로」)까지의 따옴표 가운데:
                //   「“X”는 … 뒤의 “A”를 “B”로」(구절 안 일부 정정) > 「“Y”의 오기」 > 「“Y”로」 > 「오기」 바로 앞 따옴표.
                // 사이의 문단 · 도 번호 · 다른 따옴표는 바른 말의 근거(대상이 아니다).
                let between = gap_after(index + 1);
                let subject = ['은', '는', '이', '가']
                    .iter()
                    .any(|m| between.starts_with(*m));
                let object = ['을', '를'].iter().any(|m| between.starts_with(*m));
                let mut candidates: Vec<(usize, &String, &str)> = Vec::new();
                for (at, token) in tokens.iter().enumerate().skip(index + 1) {
                    match token {
                        Token::Quote(text) => {
                            let after = gap_after(at + 1);
                            candidates.push((at, text, after));
                            if ends_to(after) {
                                break;
                            }
                        }
                        Token::Gap(gap) if gap.contains("오기") => break,
                        _ => {}
                    }
                }
                let pick = |found: Option<&(usize, &String, &str)>| {
                    found.map(|(at, text, _)| (*at, (*text).clone()))
                };
                let nested = candidates.windows(2).find(|pair| {
                    ['을', '를'].iter().any(|m| pair[0].2.starts_with(*m)) && ends_to(pair[1].2)
                });
                let positional = nested.is_some_and(|pair| {
                    matches!(&tokens[pair[0].0 - 1], Token::Gap(gap)
                        if ["뒤의", "앞의", "첫", "번째", "마지막"].iter().any(|word| gap.trim_end().ends_with(word)))
                });
                let (wrong, paired) = match nested {
                    Some(pair) if subject => (pair[0].1.clone(), pick(Some(&pair[1]))),
                    _ if subject => (
                        wrong.clone(),
                        pick(candidates.iter().find(|(_, _, after)| {
                            after.starts_with('의') && after.contains("오기")
                        }))
                        .or_else(|| pick(candidates.iter().find(|(_, _, after)| ends_to(after))))
                        .or_else(|| {
                            pick(
                                candidates
                                    .iter()
                                    .rev()
                                    .find(|(_, _, after)| after.contains("오기")),
                            )
                        }),
                    ),
                    _ if object => (
                        wrong.clone(),
                        pick(candidates.first().filter(|(_, _, after)| ends_to(after))),
                    ),
                    _ => (wrong.clone(), None),
                };
                let right = paired
                    .as_ref()
                    .map(|(_, right)| right.clone())
                    .or(bracketed);
                if corrective && !paragraphs.is_empty() {
                    notes.push(DescriptionNote {
                        paragraphs: std::mem::take(&mut paragraphs),
                        wrong,
                        right,
                        sentence: sentence.to_string(),
                        positional,
                    });
                }
                if let Some((next, _)) = paired {
                    index = next;
                    paragraphs.clear();
                    figure = None;
                }
            }
            Token::Gap(_) => {}
        }
        index += 1;
    }
    // 「도 5 … 도면 보정」 처럼 따옴표 없이 적은 도면.
    if drawing
        && let Some(number) = figure
        && !figures
            .iter()
            .any(|note| note.number == number && note.sentence == sentence)
    {
        figures.push(FigureNote {
            number,
            sentence: sentence.to_string(),
        });
    }
}

/// 본문 블록: 표 밖 문단(줄)과 표(행 · 칸 · 칸 안 줄). 문단마다 보정 전(`w:del` 포함 · `w:ins` 뺌) · 후(반대) 글.
fn blocks(xml: &str) -> Result<Vec<Block>, CoreError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut out = Vec::new();
    let mut table_depth = 0_usize;
    let mut table: Vec<Vec<Vec<Line>>> = Vec::new();
    let mut line: Option<Line> = None;
    let (mut inserted, mut deleted, mut in_text) = (0_usize, 0_usize, false);
    let push = |line: &mut Option<Line>, text: &str, inserted: usize, deleted: usize| {
        if let Some(line) = line.as_mut() {
            if inserted == 0 {
                line.before.push_str(text);
            }
            if deleted == 0 {
                line.after.push_str(text);
            }
        }
    };
    loop {
        match reader
            .read_event()
            .map_err(|_| report_error("document_xml"))?
        {
            Event::Start(start) => match start.local_name().as_ref() {
                b"tbl" => {
                    table_depth += 1;
                    if table_depth == 1 {
                        table = Vec::new();
                    }
                }
                b"tr" if table_depth == 1 => table.push(Vec::new()),
                b"tc" if table_depth == 1 => {
                    if let Some(row) = table.last_mut() {
                        row.push(Vec::new());
                    }
                }
                b"p" => line = Some(Line::default()),
                b"ins" => inserted += 1,
                b"del" => deleted += 1,
                b"t" | b"delText" => in_text = true,
                _ => {}
            },
            Event::Empty(empty) => {
                if empty.local_name().as_ref() == b"tab" {
                    push(&mut line, "\t", inserted, deleted);
                }
            }
            Event::End(end) => match end.local_name().as_ref() {
                b"tbl" => {
                    table_depth = table_depth.saturating_sub(1);
                    if table_depth == 0 {
                        out.push(Block::Table(std::mem::take(&mut table)));
                    }
                }
                b"p" => {
                    if let Some(done) = line.take() {
                        match table.last_mut().and_then(|row| row.last_mut()) {
                            Some(cell) if table_depth > 0 => cell.push(done),
                            _ => out.push(Block::Line(done)),
                        }
                    }
                }
                b"ins" => inserted = inserted.saturating_sub(1),
                b"del" => deleted = deleted.saturating_sub(1),
                b"t" | b"delText" => in_text = false,
                _ => {}
            },
            Event::Text(text) if in_text => {
                let decoded = text.decode().map_err(|_| report_error("document_xml"))?;
                let text = quick_xml::escape::unescape(&decoded)
                    .map_err(|_| report_error("document_xml"))?;
                push(&mut line, &text, inserted, deleted);
            }
            Event::GeneralRef(reference) if in_text => {
                let name = reference
                    .decode()
                    .map_err(|_| report_error("document_xml"))?;
                if let Some(ch) = crate::docx::resolve_xml_reference(&name) {
                    push(&mut line, &ch.to_string(), inserted, deleted);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notes(sentence: &str) -> (Vec<DescriptionNote>, Vec<FigureNote>) {
        let (mut notes, mut figures) = (Vec::new(), Vec::new());
        scan_sentence(sentence, &mut notes, &mut figures);
        (notes, figures)
    }

    #[test]
    fn reads_typo_sentences_and_skips_citations() {
        let (found, _) = notes(
            "【0011】, 【0016】의 “가나”는 【0053】의 기재에 비추어 “가다”의 명백한 오기이고, 【0021】의 “라마(Ab)”는 “라마(Abc)”의 오기입니다.",
        );
        assert_eq!(found.len(), 2, "{found:?}");
        // 두 따옴표 사이 【0053】 은 바른 말의 근거라 대상이 아니다.
        assert_eq!(found[0].paragraphs, ["0011", "0016"]);
        assert_eq!(
            (found[0].wrong.as_str(), found[0].right.as_deref()),
            ("가나", Some("가다"))
        );
        assert_eq!(found[1].paragraphs, ["0021"]);
        assert_eq!(found[1].right.as_deref(), Some("라마(Abc)"));

        let (found, figures) = notes(
            "보정서 제출시 참고사항(오기 정정 권고): 명세서 단락 [0070]의 '갉', 단락 [0127]의 '있.', 도 5의 'teh'(the의 오기, 도면 보정 필요)",
        );
        assert_eq!(
            found
                .iter()
                .map(|n| (n.paragraphs.clone(), n.wrong.as_str(), n.right.clone()))
                .collect::<Vec<_>>(),
            [
                (vec!["0070".to_string()], "갉", None),
                (vec!["0127".to_string()], "있.", None),
            ]
        );
        assert_eq!(figures.len(), 1);
        assert_eq!(figures[0].number, "5");

        // 나열 「“X”는 … “Y”로, “X2”은 도 5의 “Z” 표기에 비추어 “Y2”으로, “긴 구절”는 … 뒤의 “A”를 “B”로 … 정정」.
        let (found, figures) = notes(
            "아울러 【0072】의 “열 개”는 도 6의 “(아홉)” 표기와 어긋나므로 “아홉 개”로, 【0070】의 “Alfa”은 도 5의 “Alpha” 표기에 비추어 “Alpha”으로, 【0049】의 “제1 장치로부터 수신된 제1 장치”는 도 2의 S102 기재에 비추어 뒤의 “제1”을 “제2”로 각각 바로잡는 정정을 제안드립니다.",
        );
        assert_eq!(
            found
                .iter()
                .map(|n| (n.paragraphs.clone(), n.wrong.as_str(), n.right.as_deref()))
                .collect::<Vec<_>>(),
            [
                (vec!["0072".to_string()], "열 개", Some("아홉 개")),
                (vec!["0070".to_string()], "Alfa", Some("Alpha")),
                (vec!["0049".to_string()], "제1", Some("제2")),
            ]
        );
        assert_eq!(
            found.iter().map(|n| n.positional).collect::<Vec<_>>(),
            [false, false, true],
            "「뒤의」 = 한 자리"
        );
        assert!(figures.is_empty(), "도 번호는 근거 인용: {figures:?}");

        // 참조 인용만(오기 · 정정 없음)은 후보가 아니다.
        let (found, figures) = notes("부품은 단락 [0050]의 “제1 부품”과 같다(도 2의 'S100' 참조).");
        assert!(found.is_empty() && figures.is_empty());
    }

    #[test]
    fn reads_proposal_tables_with_tracked_changes() {
        let p = |runs: &str| format!("<w:p>{runs}</w:p>");
        let r = |text: &str| format!("<w:r><w:t xml:space=\"preserve\">{text}</w:t></w:r>");
        let ins = |text: &str| format!("<w:ins><w:r><w:t>{text}</w:t></w:r></w:ins>");
        let del = |text: &str| format!("<w:del><w:r><w:delText>{text}</w:delText></w:r></w:del>");
        let cell = |body: String| format!("<w:tc>{body}</w:tc>");
        let table = |label: &str, body: String| {
            format!(
                "<w:tbl><w:tr>{}{}</w:tr></w:tbl>",
                cell(p(&r(label))),
                cell(body)
            )
        };
        let proposal = [
            p(&r("【청구항 1】")),
            p(&format!(
                "{}{}{}",
                r("몸체를 "),
                del("가진"),
                ins("포함하는")
            )),
            p(&ins("덮개를 더 포함하는 장치.")),
            p(&del("【청구항 2】")),
            p(&del("제1항에 있어서, 바닥.")),
            p(&r("【청구항 3】제1항에 있어서, 뚜껑.")),
        ]
        .concat();
        let xml = format!(
            "<w:document xmlns:w=\"w\"><w:body>{}{}{}</w:body></w:document>",
            p(&r("4. 보정안")),
            table("보정안 1", proposal),
            table("구성", p(&r("단락 [0011]의 “가나”는 “가다”의 오기입니다."))),
        );
        let report = review_from_document_xml(&xml).unwrap();
        assert_eq!(report.proposals.len(), 1);
        assert_eq!(report.proposals[0].label, "보정안 1");
        assert!(report.proposals[0].tracked);
        assert_eq!(
            report.proposals[0].claims,
            [
                ProposedClaim {
                    number: "1".into(),
                    before: vec!["몸체를 가진".into()],
                    after: vec!["몸체를 포함하는".into(), "덮개를 더 포함하는 장치.".into()],
                },
                ProposedClaim {
                    number: "2".into(),
                    before: vec!["제1항에 있어서, 바닥.".into()],
                    after: Vec::new(),
                },
                ProposedClaim {
                    number: "3".into(),
                    before: vec!["제1항에 있어서, 뚜껑.".into()],
                    after: vec!["제1항에 있어서, 뚜껑.".into()],
                },
            ]
        );
        // 보정안이 아닌 표의 문장에서도 오기 후보를 찾는다.
        assert_eq!(report.notes.len(), 1);
        assert_eq!(report.notes[0].right.as_deref(), Some("가다"));
    }

    #[test]
    fn proposal_labels() {
        for label in [
            "보정안 1",
            "보정안2",
            "제1안",
            "2안",
            " 보정안 3 ",
            "보정안",
            "보정안(제1안)",
        ] {
            assert!(is_proposal_label(label), "{label}");
        }
        for label in ["대응안", "구성 1", "본원발명"] {
            assert!(!is_proposal_label(label), "{label}");
        }
    }
}
