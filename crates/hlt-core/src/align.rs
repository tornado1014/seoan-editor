//! 문장 가르기 + 추정 정렬: 짝 없는 원문(워드 · PDF)과 편집 중인 문서를 문장 단위로 짝짓는다.
//! 틀린 문장 짝은 짝이 없는 것보다 나쁘다 — 문장 짝은 두 문단의 문장 수가 같을 때만, 아니면 문단 짝 하나, 그것도 자신 없으면 짝 없음.

use serde::{Deserialize, Serialize};

use std::collections::{BTreeMap, HashMap};

use crate::{EditorParagraph, paragraph_keys, source_keys};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Ko,
    En,
}

/// 문장 범위. ⚠ 단위는 **유니코드 글자(char) 수** — 바이트도 UTF-16 칸도 아니다(TS 는 `[...text]` 로 센다).
///
/// `claim` 이면 구성요소 줄(`;` · 줄바꿈)이 단위. 아니면 `.` `?` `!` 뒤 공백/끝에서 끊되 괄호 안 · 영문 약어 뒤는 안 끊는다.
pub fn sentences(text: &str, lang: Lang, claim: bool) -> Vec<(u32, u32)> {
    let c: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut push = |start: usize, end: usize| {
        let (mut s, mut e) = (start, end);
        while s < e && c[s].is_whitespace() {
            s += 1;
        }
        while e > s && c[e - 1].is_whitespace() {
            e -= 1;
        }
        if s < e {
            out.push((s as u32, e as u32));
        }
    };
    let (mut start, mut depth) = (0, 0_i32);
    for (i, &ch) in c.iter().enumerate() {
        if i < start {
            continue; // 앞 단위가 삼킨 접속어
        }
        match ch {
            '(' | '[' | '（' => depth += 1,
            ')' | ']' | '）' => depth = (depth - 1).max(0),
            _ => {}
        }
        let end = if claim {
            match ch {
                ';' | '；' if depth == 0 => Some(i + 1),
                '\n' => Some(i),
                _ => None,
            }
        } else if matches!(ch, '.' | '?' | '!' | '。') && depth == 0 && ends_sentence(&c, i, lang)
        {
            Some(i + 1)
        } else {
            None
        };
        if let Some(mut end) = end {
            if claim && ch != '\n' {
                end += trailing_connector(&c[end..]);
            }
            push(start, end);
            start = end.max(i + 1);
        }
    }
    push(start, c.len());
    out
}

/// `;` 바로 뒤에 붙는 접속어(「및」 「또는」 · and · or) 길이 — 앞 구성요소 줄에 딸린다(줄바꿈 앞일 때만).
fn trailing_connector(rest: &[char]) -> usize {
    let spaces = rest.iter().take_while(|c| **c == ' ').count();
    let word: String = rest[spaces..]
        .iter()
        .take_while(|c| !c.is_whitespace())
        .collect();
    let after = rest.get(spaces + word.chars().count());
    let ok =
        ["및", "또는", "and", "or"].contains(&word.as_str()) && after.is_none_or(|c| *c == '\n');
    if ok { spaces + word.chars().count() } else { 0 }
}

/// 영문 약어(소문자 · 마침표 포함) — 뒤에서 안 끊는다.
const ABBREVIATIONS: [&str; 16] = [
    "fig.", "figs.", "e.g.", "i.e.", "etc.", "no.", "nos.", "al.", "approx.", "vs.", "cf.", "dr.",
    "inc.", "ltd.", "co.", "u.s.",
];

/// `c[i]` 가 `.` `?` `!` 일 때 거기서 문장이 끝나는가.
fn ends_sentence(c: &[char], i: usize, lang: Lang) -> bool {
    if c.get(i + 1).is_some_and(|next| !next.is_whitespace()) {
        return false;
    }
    if c[i] != '.' {
        return true;
    }
    let from = c[..i]
        .iter()
        .rposition(|ch| ch.is_whitespace())
        .map_or(0, |at| at + 1);
    let token: String = c[from..=i]
        .iter()
        .skip_while(|ch| !ch.is_alphanumeric())
        .collect();
    if ABBREVIATIONS.contains(&token.to_lowercase().as_str()) {
        return false;
    }
    if lang == Lang::En {
        let mut token_chars = token.chars();
        let single_capital =
            token_chars.next().is_some_and(char::is_uppercase) && token_chars.as_str() == ".";
        let next_lower = c[i + 1..]
            .iter()
            .find(|ch| !ch.is_whitespace())
            .is_some_and(|ch| ch.is_lowercase());
        return !single_capital && !next_lower;
    }
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourcePara {
    pub key: Option<String>,
    pub text: String,
    /// 문장 범위 — ⚠ 글자(유니코드 스칼라) 단위 `[start, end)`.
    pub sentences: Vec<(u32, u32)>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Grade {
    Sentence,
    Paragraph,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pair {
    pub target_id: String,
    pub target_sentence: Option<u32>,
    pub source_paragraph: u32,
    pub source_sentence: Option<u32>,
    pub grade: Grade,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Alignment {
    pub source: Vec<SourcePara>,
    /// 글이 있는 모든 편집 문서 문단(짝 없는 것 포함) · 문서 차례 · 범위는 ⚠ 글자(char) 단위.
    pub target_sentences: Vec<(String, Vec<(u32, u32)>)>,
    pub pairs: Vec<Pair>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub target_id: String,
    pub target_sentence: u32,
    pub source_paragraph: u32,
    pub source_sentence: u32,
}

/// 짝 없는 재료(워드 · PDF): 닻(손 고정 · 식별번호 · 청구항 번호 · 표제) → 닻 사이 차례 → 문장 수가 같을 때만 문장 짝.
/// 편집 문서는 국문, 원문은 영문으로 본다.
pub fn align_inferred(
    target: &[EditorParagraph],
    source_texts: &[String],
    pins: &[Pin],
) -> Alignment {
    let (nt, ns) = (target.len(), source_texts.len());
    let source_refs: Vec<&str> = source_texts.iter().map(String::as_str).collect();
    let skeys = source_keys(&source_refs);
    let tkeys = paragraph_keys(target);
    let sheads: Vec<Option<u8>> = source_texts.iter().map(|t| source_heading(t)).collect();
    let theads: Vec<Option<u8>> = target.iter().map(|p| target_heading(&p.text)).collect();

    // 원문 청구항 묶음: 번호가 매겨진 첫 줄 + 뒤따르는 번호 없는 줄(다음 번호 · 표제 앞까지).
    let mut sclaim: Vec<Option<u32>> = vec![None; ns];
    let mut current = None;
    for i in 0..ns {
        match skeys[i].as_deref() {
            Some(key) if key.starts_with("claim:") => current = key[6..].parse().ok(),
            Some(_) => current = None,
            None if sheads[i].is_some() => current = None,
            None => {}
        }
        sclaim[i] = current;
    }
    // 편집 문서 청구항 본문(표제 줄은 뺀다).
    let tclaim: Vec<Option<u32>> = (0..nt)
        .map(|i| {
            let key = tkeys[i].as_deref()?.strip_prefix("claim:")?;
            (!target[i].text.trim().starts_with('【')).then(|| key.parse().ok())?
        })
        .collect();

    let tsent: Vec<Vec<(u32, u32)>> = (0..nt)
        .map(|i| {
            let text = &target[i].text;
            if text.chars().all(|c| c == '_' || c.is_whitespace()) {
                Vec::new()
            } else {
                sentences(text, Lang::Ko, tclaim[i].is_some())
            }
        })
        .collect();
    let ssent: Vec<Vec<(u32, u32)>> = (0..ns)
        .map(|i| sentences(&source_texts[i], Lang::En, sclaim[i].is_some()))
        .collect();

    let is_claim_key = |k: &Option<String>| k.as_deref().is_some_and(|k| k.starts_with("claim:"));
    let tcand = |i: usize| {
        !tsent[i].is_empty()
            && !is_claim_key(&tkeys[i])
            && theads[i].is_none()
            && !target[i].text.trim().starts_with('【')
    };
    let scand = |i: usize| !ssent[i].is_empty() && sclaim[i].is_none() && sheads[i].is_none();

    // 닻: 센 것부터 — 손 고정 · 식별번호 · 표제. 차례를 어기는 것은 버린다.
    let mut anchors: BTreeMap<usize, usize> = BTreeMap::new();
    let mut pinned: HashMap<usize, Vec<(u32, u32)>> = HashMap::new();
    for pin in pins {
        let (Some(t), s) = (
            target.iter().position(|p| p.id == pin.target_id),
            pin.source_paragraph as usize,
        ) else {
            continue;
        };
        if s >= ns || !tcand(t) || !scand(s) || !add_anchor(&mut anchors, t, s) {
            continue;
        }
        if (pin.target_sentence as usize) < tsent[t].len()
            && (pin.source_sentence as usize) < ssent[s].len()
        {
            pinned
                .entry(t)
                .or_default()
                .push((pin.target_sentence, pin.source_sentence));
        }
    }
    let unique = |keys: &[Option<String>], key: &str| {
        let mut at = keys
            .iter()
            .enumerate()
            .filter(|(_, k)| k.as_deref() == Some(key));
        let first = at.next()?.0;
        at.next().is_none().then_some(first)
    };
    for (t, key) in tkeys.iter().enumerate() {
        let Some(key) = key.as_deref().filter(|k| !k.starts_with("claim:")) else {
            continue;
        };
        if let (Some(t), Some(s)) = (unique(&tkeys, key).filter(|&u| u == t), unique(&skeys, key))
            && tcand(t)
            && scand(s)
        {
            add_anchor(&mut anchors, t, s);
        }
    }
    for kind in 0..HEADINGS {
        let only = |heads: &[Option<u8>]| {
            let mut at = heads.iter().enumerate().filter(|(_, h)| **h == Some(kind));
            let first = at.next()?.0;
            at.next().is_none().then_some(first)
        };
        if let (Some(t), Some(s)) = (only(&theads), only(&sheads)) {
            add_anchor(&mut anchors, t, s);
        }
    }

    let mut out: Vec<(usize, Pair)> = Vec::new();
    for (&t, &s) in &anchors {
        if theads[t].is_none() {
            let mut at = pinned.remove(&t).unwrap_or_default();
            at.sort_unstable();
            emit(
                &mut out,
                target,
                (t, s),
                (tsent[t].len(), ssent[s].len()),
                &at,
            );
        }
    }
    // 닻 사이 구간: 문단 수가 같으면 차례로, 다르면 숫자 열이 유일하게 같은 것만.
    let mut bounds: Vec<(i64, i64)> = vec![(-1, -1)];
    bounds.extend(anchors.iter().map(|(&t, &s)| (t as i64, s as i64)));
    bounds.push((nt as i64, ns as i64));
    for w in bounds.windows(2) {
        let ts: Vec<usize> = ((w[0].0 + 1) as usize..w[1].0 as usize)
            .filter(|&i| tcand(i))
            .collect();
        let ss: Vec<usize> = ((w[0].1 + 1) as usize..w[1].1 as usize)
            .filter(|&i| scand(i))
            .collect();
        let pairs: Vec<(usize, usize)> = if ts.len() == ss.len() {
            ts.into_iter().zip(ss).collect()
        } else {
            let tn: Vec<Vec<String>> = ts.iter().map(|&i| numbers(&target[i].text)).collect();
            let sn: Vec<Vec<String>> = ss.iter().map(|&i| numbers(&source_texts[i])).collect();
            let mut found = Vec::new();
            let mut last: Option<usize> = None;
            for (a, seq) in tn.iter().enumerate().filter(|(_, q)| !q.is_empty()) {
                let same_t = tn.iter().filter(|q| *q == seq).count();
                let mut same_s = sn.iter().enumerate().filter(|(_, q)| *q == seq);
                if let (1, Some((b, _)), None) = (same_t, same_s.next(), same_s.next().map(|_| ()))
                    && last.is_none_or(|l| b > l)
                {
                    last = Some(b);
                    found.push((ts[a], ss[b]));
                }
            }
            found
        };
        for (t, s) in pairs {
            emit(
                &mut out,
                target,
                (t, s),
                (tsent[t].len(), ssent[s].len()),
                &[],
            );
        }
    }

    // 청구항: 번호가 같은 묶음끼리 — 구성요소 줄 수가 같을 때만 줄 짝, 아니면 묶음 첫 문단끼리 문단 짝 하나.
    let mut numbers_seen: Vec<u32> = tclaim.iter().flatten().copied().collect();
    numbers_seen.dedup();
    for number in numbers_seen {
        let ts: Vec<usize> = (0..nt).filter(|&i| tclaim[i] == Some(number)).collect();
        let starts: Vec<usize> = (0..ns)
            .filter(|&i| skeys[i].as_deref() == Some(&format!("claim:{number}")))
            .collect();
        let [start] = starts[..] else { continue };
        let ss: Vec<usize> = (start..ns)
            .take_while(|&i| i == start || sclaim[i] == Some(number))
            .filter(|&i| !ssent[i].is_empty())
            .collect();
        let tunits: Vec<(usize, u32)> = ts
            .iter()
            .flat_map(|&i| (0..tsent[i].len() as u32).map(move |k| (i, k)))
            .collect();
        let sunits: Vec<(usize, u32)> = ss
            .iter()
            .flat_map(|&i| (0..ssent[i].len() as u32).map(move |k| (i, k)))
            .collect();
        if !tunits.is_empty() && tunits.len() == sunits.len() {
            for (&(t, tk), &(s, sk)) in tunits.iter().zip(&sunits) {
                out.push((
                    t,
                    Pair {
                        target_id: target[t].id.clone(),
                        target_sentence: Some(tk),
                        source_paragraph: s as u32,
                        source_sentence: Some(sk),
                        grade: Grade::Sentence,
                    },
                ));
            }
        } else if let (Some(&t), Some(&s)) =
            (ts.iter().find(|&&i| !tsent[i].is_empty()), ss.first())
        {
            out.push((
                t,
                Pair {
                    target_id: target[t].id.clone(),
                    target_sentence: None,
                    source_paragraph: s as u32,
                    source_sentence: None,
                    grade: Grade::Paragraph,
                },
            ));
        }
    }
    out.sort_by_key(|(t, _)| *t);

    Alignment {
        source: (0..ns)
            .map(|i| SourcePara {
                key: skeys[i].clone(),
                text: source_texts[i].clone(),
                sentences: ssent[i].clone(),
            })
            .collect(),
        target_sentences: (0..nt)
            .filter(|&i| !tsent[i].is_empty())
            .map(|i| (target[i].id.clone(), tsent[i].clone()))
            .collect(),
        pairs: out.into_iter().map(|(_, p)| p).collect(),
    }
}

/// 닻 하나를 더한다 — 이미 있는 닻과 차례가 어긋나면 거절(false).
fn add_anchor(anchors: &mut BTreeMap<usize, usize>, t: usize, s: usize) -> bool {
    if let Some(&have) = anchors.get(&t) {
        return have == s;
    }
    if anchors.range(..t).next_back().is_some_and(|(_, &p)| p >= s)
        || anchors.range(t + 1..).next().is_some_and(|(_, &n)| n <= s)
    {
        return false;
    }
    anchors.insert(t, s);
    true
}

/// 짝지은 문단 하나 → 문장 짝(문장 수가 같을 때만) 또는 문단 짝 하나. 손 고정 문장이 있으면 그 앞 · 뒤를 각각 같은 규칙으로.
fn emit(
    out: &mut Vec<(usize, Pair)>,
    target: &[EditorParagraph],
    (t, s): (usize, usize),
    (tn, sn): (usize, usize),
    pins: &[(u32, u32)],
) {
    let pair = |tk: Option<u32>, sk: Option<u32>, grade| {
        (
            t,
            Pair {
                target_id: target[t].id.clone(),
                target_sentence: tk,
                source_paragraph: s as u32,
                source_sentence: sk,
                grade,
            },
        )
    };
    let mut kept: Vec<(u32, u32)> = Vec::new();
    for &(a, b) in pins {
        if kept.last().is_none_or(|&(la, lb)| a > la && b > lb) {
            kept.push((a, b));
        }
    }
    if kept.is_empty() {
        if tn == sn {
            out.extend((0..tn as u32).map(|k| pair(Some(k), Some(k), Grade::Sentence)));
        } else {
            out.push(pair(None, None, Grade::Paragraph));
        }
        return;
    }
    let (mut ta, mut sa) = (0_u32, 0_u32);
    kept.push((tn as u32, sn as u32)); // 끝 울타리: 뒤 구간을 같은 식으로 처리한다
    for (i, &(a, b)) in kept.iter().enumerate() {
        if a - ta == b - sa {
            out.extend((0..a - ta).map(|k| pair(Some(ta + k), Some(sa + k), Grade::Sentence)));
        }
        if i + 1 < kept.len() {
            out.push(pair(Some(a), Some(b), Grade::Sentence));
        }
        (ta, sa) = (a + 1, b + 1);
    }
}

/// 문단 안 `\d+[a-z]?` 를 차례로 모은 열(「도 3」 ≡ 「FIG. 3」).
fn numbers(text: &str) -> Vec<String> {
    let c: Vec<char> = text.chars().collect();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < c.len() {
        if c[i].is_ascii_digit() {
            let from = i;
            while i < c.len() && c[i].is_ascii_digit() {
                i += 1;
            }
            if i < c.len() && c[i].is_ascii_lowercase() {
                i += 1;
            }
            out.push(c[from..i].iter().collect());
        } else {
            i += 1;
        }
    }
    out
}

const HEADINGS: u8 = 10;

/// 표제 닻 표: 국문 ↔ 영문(소문자 · 끝 콜론 무시).
fn target_heading(text: &str) -> Option<u8> {
    let name = text.trim().strip_prefix('【')?.strip_suffix('】')?.trim();
    Some(match name {
        "기술분야" => 0,
        "배경기술" => 1,
        "발명의 내용" => 2,
        "해결하려는 과제" => 3,
        "과제의 해결 수단" => 4,
        "발명의 효과" => 5,
        "도면의 간단한 설명" => 6,
        "발명을 실시하기 위한 구체적인 내용" => 7,
        "청구범위" | "특허청구범위" => 8,
        "요약서" => 9,
        _ => return None,
    })
}

fn source_heading(text: &str) -> Option<u8> {
    let lower = text.trim().trim_end_matches(':').trim().to_lowercase();
    Some(match lower.as_str() {
        "technical field" => 0,
        "background" | "background art" => 1,
        "summary" => 2,
        "technical problem" => 3,
        "solution" | "technical solution" => 4,
        "advantageous effects" => 5,
        "brief description of drawings" | "brief description of the drawings" => 6,
        "detailed description" => 7,
        "claims" => 8,
        l if l.starts_with("what is claimed") => 8,
        "abstract" => 9,
        _ => return None,
    })
}
