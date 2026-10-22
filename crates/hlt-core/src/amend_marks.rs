//! 보정 표시(최종본에 견준 바뀐 글 · 화면 문단 좌표) · 청구항 인용 검사(특허로 점검 JS `claim_check_kr.js` 의
//! 오참조 · 중복 다중참조 · 택일어 미사용 규칙을 옮김 — 판정이 같은지는 실물 코퍼스에서 그 JS 와 대조했다).

use difflib::sequencematcher::SequenceMatcher;
use serde::Serialize;

use crate::EditorParagraph;

/// 바뀐 문단 하나. `inserted` = 지금 글의 [start, end) · `deleted` = (지금 글 자리, 지운 글). 둘 다 비면 서식만 바뀌었거나
/// 줄을 나누고 붙인 것(그래도 보정서에서는 바뀐 요소다).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParagraphMark {
    pub id: String,
    pub inserted: Vec<(u32, u32)>,
    pub deleted: Vec<(u32, String)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AmendmentMarks {
    pub paragraphs: Vec<ParagraphMark>,
    /// 바뀐 요소(보정서 `A`) · 새 요소(`I`) · 지운 요소(`D`) — 「청구항 7」 · 「【0049】」.
    pub changed: Vec<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

/// 비교 단위 = 보정서 요소: 식별번호 문단 · 청구항(표제 뒤 본문 줄들). 「+」(새 문단)는 하나하나 새 요소.
struct Unit<'a> {
    key: String,
    label: String,
    parts: Vec<&'a EditorParagraph>,
}

fn units(paragraphs: &[EditorParagraph]) -> Vec<Unit<'_>> {
    let texts: Vec<&str> = paragraphs.iter().map(|p| p.text.as_str()).collect();
    let blocks = crate::editor::claim_blocks(&texts);
    let mut in_claims = vec![false; paragraphs.len()];
    let mut out = Vec::new();
    for (number, heading, end) in &blocks {
        in_claims[*heading..*end].fill(true);
        out.push(Unit {
            key: format!("claim:{number}"),
            label: format!("청구항 {number}"),
            parts: paragraphs[heading + 1..*end].iter().collect(),
        });
    }
    for (at, paragraph) in paragraphs.iter().enumerate() {
        let Some(number) = paragraph.number.as_deref().filter(|_| !in_claims[at]) else {
            continue;
        };
        let (key, label) = if number == "+" {
            (format!("new:{}", paragraph.id), "새 문단".to_string())
        } else {
            (format!("p:{number}"), format!("【{number}】"))
        };
        out.push(Unit {
            key,
            label,
            parts: vec![paragraph],
        });
    }
    out
}

/// 최종본(`before`) 대비 지금 문단(`now`)의 보정 표시. 글자 차이는 요소 글(줄을 `\n` 으로 이음)끼리 재서 문단 좌표로 돌린다.
// ponytail: 그림만 바꾼 문단 · 도면은 화면 글(`_`)이 같아 안 잡힌다 — 그림 정체 비교는 필요해지면.
pub fn amendment_marks(before: &[EditorParagraph], now: &[EditorParagraph]) -> AmendmentMarks {
    let old_units = units(before);
    let new_units = units(now);
    let mut marks = AmendmentMarks::default();
    let joined = |unit: &Unit| -> Vec<char> {
        let texts: Vec<&str> = unit.parts.iter().map(|p| p.text.as_str()).collect();
        texts.join("\n").chars().collect()
    };
    for unit in &new_units {
        let Some(old) = old_units.iter().find(|old| old.key == unit.key) else {
            marks.added.push(unit.label.clone());
            for part in &unit.parts {
                let length = part.text.chars().count() as u32;
                marks.paragraphs.push(ParagraphMark {
                    id: part.id.clone(),
                    inserted: if length > 0 {
                        vec![(0, length)]
                    } else {
                        Vec::new()
                    },
                    deleted: Vec::new(),
                });
            }
            continue;
        };
        let (old_text, new_text) = (joined(old), joined(unit));
        let same_spans = old.parts.len() == unit.parts.len()
            && old
                .parts
                .iter()
                .zip(&unit.parts)
                .all(|(a, b)| a.spans == b.spans);
        if old_text == new_text && same_spans {
            continue;
        }
        marks.changed.push(unit.label.clone());
        // 지금 글의 문단 경계: 문단 k 는 joined 의 [starts[k], starts[k] + 길이).
        let mut starts = Vec::with_capacity(unit.parts.len());
        let mut at = 0usize;
        for part in &unit.parts {
            starts.push(at);
            at += part.text.chars().count() + 1;
        }
        let part_of = |position: usize| {
            starts
                .iter()
                .rposition(|start| *start <= position)
                .unwrap_or(0)
        };
        let mut per_part: Vec<ParagraphMark> = unit
            .parts
            .iter()
            .map(|part| ParagraphMark {
                id: part.id.clone(),
                inserted: Vec::new(),
                deleted: Vec::new(),
            })
            .collect();
        let mut touched = vec![false; unit.parts.len()];
        let mut matcher = SequenceMatcher::new(&old_text, &new_text);
        for op in matcher.get_opcodes() {
            if op.tag == "equal" {
                continue;
            }
            if op.second_end > op.second_start {
                // 넣은 글: 문단 경계(`\n`)에서 자른다.
                let mut position = op.second_start;
                while position < op.second_end {
                    let k = part_of(position);
                    let end = (starts[k] + unit.parts[k].text.chars().count()).min(op.second_end);
                    touched[k] = true;
                    if end > position {
                        per_part[k]
                            .inserted
                            .push(((position - starts[k]) as u32, (end - starts[k]) as u32));
                    }
                    position = end.max(position) + 1;
                }
            }
            if op.first_end > op.first_start {
                let k = part_of(op.second_start);
                touched[k] = true;
                let text: String = old_text[op.first_start..op.first_end]
                    .iter()
                    .map(|ch| if *ch == '\n' { '↵' } else { *ch })
                    .collect();
                let local = op.second_start.saturating_sub(starts[k]);
                let local = local.min(unit.parts[k].text.chars().count());
                per_part[k].deleted.push((local as u32, text));
            }
        }
        if !same_spans && old_text == new_text {
            for (k, (a, b)) in old.parts.iter().zip(&unit.parts).enumerate() {
                touched[k] |= a.spans != b.spans;
            }
        }
        if !touched.contains(&true) {
            touched.fill(true);
        }
        marks.paragraphs.extend(
            per_part
                .into_iter()
                .zip(touched)
                .filter_map(|(mark, touched)| touched.then_some(mark)),
        );
    }
    for old in &old_units {
        if !new_units.iter().any(|unit| unit.key == old.key) && !old.key.starts_with("new:") {
            marks.removed.push(old.label.clone());
        }
    }
    marks
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CitationKind {
    /// 자기 항 · 뒤 항 · 없는 항을 인용(점검 페이지 「청구항 오참조」).
    MisReference,
    /// 지운 항(번호가 빈 앞 항)을 인용 — 점검 페이지는 빈 번호를 「삭제」로 채워 못 잡는다.
    Removed,
    /// 다중 인용 항이 다중 인용 항을 (거슬러) 인용(「중복 다중참조」).
    DuplicateMultiple,
    /// 「제N항 내지 제M항」 뒤에 「어느」가 없다(「택일어 미사용」).
    NoAlternative,
    // 아래는 앱만 보는 규칙(점검 페이지는 못 잡는다 — 페이지 대조에서 뺀다).
    /// 페이지가 한 번호만 읽는 범위(「청구항 1 내지 3」 · 「청구항 1 내지 청구항 3」)의 나머지 번호가 자기 · 뒤 · 없는 항.
    RangeMisReference,
    /// 둘 이상을 「및」 · 「과/와」로 묶어 인용(택일적 기재가 아니다).
    AndCitation,
    /// 페이지가 범위로 안 읽는 범위(「청구항 1 내지 3」 등) 뒤에 「어느」가 없다.
    RangeNoAlternative,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CitationIssue {
    pub claim: u32,
    pub kind: CitationKind,
    /// 문제가 된 인용 번호(중복 다중참조 · 택일어는 이 항이 인용한 번호 전부).
    pub cited: Vec<u32>,
}

/// 청구항 인용 검사. `claims` = (번호, 본문 — 줄을 구분자 없이 이은 글: 특허로 목록처럼 `<br/>` 를 지운 꼴), 번호 차례.
pub fn claim_citation_issues(claims: &[(u32, String)]) -> Vec<CitationIssue> {
    let numbers: std::collections::BTreeSet<u32> =
        claims.iter().map(|(number, _)| *number).collect();
    // 부모 목록(같은 항을 두 번 인용하면 두 번 — 점검 페이지 `addParent` 가 그렇다) · 빈 번호(지운 항)는 부모가 없는 항.
    let mut parents: std::collections::BTreeMap<u32, Vec<u32>> = Default::default();
    let mut issues = Vec::new();
    for (number, text) in claims {
        let number = *number;
        let mut mine: Vec<u32> = Vec::new();
        let (mut missing, mut removed, mut duplicate, mut alternative) =
            (Vec::new(), Vec::new(), false, false);
        let multi_branch = |parents: &std::collections::BTreeMap<u32, Vec<u32>>, start: u32| {
            let mut list = parents.get(&start).cloned().unwrap_or_default();
            loop {
                match list.len() {
                    0 => return false,
                    1 => list = parents.get(&list[0]).cloned().unwrap_or_default(),
                    _ => return true,
                }
            }
        };
        for citation in citations(text) {
            let cited: Vec<u32> = match citation {
                Citation::AllPrevious => (1..number).collect(),
                Citation::Range(from, to, ref rest) => {
                    if to.is_some() && !chooses(rest) {
                        alternative = true;
                    }
                    (from..=to.unwrap_or(from)).collect()
                }
            };
            for i in cited {
                if i >= number {
                    missing.push(i);
                    continue;
                }
                if !numbers.contains(&i) {
                    removed.push(i);
                }
                mine.push(i);
            }
            if mine.len() > 1 && mine.iter().any(|parent| multi_branch(&parents, *parent)) {
                duplicate = true;
            }
        }
        // 앱만 보는 인용 묶음: 페이지가 읽은 번호 밖의 범위 번호 · 「및」 묶음 · 페이지가 안 본 범위의 택일어.
        let seen: std::collections::BTreeSet<u32> = mine.iter().chain(&missing).copied().collect();
        let (mut range_missing, mut and_citation, mut range_alternative) =
            (Vec::new(), false, false);
        for group in groups(text) {
            // 「…및 제5항 중 어느 한 항」은 택일적이다(실물 7건) — 뒤에 「어느」가 오면 묶음이 아니다.
            and_citation |= group.and && group.numbers.len() > 1 && !chooses_loosely(&group.rest);
            range_alternative |= group.range && !alternative && !chooses_loosely(&group.rest);
            for i in group.numbers.into_iter().filter(|i| !seen.contains(i)) {
                if i >= number {
                    range_missing.push(i);
                } else if !numbers.contains(&i) {
                    removed.push(i);
                }
            }
        }
        removed.sort_unstable();
        removed.dedup();
        range_missing.sort_unstable();
        range_missing.dedup();
        let mut cited_all = mine.clone();
        cited_all.dedup();
        if !missing.is_empty() {
            issues.push(CitationIssue {
                claim: number,
                kind: CitationKind::MisReference,
                cited: missing,
            });
        }
        if !removed.is_empty() {
            issues.push(CitationIssue {
                claim: number,
                kind: CitationKind::Removed,
                cited: removed,
            });
        }
        if duplicate {
            issues.push(CitationIssue {
                claim: number,
                kind: CitationKind::DuplicateMultiple,
                cited: cited_all.clone(),
            });
        }
        if alternative {
            issues.push(CitationIssue {
                claim: number,
                kind: CitationKind::NoAlternative,
                cited: cited_all.clone(),
            });
        }
        if !range_missing.is_empty() {
            issues.push(CitationIssue {
                claim: number,
                kind: CitationKind::RangeMisReference,
                cited: range_missing,
            });
        }
        if and_citation {
            issues.push(CitationIssue {
                claim: number,
                kind: CitationKind::AndCitation,
                cited: cited_all.clone(),
            });
        }
        if range_alternative {
            issues.push(CitationIssue {
                claim: number,
                kind: CitationKind::RangeNoAlternative,
                cited: cited_all,
            });
        }
        parents.insert(number, mine);
    }
    issues
}

enum Citation {
    /// 「위 항들」 등 — 앞 항 전부.
    AllPrevious,
    /// (첫 번호, 「내지」 끝 번호, 그 뒤 글).
    Range(u32, Option<u32>, String),
}

/// 점검 페이지 정규식(`checkRegex1`)을 왼쪽부터 훑는 것과 같은 차례로 인용을 읽는다. 「상기 …」 구절도 그 페이지처럼
/// 소비한다(「상기 청구항 1」의 번호는 인용이 아니다). 불명확한 표현 낱말은 인용과 겹치지 않아 건너뛴다.
fn citations(text: &str) -> Vec<Citation> {
    const ALL: [&str; 4] = ["위 항들", "위의 항들", "상기의 항들", "선행하는 항들"];
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut at = 0;
    'scan: while at < chars.len() {
        let rest: String = chars[at..].iter().collect();
        for word in ALL {
            if rest.starts_with(word) {
                out.push(Citation::AllPrevious);
                at += word.chars().count();
                continue 'scan;
            }
        }
        if let Some((from, to, used)) = range_at(&chars[at..]) {
            let tail: String = chars[at + used..].iter().collect();
            out.push(Citation::Range(from, Some(to), tail));
            at += used;
            continue;
        }
        if let Some((number, used)) = single_at(&chars[at..]) {
            out.push(Citation::Range(number, None, String::new()));
            at += used;
            continue;
        }
        if let Some(used) = above_at(&chars[at..]) {
            at += used;
            continue;
        }
        at += 1;
    }
    out
}

/// 앱만 보는 인용 묶음: 첫 인용(「제N항」 · 「청구항 N」 · 「제N항 내지 제M항」) 뒤로 연결어(`,` · 또는 · 내지 · ~ · 및 · 과/와)와
/// 번호가 이어진 것. 뒤 번호는 「제 … 항」 없이 써도 된다(「청구항 1 내지 3」). 「상기 …」 구절은 페이지처럼 건너뛴다.
struct Group {
    numbers: Vec<u32>,
    /// 「및」 · 「과/와」로 이었다.
    and: bool,
    /// 「내지」 · 「~」 범위가 있다(페이지가 범위로 읽는 「제N항 내지 제M항」 하나뿐인 묶음은 뺀다 — 그건 페이지 몫).
    range: bool,
    rest: String,
}

fn groups(text: &str) -> Vec<Group> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        if let Some(used) = above_at(&chars[at..]) {
            at += used;
            continue;
        }
        let first = range_at(&chars[at..])
            .map(|(from, to, used)| ((from..=to).collect::<Vec<u32>>(), used, true))
            .or_else(|| single_at(&chars[at..]).map(|(number, used)| (vec![number], used, false)));
        let Some((mut numbers, used, _)) = first else {
            at += 1;
            continue;
        };
        at += used;
        let (mut and, mut range) = (false, false);
        loop {
            let mut next = at;
            let mut joins = Vec::new();
            while let Some((join, used)) = join_at(&chars, next) {
                joins.push(join);
                next += used;
            }
            if joins.is_empty() {
                break;
            }
            let Some((number, used)) =
                single_at(&chars[next..]).or_else(|| bare_number_at(&chars, next))
            else {
                break;
            };
            let last = *numbers.last().unwrap_or(&number);
            if joins.iter().any(|join| matches!(*join, "내지" | "~")) {
                numbers.extend(last.saturating_add(1)..=number);
                range = true;
            } else {
                numbers.push(number);
            }
            and |= joins.iter().any(|join| matches!(*join, "및" | "과" | "와"));
            at = next + used;
        }
        // 첫 인용이 페이지 범위(「제N항 내지 제M항」)면 그 택일어는 페이지 몫 — `range` 는 뒤에 이은 범위만 센다.
        out.push(Group {
            numbers,
            and,
            range,
            rest: chars[at..].iter().collect(),
        });
    }
    out
}

/// 인용 사이 연결어(앞뒤 공백 포함) → (연결어, 쓴 글자 수).
fn join_at(chars: &[char], at: usize) -> Option<(&'static str, usize)> {
    let lead = spaces(chars, at);
    for join in [",", "또는", "내지", "~", "및", "과", "와"] {
        let word: Vec<char> = join.chars().collect();
        if chars[at + lead..].starts_with(&word) {
            let used = lead + word.len();
            return Some((join, used + spaces(chars, at + used)));
        }
    }
    None
}

/// 연결어 뒤 맨 번호(「청구항 1 내지 3」의 3 · 「항」 붙어도 됨). 「2개」처럼 낱말에 붙은 수는 번호가 아니다.
fn bare_number_at(chars: &[char], at: usize) -> Option<(u32, usize)> {
    let (number, used) = digits(chars, at)?;
    let mut end = at + used;
    let gap = spaces(chars, end);
    if chars.get(end + gap) == Some(&'항') {
        end += gap + 1;
    }
    let boundary = match chars.get(end) {
        None => true,
        Some(ch) => matches!(
            ch,
            ' ' | ',' | '.' | '~' | '에' | '의' | '중' | '과' | '와' | '및' | '또' | '내'
        ),
    };
    // ponytail: 「2 개의」처럼 띄어 쓴 단위 말은 번호로 읽힌다 — 실물에서 걸리면 낱말 목록으로.
    boundary.then_some((number, end - at))
}

fn spaces(chars: &[char], at: usize) -> usize {
    chars[at..].iter().take_while(|ch| **ch == ' ').count()
}

fn digits(chars: &[char], at: usize) -> Option<(u32, usize)> {
    let count = chars[at..]
        .iter()
        .take_while(|ch| ch.is_ascii_digit())
        .count();
    let value: String = chars[at..at + count].iter().collect();
    value.parse().ok().map(|value| (value, count))
}

/// `제 *[0-9]+ *항` 또는 `청구항 *[0-9]+` → (번호, 쓴 글자 수).
fn single_at(chars: &[char]) -> Option<(u32, usize)> {
    if chars.first() == Some(&'제') {
        let mut at = 1 + spaces(chars, 1);
        let (number, used) = digits(chars, at)?;
        at += used;
        at += spaces(chars, at);
        return (chars.get(at) == Some(&'항')).then_some((number, at + 1));
    }
    if chars.starts_with(&['청', '구', '항']) {
        let at = 3 + spaces(chars, 3);
        let (number, used) = digits(chars, at)?;
        return Some((number, at + used));
    }
    None
}

/// `제 *[0-9]+ *항? +내지 +제? *[0-9]+ *항` → (첫 번호, 끝 번호, 쓴 글자 수).
fn range_at(chars: &[char]) -> Option<(u32, u32, usize)> {
    if chars.first() != Some(&'제') {
        return None;
    }
    let mut at = 1 + spaces(chars, 1);
    let (from, used) = digits(chars, at)?;
    at += used;
    at += spaces(chars, at);
    if chars.get(at) == Some(&'항') {
        at += 1;
    }
    let gap = spaces(chars, at);
    if gap == 0 || !chars[at + gap..].starts_with(&['내', '지']) {
        return None;
    }
    at += gap + 2;
    let gap = spaces(chars, at);
    if gap == 0 {
        return None;
    }
    at += gap;
    if chars.get(at) == Some(&'제') {
        at += 1;
    }
    at += spaces(chars, at);
    let (to, used) = digits(chars, at)?;
    at += used;
    at += spaces(chars, at);
    (chars.get(at) == Some(&'항')).then_some((from, to, at + 1))
}

/// `상기[^\S] *[^ .,]+` → 쓴 글자 수.
fn above_at(chars: &[char]) -> Option<usize> {
    if !chars.starts_with(&['상', '기']) || !chars.get(2).is_some_and(|ch| ch.is_whitespace()) {
        return None;
    }
    let mut at = 3 + spaces(chars, 3);
    let word = chars[at..]
        .iter()
        .take_while(|ch| !matches!(ch, ' ' | '.' | ','))
        .count();
    if word == 0 {
        // ` *` 가 물러나 마지막 공백이 낱말이 될 수는 없다(`[^ .,]`) — 공백 하나 뒤 낱말이 없으면 안 맞는다.
        return None;
    }
    at += word;
    Some(at)
}

/// 앱 규칙의 택일어: 인용 뒤 15자 안에 「어느」(「중, 어느 하나의」 · 「의 방법 중 어느 한 항」 같은 실물 꼴까지).
fn chooses_loosely(rest: &str) -> bool {
    rest.chars().take(15).collect::<String>().contains("어느")
}

/// 「내지」 범위 뒤 글이 `/ *(중|중의)? +([^ ]+)/` 의 첫 맞음에서 「어느」인가(점검 페이지 `chkChooseWord`).
fn chooses(rest: &str) -> bool {
    let chars: Vec<char> = rest.chars().collect();
    let word_at =
        |at: usize| -> String { chars[at..].iter().take_while(|ch| **ch != ' ').collect() };
    for start in 0..chars.len() {
        let gap = spaces(&chars, start);
        let after = start + gap;
        for particle in [&['중'][..], &['중', '의'][..]] {
            if chars[after..].starts_with(particle) {
                let next = after + particle.len();
                let more = spaces(&chars, next);
                if more > 0 && next + more < chars.len() {
                    return word_at(next + more) == "어느";
                }
            }
        }
        if gap > 0 && after < chars.len() {
            return word_at(after) == "어느";
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issues(claims: &[(u32, &str)]) -> Vec<(u32, CitationKind, Vec<u32>)> {
        let claims: Vec<(u32, String)> = claims.iter().map(|(n, t)| (*n, t.to_string())).collect();
        claim_citation_issues(&claims)
            .into_iter()
            .map(|issue| (issue.claim, issue.kind, issue.cited))
            .collect()
    }

    #[test]
    fn reads_citations_like_the_kipo_page() {
        use CitationKind::*;
        // 바른 인용: 단일 · 택일 · 범위+어느 · 「청구항 N」.
        assert!(
            issues(&[
                (1, "장치."),
                (2, "제1항에 있어서, 덮개를 포함하는 장치."),
                (3, "제1항 또는 제2항에 있어서, 손잡이를 포함하는 장치."),
                (4, "청구항 1에 있어서, 바퀴를 포함하는 장치."),
                (5, "방법."),
                (
                    6,
                    "제5항 내지 제5항 중 어느 한 항에 있어서, 단계를 포함하는 방법."
                ),
            ])
            .is_empty()
        );
        // 뒤 항 · 자기 항 · 없는 항.
        assert_eq!(
            issues(&[
                (1, "제2항에 있어서, 장치."),
                (2, "제2항의 장치."),
                (3, "제9항의 장치.")
            ]),
            [
                (1, MisReference, vec![2]),
                (2, MisReference, vec![2]),
                (3, MisReference, vec![9])
            ]
        );
        // 지운 항(빈 번호) 인용.
        assert_eq!(
            issues(&[(1, "장치."), (3, "제2항에 있어서, 장치.")]),
            [(3, Removed, vec![2])]
        );
        // 범위 뒤 「어느」 없음.
        assert_eq!(
            issues(&[
                (1, "장치."),
                (2, "장치."),
                (3, "제1항 내지 제2항에 따른 장치.")
            ]),
            [(3, NoAlternative, vec![1, 2])]
        );
        // 다중 인용(3)을 다시 다중 인용(4) → 중복 다중참조. 2 는 한 항 범위라 다중이 아니다(3 은 문제없다).
        assert_eq!(
            issues(&[
                (1, "장치."),
                (2, "제1항 내지 제1항 중 어느 한 항에 있어서, 장치."),
                (3, "제1항 또는 제2항에 있어서, 장치."),
                (4, "제2항 또는 제3항에 있어서, 장치."),
            ]),
            [(4, DuplicateMultiple, vec![2, 3])]
        );
        // 앱만 보는 규칙: 「및」 묶음 · 페이지가 1만 읽는 「청구항 1 내지 3」의 나머지(지운 2 · 뒤 4) · 그 범위 뒤 「어느」 없음.
        assert_eq!(
            issues(&[
                (1, "장치."),
                (3, "제1항 및 제2항에 있어서, 장치."),
                (4, "청구항 1 내지 청구항 3 중 어느 한 항에 있어서, 장치."),
                (5, "청구항 1 내지 6에 따른 장치."),
                (6, "제1항과 2개의 바퀴를 포함하는 장치."),
                (7, "제1항 및 제6항 중, 어느 하나의 항에 따른 장치."),
            ]),
            [
                (3, Removed, vec![2]),
                (3, AndCitation, vec![1, 2]),
                (4, Removed, vec![2]),
                (4, DuplicateMultiple, vec![1, 3]), // 3 이 다중(1 · 2) — 페이지 규칙
                (5, Removed, vec![2]),
                (5, RangeMisReference, vec![5, 6]),
                (5, RangeNoAlternative, vec![1]),
            ]
        );
        // 인용이 아닌 것: 「제1 부재」 · 번호 없는 「청구항에」 · 「상기 청구항 1」(점검 페이지가 「상기」 구절로 먹는다).
        assert!(issues(&[(1, "제1 부재와 청구항에 기재된 상기 청구항 9의 장치.")]).is_empty());
    }

    fn paragraph(id: &str, text: &str, number: Option<&str>) -> EditorParagraph {
        EditorParagraph {
            id: id.into(),
            text: text.into(),
            wipo_id: None,
            number: number.map(str::to_string),
            has_objects: false,
            spans: Vec::new(),
        }
    }

    #[test]
    fn marks_changes_against_the_final_version() {
        let before = vec![
            paragraph("a", "【발명의 설명】", None),
            paragraph("b", "몸체를 갖는 장치.", Some("0001")),
            paragraph("c", "지울 문단.", Some("0002")),
            paragraph("d", "【청구항 1】", None),
            paragraph("e", "몸체를 포함하는 장치.", None),
            paragraph("f", "【청구항 2】", None),
            paragraph("g", "제1항의 장치.", None),
        ];
        // 【0001】 고침 · 「+」 새 문단 · 【0002】 지움 · 청구항 1 두 군데 고침 + 줄 하나 더.
        let now = vec![
            before[0].clone(),
            paragraph("b", "몸체와 바닥을 갖는 장치.", Some("0001")),
            paragraph("h", "새 문단.", Some("+")),
            before[3].clone(),
            paragraph("e", "큰 몸체를 포함하는 장치,", None),
            paragraph("e2", "덮개.", None),
            before[5].clone(),
            before[6].clone(),
        ];
        let marks = amendment_marks(&before, &now);
        assert_eq!(marks.changed, ["청구항 1", "【0001】"]);
        assert_eq!(marks.removed, ["【0002】"]);
        assert_eq!(marks.added, ["새 문단"]);
        let of = |id: &str| {
            marks
                .paragraphs
                .iter()
                .find(|m| m.id == id)
                .cloned()
                .unwrap()
        };
        assert_eq!(of("b").inserted, [(2, 7)]); // 「를」 → 「와 바닥을」
        assert_eq!(of("b").deleted, [(2, "를".to_string())]);
        // 「큰 」 · 「,」(줄 끝 「.」은 다음 줄 「덮개.」의 마침표와 짝지어진다) · 새 줄 「덮개」.
        assert_eq!(of("e").inserted, [(0, 2), (13, 14)]);
        assert!(of("e").deleted.is_empty());
        assert_eq!(of("e2").inserted, [(0, 2)]);
        assert_eq!(of("h").inserted, [(0, 5)]);
        assert!(
            marks.paragraphs.iter().all(|m| m.id != "g"),
            "안 바뀐 청구항 2"
        );
        // 서식만 바꿈 → 글 차이 없이 바뀐 문단.
        let mut styled = before.clone();
        styled[6].spans = vec![crate::EditorSpan {
            start: 0,
            end: 3,
            style: 2,
            size: None,
        }];
        let marks = amendment_marks(&before, &styled);
        assert_eq!(marks.changed, ["청구항 2"]);
        assert_eq!(
            marks.paragraphs,
            [ParagraphMark {
                id: "g".into(),
                inserted: vec![],
                deleted: vec![]
            }]
        );
    }
}
