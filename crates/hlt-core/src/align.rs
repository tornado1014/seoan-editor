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
const ABBREVIATIONS: &[&str] = &[
    "fig.", "figs.", "e.g.", "i.e.", "etc.", "no.", "nos.", "al.", "approx.", "vs.", "cf.", "dr.",
    "inc.", "ltd.", "co.", "u.s.", "pat.", "pats.", "appl.", "ser.", "eq.", "eqs.", "sec.", "ref.",
    "refs.", "ex.", "vol.", "pp.", "p.", "ca.", "wt.", "temp.",
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
    /// 적용하지 못한 손 고정(청구항 번호가 다른 것 · 범위 밖 · 모르는 문단 · 더 센 닻이나 앞선 고정과 차례가 어긋난 것).
    pub dropped_pins: Vec<Pin>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub target_id: String,
    pub target_sentence: u32,
    pub source_paragraph: u32,
    pub source_sentence: u32,
}

/// 편집 문서 청구항 본문(표제 줄은 뺀다): 문단 → 청구항 번호.
pub(crate) fn target_claims(
    target: &[EditorParagraph],
    tkeys: &[Option<String>],
) -> Vec<Option<u32>> {
    (0..target.len())
        .map(|i| {
            let key = tkeys[i].as_deref()?.strip_prefix("claim:")?;
            (!target[i].text.trim().starts_with('【')).then(|| key.parse().ok())?
        })
        .collect()
}

/// 편집 문서 문단별 문장 범위(글자 단위) — 글이 없거나 그림 자리표(`_`)뿐인 문단은 빈 목록.
pub(crate) fn target_sentence_ranges(
    target: &[EditorParagraph],
    tclaim: &[Option<u32>],
) -> Vec<Vec<(u32, u32)>> {
    (0..target.len())
        .map(|i| {
            let text = &target[i].text;
            if text.chars().all(|c| c == '_' || c.is_whitespace()) {
                Vec::new()
            } else {
                sentences(text, Lang::Ko, tclaim[i].is_some())
            }
        })
        .collect()
}

/// 짝 없는 재료(워드 · PDF): 닻(손 고정 · 식별번호 · 청구항 번호 · 표제) → 닻 사이 차례 → 문장 수가 같을 때만 문장 짝.
/// 편집 문서는 국문, 원문은 영문으로 본다.
pub fn align_inferred(
    target: &[EditorParagraph],
    source_texts: &[String],
    pins: &[Pin],
) -> Alignment {
    let r = infer(target, source_texts, pins, None);
    Alignment {
        source: (0..source_texts.len())
            .map(|i| SourcePara {
                key: r.skeys[i].clone(),
                text: source_texts[i].clone(),
                sentences: r.ssent[i].clone(),
            })
            .collect(),
        target_sentences: (0..target.len())
            .filter(|&i| !r.tsent[i].is_empty())
            .map(|i| (target[i].id.clone(), r.tsent[i].clone()))
            .collect(),
        pairs: r.pairs.into_iter().map(|(_, p)| p).collect(),
        dropped_pins: r.dropped.into_iter().map(|n| pins[n].clone()).collect(),
    }
}

/// `align_xliff` 의 2단계가 `infer` 에 밖에서 주는 것 — 원문 문장 범위(세그먼트 경계) · 1단계가 이미 쓴 자리.
pub(crate) struct Given<'a> {
    /// 원문 문단별 문장 범위(글자 단위 · 글은 `source_texts`).
    pub ssent: &'a [Vec<(u32, u32)>],
    /// 편집 문단 중 짝을 받을 수 있는 것(1단계에서 문장 짝도 문단 짝도 못 받은 것).
    pub free: &'a [bool],
    /// 1단계가 이미 쓴 원문 문단 — 문장 짝 금지(문단 짝만) · 차례 채움 후보 아님.
    pub used: &'a [bool],
    /// 1단계 짝이 가리키는 모든 (편집 문단, 원문 문단) — 구간을 가르는 가장 센 경계(짝은 더 만들지 않는다).
    pub fixed: &'a [(usize, usize)],
}

pub(crate) struct Inferred {
    pub skeys: Vec<Option<String>>,
    pub ssent: Vec<Vec<(u32, u32)>>,
    pub tsent: Vec<Vec<(u32, u32)>>,
    /// (편집 문단 차례, 짝) · 편집 문서 차례.
    pub pairs: Vec<(usize, Pair)>,
    /// 못 쓴 손 고정(입력 번호 · 오름차순).
    pub dropped: Vec<usize>,
}

pub(crate) fn infer(
    target: &[EditorParagraph],
    source_texts: &[String],
    pins: &[Pin],
    given: Option<&Given>,
) -> Inferred {
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
    let tclaim = target_claims(target, &tkeys);
    let tsent = target_sentence_ranges(target, &tclaim);
    let ssent: Vec<Vec<(u32, u32)>> = match given {
        Some(g) => g.ssent.to_vec(),
        None => (0..ns)
            .map(|i| sentences(&source_texts[i], Lang::En, sclaim[i].is_some()))
            .collect(),
    };
    let free = |i: usize| given.is_none_or(|g| g.free[i]);
    let used = |i: usize| given.is_some_and(|g| g.used[i]);

    let is_claim_key = |k: &Option<String>| k.as_deref().is_some_and(|k| k.starts_with("claim:"));
    let tcand = |i: usize| {
        free(i)
            && !tsent[i].is_empty()
            && !is_claim_key(&tkeys[i])
            && theads[i].is_none()
            && !target[i].text.trim().starts_with('【')
    };
    let scand = |i: usize| !ssent[i].is_empty() && sclaim[i].is_none() && sheads[i].is_none();

    // 닻: 센 것부터 — 손 고정 · 식별번호 · 표제. 차례를 어기는 것은 버린다(손 고정은 버린 것을 알린다).
    let cx = Ctx {
        target,
        source: source_texts,
        tsent: &tsent,
        ssent: &ssent,
        used: given.map_or(&[], |g| g.used),
    };
    let mut anchors: BTreeMap<usize, usize> = BTreeMap::new();
    // 1단계 짝(밖에서 줌)이 가리키는 닻 — 구간만 가르고 짝은 내보내지 않는다.
    // 한 문단이 여러 묶음에(또는 한 묶음이 여러 문단에) 맞은 1단계 짝은 닻이 못 되는 것도 경계로는 남긴다.
    let mut fixed_t: Vec<usize> = Vec::new();
    let mut extra: Vec<(usize, usize)> = Vec::new();
    for &(t, s) in given.map_or(&[][..], |g| g.fixed) {
        if add_anchor(&mut anchors, t, s) {
            fixed_t.push(t);
        } else {
            extra.push((t, s));
        }
    }
    let mut pinned: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();
    let mut claim_pins: HashMap<u32, Vec<ClaimPin>> = HashMap::new();
    let mut dropped: Vec<usize> = Vec::new();
    for (n, pin) in pins.iter().enumerate() {
        let s = pin.source_paragraph as usize;
        let t = target.iter().position(|p| p.id == pin.target_id);
        let Some(t) = t.filter(|_| s < ns) else {
            dropped.push(n);
            continue;
        };
        let (tk, sk) = (pin.target_sentence as usize, pin.source_sentence as usize);
        if tk >= tsent[t].len() || sk >= ssent[s].len() {
            dropped.push(n);
            continue;
        }
        match (tclaim[t], sclaim[s]) {
            (Some(a), Some(b)) if a == b => {
                claim_pins.entry(a).or_default().push((n, t, tk, s, sk))
            }
            (None, None)
                if tcand(t)
                    && scand(s)
                    && add_anchor(&mut anchors, t, s)
                    && add_ordered(pinned.entry(t).or_default(), (tk, sk)) => {}
            _ => dropped.push(n),
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
    // 식별번호가 저장돼 있지 않은 문서의 짝 이름은 차례로 매긴 것 — 구간별 개수가 맞을 때만 직접 닻이다.
    let numberless = target.iter().all(|p| p.number.is_none());
    let mut key_t: Vec<usize> = Vec::new();
    for (t, key) in tkeys.iter().enumerate() {
        let Some(key) = key.as_deref().filter(|k| !k.starts_with("claim:")) else {
            continue;
        };
        if let (Some(t), Some(s)) = (unique(&tkeys, key).filter(|&u| u == t), unique(&skeys, key))
            && tcand(t)
            && scand(s)
        {
            let stronger = anchors.contains_key(&t);
            if add_anchor(&mut anchors, t, s) && !stronger {
                key_t.push(t);
            }
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

    // 구간 경계: 닻 + 닻이 못 된 1단계 짝(차례가 어긋나지 않는 것만 · 같은 칸 경계는 허용).
    let mut points: Vec<(usize, usize)> = anchors.iter().map(|(&t, &s)| (t, s)).collect();
    for &(t, s) in &extra {
        if points
            .iter()
            .all(|&(a, b)| a == t || (a < t && b <= s) || (a > t && b >= s))
        {
            points.push((t, s));
        }
    }
    points.sort_unstable();
    points.dedup();
    // 차례로 매긴 짝 이름 닻: 더 센 경계(키 닻 제외) 사이 구간마다 편집 쪽 이름 문단 수 ≠ 원문 인쇄 번호 문단 수면 약한 닻.
    let mut weak_t: Vec<usize> = Vec::new();
    if numberless && !key_t.is_empty() {
        let mut strong: Vec<(i64, i64)> = vec![(-1, -1)];
        strong.extend(
            points
                .iter()
                .filter(|(t, _)| !key_t.contains(t))
                .map(|&(t, s)| (t as i64, s as i64)),
        );
        strong.push((nt as i64, ns as i64));
        for w in strong.windows(2) {
            let tr = (w[0].0 + 1) as usize..w[1].0 as usize;
            let sr = (w[0].1 + 1) as usize..w[1].1 as usize;
            let keyed = |keys: &[Option<String>], i: usize| {
                keys[i].as_deref().is_some_and(|k| k.starts_with("p:"))
            };
            let tc = tr
                .clone()
                .filter(|&i| keyed(&tkeys, i) && !tsent[i].is_empty() && free(i))
                .count();
            let sc = sr.filter(|&i| keyed(&skeys, i) && !used(i)).count();
            if tc != sc {
                weak_t.extend(key_t.iter().copied().filter(|t| tr.contains(t)));
            }
        }
    }

    let mut out: Vec<(usize, Pair)> = Vec::new();
    for (&t, &s) in &anchors {
        if theads[t].is_none() && !fixed_t.contains(&t) {
            emit_paragraph(
                &mut out,
                &cx,
                (t, s),
                pinned.remove(&t).unwrap_or_default(),
                weak_t.contains(&t),
            );
        }
    }
    // 닻 사이 구간: 문단 수가 같으면 차례로, 다르면 숫자 열이 유일하게 같은 것만.
    let mut bounds: Vec<(i64, i64)> = vec![(-1, -1)];
    bounds.extend(points.iter().map(|&(t, s)| (t as i64, s as i64)));
    bounds.push((nt as i64, ns as i64));
    for w in bounds.windows(2) {
        let ts: Vec<usize> = ((w[0].0 + 1) as usize..w[1].0 as usize)
            .filter(|&i| tcand(i))
            .collect();
        let ss: Vec<usize> = ((w[0].1 + 1) as usize..w[1].1 as usize)
            .filter(|&i| scand(i) && !used(i))
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
            emit_paragraph(&mut out, &cx, (t, s), Vec::new(), false);
        }
    }

    // 청구항: 번호가 같은 묶음끼리(묶음을 넘어 짝짓지 않는다) — 구성요소 줄 수가 같을 때만 줄 짝, 아니면 문단 짝 하나.
    let mut numbers_seen: Vec<u32> = tclaim.iter().flatten().copied().collect();
    numbers_seen.sort_unstable();
    numbers_seen.dedup();
    for number in numbers_seen {
        let mut claim_pins = claim_pins.remove(&number).unwrap_or_default();
        let ts: Vec<usize> = (0..nt).filter(|&i| tclaim[i] == Some(number)).collect();
        if !ts.iter().all(|&i| free(i)) {
            dropped.extend(claim_pins.iter().map(|p| p.0));
            continue;
        }
        let starts: Vec<usize> = (0..ns)
            .filter(|&i| skeys[i].as_deref() == Some(&format!("claim:{number}")))
            .collect();
        let [start] = starts[..] else {
            dropped.extend(claim_pins.iter().map(|p| p.0));
            continue;
        };
        let ss: Vec<usize> = (start..ns)
            .take_while(|&i| i == start || sclaim[i] == Some(number))
            .filter(|&i| !ssent[i].is_empty())
            .collect();
        let units = |paragraphs: &[usize], sent: &[Vec<(u32, u32)>]| -> Vec<(usize, u32)> {
            paragraphs
                .iter()
                .flat_map(|&i| (0..sent[i].len() as u32).map(move |k| (i, k)))
                .collect()
        };
        let (tunits, sunits) = (units(&ts, &tsent), units(&ss, &ssent));
        if tunits.is_empty() || sunits.is_empty() {
            dropped.extend(claim_pins.iter().map(|p| p.0));
            continue;
        }
        // 번호만 같은 묶음이 1단계 짝 · 닻 · 손 고정을 가로지르면 번호 증거 자체가 깨진 것 — 문단 짝도 내지 않는다.
        let (t0, t1) = (tunits[0].0, tunits[tunits.len() - 1].0);
        let (s0, s1) = (sunits[0].0, sunits[sunits.len() - 1].0);
        let crossing = points.iter().any(|&(a, b)| {
            let inside = (t0..=t1).contains(&a) && (s0..=s1).contains(&b);
            let ordered = (a < t0 && b < s0) || (a > t1 && b > s1);
            !(inside || ordered)
        });
        if crossing {
            dropped.extend(claim_pins.iter().map(|p| p.0));
            continue;
        }
        let mut at: Vec<(usize, usize)> = Vec::new();
        claim_pins.retain(|&(n, t, tk, s, sk)| {
            let (a, b) = (
                tunits.iter().position(|&u| u == (t, tk as u32)),
                sunits.iter().position(|&u| u == (s, sk as u32)),
            );
            let kept = a.zip(b).is_some_and(|ab| add_ordered(&mut at, ab));
            if !kept {
                dropped.push(n);
            }
            kept
        });
        at.sort_unstable();
        emit_group(
            &mut out,
            &cx,
            &tunits,
            &sunits,
            &at,
            (tunits[0].0, sunits[0].0),
            true,
        );
    }
    out.sort_by_key(|(t, _)| *t);
    dropped.sort_unstable();

    Inferred {
        skeys,
        ssent,
        tsent,
        pairs: out,
        dropped,
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

/// 청구항 안 손 고정: (입력 번호, 편집 문단, 편집 문장, 원문 문단, 원문 문장).
type ClaimPin = (usize, usize, usize, usize, usize);

struct Ctx<'a> {
    target: &'a [EditorParagraph],
    source: &'a [String],
    tsent: &'a [Vec<(u32, u32)>],
    ssent: &'a [Vec<(u32, u32)>],
    /// 문장 짝 금지인 원문 문단(비어 있으면 없음).
    used: &'a [bool],
}

/// 손 고정 문장 쌍을 더한다 — 이미 받은 것들과 양쪽 차례가 같아야 한다(어긋나면 false).
fn add_ordered<T: Copy + Ord>(have: &mut Vec<(T, T)>, new: (T, T)) -> bool {
    let ok = have
        .iter()
        .all(|&(a, b)| (new.0 < a && new.1 < b) || (new.0 > a && new.1 > b));
    if ok {
        have.push(new);
    }
    ok
}

/// 짝지은 문단 하나(문장 칸 = 문장 번호) → 문장 짝 또는 문단 짝 하나.
/// `weak`: 믿기 어려운 닻(차례로 매긴 짝 이름 · 구간 개수 불일치)으로 짝지은 문단 — 문단 전체의 숫자 증거가 양쪽 비어 있지 않고
/// 같을 때만 문장 짝 후보(그 뒤 기존 조건).
fn emit_paragraph(
    out: &mut Vec<(usize, Pair)>,
    cx: &Ctx,
    (t, s): (usize, usize),
    mut pins: Vec<(usize, usize)>,
    weak: bool,
) {
    pins.sort_unstable();
    let units = |n: usize, p: usize| (0..n as u32).map(|k| (p, k)).collect::<Vec<_>>();
    let (tu, su) = (units(cx.tsent[t].len(), t), units(cx.ssent[s].len(), s));
    if weak {
        let all = |texts: &[String]| texts.iter().flat_map(|x| evidence(x)).collect::<Vec<_>>();
        let te = all(&tu
            .iter()
            .map(|&(p, k)| slice(&cx.target[p].text, cx.tsent[p][k as usize]))
            .collect::<Vec<_>>());
        let se = all(&su
            .iter()
            .map(|&(p, k)| {
                strip_number_prefix(&slice(&cx.source[p], cx.ssent[p][k as usize])).to_string()
            })
            .collect::<Vec<_>>());
        if te.is_empty() || te != se {
            return paragraph_pair(out, cx, (t, s));
        }
    }
    emit_group(out, cx, &tu, &su, &pins, (t, s), false);
}

/// 문장 칸 목록(문단 하나 또는 청구항 하나) 짝짓기: 문장 수가 같고 숫자 증거가 어긋나지 않을 때만 문장 짝,
/// 아니면 `fallback` 문단 짝 하나. 손 고정(문장 칸 번호 쌍)이 있으면 고정 앞 · 뒤를 각각 같은 규칙으로 —
/// 이때 어긋난 구간은 그 구간 짝만 버린다(고정 자체는 남는다). 숫자 검문은 고정 쌍에는 적용하지 않는다.
fn emit_group(
    out: &mut Vec<(usize, Pair)>,
    cx: &Ctx,
    tunits: &[(usize, u32)],
    sunits: &[(usize, u32)],
    pins: &[(usize, usize)],
    fallback: (usize, usize),
    claim: bool,
) {
    if sunits.iter().any(|&(s, _)| cx.used.get(s) == Some(&true)) {
        return paragraph_pair(out, cx, fallback); // 이미 쓴 원문 문단은 문단 짝만
    }
    let pair = |a: usize, b: usize| {
        let ((t, tk), (s, sk)) = (tunits[a], sunits[b]);
        (
            t,
            Pair {
                target_id: cx.target[t].id.clone(),
                target_sentence: Some(tk),
                source_paragraph: s as u32,
                source_sentence: Some(sk),
                grade: Grade::Sentence,
            },
        )
    };
    let consistent = |ta: usize, sa: usize, len: usize| {
        (0..len).all(|k| {
            let ((t, tk), (s, sk)) = (tunits[ta + k], sunits[sa + k]);
            let te = evidence(&slice(&cx.target[t].text, cx.tsent[t][tk as usize]));
            let st = slice(&cx.source[s], cx.ssent[s][sk as usize]);
            let mut st = strip_number_prefix(&st);
            if claim && (s, sk) == sunits[0] {
                // 청구항 묶음의 맨 처음 칸만: 그 「12.」 는 항 번호 — 역문에는 없으니 증거로 세지 않는다.
                // (다른 줄 맨 앞 숫자는 본문이다.)
                st = strip_claim_number(st);
            }
            let tt = slice(&cx.target[t].text, cx.tsent[t][tk as usize]);
            // 도면 · 양태 설명의 앞 번호(「도 2」 ≡ 「FIG. 2」)는 한 자리여도 어긋나면 안 된다(청구항 제외).
            (claim || labels_agree(&tt, st)) && te == evidence(st)
        })
    };
    let mut bounds = pins.to_vec();
    bounds.push((tunits.len(), sunits.len())); // 끝 울타리: 뒤 구간을 같은 식으로 처리한다
    let (mut ta, mut sa) = (0, 0);
    let mut segments = Vec::new();
    for (i, &(a, b)) in bounds.iter().enumerate() {
        if a - ta == b - sa && consistent(ta, sa, a - ta) {
            segments.extend((0..a - ta).map(|k| pair(ta + k, sa + k)));
        } else if pins.is_empty() {
            segments.clear();
            break;
        }
        if i + 1 < bounds.len() {
            segments.push(pair(a, b));
        }
        (ta, sa) = (a + 1, b + 1);
    }
    if segments.is_empty() {
        paragraph_pair(out, cx, fallback);
    } else {
        out.extend(segments);
    }
}

fn paragraph_pair(out: &mut Vec<(usize, Pair)>, cx: &Ctx, (t, s): (usize, usize)) {
    out.push((
        t,
        Pair {
            target_id: cx.target[t].id.clone(),
            target_sentence: None,
            source_paragraph: s as u32,
            source_sentence: None,
            grade: Grade::Paragraph,
        },
    ));
}

fn slice(text: &str, (s, e): (u32, u32)) -> String {
    text.chars()
        .skip(s as usize)
        .take((e - s) as usize)
        .collect()
}

/// 원문 문단 앞 `[0001]` 식별번호를 뺀 글.
fn strip_number_prefix(text: &str) -> &str {
    let rest = text.trim_start();
    match rest.strip_prefix('[').and_then(|r| r.split_once(']')) {
        Some((inner, after))
            if inner.starts_with(|c: char| c.is_ascii_digit())
                && inner.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            after
        }
        _ => text,
    }
}

/// 원문 청구항 첫 줄 앞 「12.」 를 뺀 글.
fn strip_claim_number(text: &str) -> &str {
    let rest = text.trim_start();
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    match rest[digits..].strip_prefix('.') {
        Some(after) if digits > 0 && after.starts_with(char::is_whitespace) => after,
        _ => text,
    }
}

/// 문장 맨 앞 「낱말 + 번호」 꼴(「도 2」 · 「FIG. 2」 · 「양태 2:」 · 「Aspect 2:」) → (낱말, 번호). 낱말 하나(11자 이하) 바로 뒤 숫자만.
fn lead_label(text: &str) -> Option<(String, String)> {
    let c: Vec<char> = text.trim_start().chars().collect();
    let mut i = 0;
    while i < c.len() && (c[i].is_alphabetic() || c[i] == '.') {
        i += 1;
    }
    if i == 0 || i > 11 {
        return None;
    }
    let word: String = c[..i].iter().collect();
    if c.get(i).is_some_and(|ch| ch.is_whitespace()) {
        i += 1;
    }
    let digits: String = c[i..].iter().take_while(|ch| ch.is_ascii_digit()).collect();
    (!digits.is_empty()).then_some((word, digits))
}

/// 「FIG」 · 「FIGS」 · 「FIGs」 낱말로 시작하는가(「FIGURATIVE」 는 아니다).
fn starts_with_fig(text: &str) -> bool {
    let Some(rest) = text.trim_start().strip_prefix("FIG") else {
        return false;
    };
    let rest = rest.strip_prefix(['s', 'S']).unwrap_or(rest);
    !rest.starts_with(char::is_alphabetic)
}

/// 앞 번호가 어긋나면 안 된다 — 한 자리 번호는 숫자 증거가 아니라서 한 칸 밀린 도면 · 양태 설명이 짝 지어진다.
/// 양쪽이 「낱말 + 번호」로 시작할 때만 견준다(「도 6을 참조하면」 ↔ 「Referring to FIG. 6」 은 원문이 낱말로 시작하지
/// 않는다). 원문이 「FIG.」 로 시작하는데 번호가 없으면(PDF 가 번호를 흘린 것) 짝 짓지 않는다.
/// 역문이 낱말 없이 숫자로 시작하거나(「325에서」) 순서 낱말(「제1」 ↔ first)이면 보지 않는다.
fn labels_agree(target: &str, source: &str) -> bool {
    match (lead_label(target), lead_label(source)) {
        (Some((word, number)), Some(s)) if !word.is_empty() && word != "제" => s.1 == number,
        (Some((word, _)), None) if !word.is_empty() && word != "제" => !starts_with_fig(source),
        _ => true,
    }
}

/// 숫자 증거: 글 안 두 자리 이상 숫자 덩이를 차례로(`0.5` · `5,000,000` 은 한 덩이 · 쉼표 뺌). 한 자리는 뺀다 —
/// 국문은 순서를 숫자(제1 · 제2)로, 영문은 낱말(first · second)로 쓴다.
fn evidence(text: &str) -> Vec<String> {
    let c: Vec<char> = text.chars().collect();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < c.len() {
        if !c[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let mut token = String::new();
        while i < c.len() {
            if c[i].is_ascii_digit() {
                token.push(c[i]);
            } else if matches!(c[i], '.' | ',') && c.get(i + 1).is_some_and(char::is_ascii_digit) {
                if c[i] == '.' {
                    token.push('.');
                }
            } else {
                break;
            }
            i += 1;
        }
        if token.len() >= 2 {
            out.push(token);
        }
    }
    out
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
