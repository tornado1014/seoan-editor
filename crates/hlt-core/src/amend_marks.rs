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

/// 보정 전후 대비표 한 줄: 요소 이름 · 보정 전 글 · 보정 후 글(줄은 `\n`) · 지운 글자 범위(보정 전) · 넣은 글자 범위(보정 후).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AmendRow {
    pub label: String,
    pub before: String,
    pub after: String,
    pub deleted: Vec<(usize, usize)>,
    pub inserted: Vec<(usize, usize)>,
}

/// 보정 전후 대비표: 보정 표시와 같은 요소 · 같은 비교로 바뀐 것 · 새 것 · 지운 것만, 상세설명 → 청구항 → 도면 차례.
/// 도면은 그림이라 글 대신 「(그림)」을 쓴다.
pub fn amendment_table(
    before: &[EditorParagraph],
    now: &[EditorParagraph],
    before_figures: &FigureSignatures,
    now_figures: &FigureSignatures,
) -> Vec<AmendRow> {
    let joined = |unit: &Unit| -> String {
        if unit.key.starts_with("figure:") {
            return "(그림)".to_string();
        }
        let texts: Vec<&str> = unit.parts.iter().map(|p| p.text.as_str()).collect();
        texts.join("\n")
    };
    let figures = |unit: &Unit, signatures: &FigureSignatures| -> Vec<Vec<u64>> {
        unit.parts
            .iter()
            .map(|part| signatures.get(&part.id).cloned().unwrap_or_default())
            .collect()
    };
    let compare_figures = !before_figures.is_empty() && !now_figures.is_empty();
    let (old_units, new_units) = (units(before), units(now));
    let mut rows: Vec<((u8, u64, usize), AmendRow)> = Vec::new();
    let order = |unit: &Unit, at: usize| {
        let rank = match unit.key.split(':').next() {
            Some("claim") => 1,
            Some("figure") => 2,
            _ => 0,
        };
        let digits: String = unit
            .key
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(char::is_ascii_digit)
            .collect();
        (rank, digits.parse().unwrap_or(u64::MAX), at)
    };
    for (at, unit) in new_units.iter().enumerate() {
        let after = joined(unit);
        let Some(old) = old_units.iter().find(|old| old.key == unit.key) else {
            let length = after.chars().count();
            rows.push((
                order(unit, at),
                AmendRow {
                    label: unit.label.clone(),
                    inserted: vec![(0, length)],
                    after,
                    ..AmendRow::default()
                },
            ));
            continue;
        };
        let before = joined(old);
        let figure_changed =
            compare_figures && figures(old, before_figures) != figures(unit, now_figures);
        if before == after && !figure_changed {
            continue;
        }
        let (old_chars, new_chars): (Vec<char>, Vec<char>) =
            (before.chars().collect(), after.chars().collect());
        let mut row = AmendRow {
            label: unit.label.clone(),
            ..AmendRow::default()
        };
        let mut matcher = SequenceMatcher::new(&old_chars, &new_chars);
        for op in matcher.get_opcodes() {
            if op.tag == "equal" {
                continue;
            }
            if op.first_end > op.first_start {
                row.deleted.push((op.first_start, op.first_end));
            }
            if op.second_end > op.second_start {
                row.inserted.push((op.second_start, op.second_end));
            }
        }
        row.before = before;
        row.after = after;
        rows.push((order(unit, at), row));
    }
    for (at, old) in old_units.iter().enumerate() {
        if new_units.iter().any(|unit| unit.key == old.key) {
            continue;
        }
        let before = joined(old);
        let length = before.chars().count();
        rows.push((
            order(old, at),
            AmendRow {
                label: old.label.clone(),
                deleted: vec![(0, length)],
                before,
                ..AmendRow::default()
            },
        ));
    }
    rows.sort_by_key(|(key, _)| *key);
    rows.into_iter().map(|(_, row)| row).collect()
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
    // 도면: 【도면】 절의 【도 N】 표제 뒤 · 다음 표제 앞 문단들(그림 문단) — 그림을 바꾼 것은 그림 서명으로 견준다.
    let heading = |text: &str| {
        let text = text.trim();
        text.strip_prefix('【')
            .and_then(|rest| rest.strip_suffix('】'))
            .map(str::to_string)
    };
    if let Some(drawings) = paragraphs
        .iter()
        .position(|p| heading(&p.text).as_deref() == Some("도면"))
    {
        let mut at = drawings + 1;
        while at < paragraphs.len() {
            let Some(name) = heading(&paragraphs[at].text)
                .and_then(|h| h.strip_prefix("도 ").map(str::to_string))
            else {
                at += 1;
                continue;
            };
            let end = (at + 1..paragraphs.len())
                .find(|k| heading(&paragraphs[*k].text).is_some())
                .unwrap_or(paragraphs.len());
            out.push(Unit {
                key: format!("figure:{name}"),
                label: format!("도 {name}"),
                parts: paragraphs[at + 1..end].iter().collect(),
            });
            at = end;
        }
    }
    out
}

/// 번호를 받을 상세설명 문단 차례(【발명의 설명】 안 · 표제와 【발명의 명칭】 글은 빼고 【표 N】 · 【수학식 N】 · 【화학식 N】은 넣는다).
pub(crate) fn description_body(paragraphs: &[EditorParagraph]) -> Vec<usize> {
    let (mut inside, mut title) = (false, false);
    let mut out = Vec::new();
    for (at, paragraph) in paragraphs.iter().enumerate() {
        let text = paragraph.text.trim();
        if let Some(name) = text.strip_prefix('【').and_then(|t| t.strip_suffix('】')) {
            if ["표 ", "수학식 ", "화학식 "]
                .iter()
                .any(|kind| name.starts_with(kind))
            {
                if inside {
                    out.push(at);
                }
                continue;
            }
            match name {
                "발명의 설명" => inside = true,
                "청구범위" | "특허청구범위" | "요약서" | "도면" => inside = false,
                _ => {}
            }
            title = name == "발명의 명칭";
            continue;
        }
        if inside && !title {
            out.push(at);
        }
    }
    out
}

/// 식별번호가 없는 두 판(그냥 HLT — 번호는 제출파일을 만들 때 매긴다)을 견주려고 상세설명 문단에 짝 번호를 매긴다.
/// 두 판의 상세설명 문단(【발명의 설명】 안 · 표제와 【발명의 명칭】 글은 빼고 【표 N】 등은 넣는다) 글을 차례 맞춤해,
/// 짝(같은 글 · 바뀐 글)은 같은 번호(지금 판 차례 `0001`…), 지금 판에만 있는 문단도 그 차례 번호, 앞 판에만 있는 문단은
/// `이전 0003`(앞 판 차례)을 단다. 어느 한쪽에 번호가 하나라도 있으면 그대로 둔다.
pub fn number_unnumbered(before: &mut [EditorParagraph], now: &mut [EditorParagraph]) {
    if before.iter().chain(now.iter()).any(|p| p.number.is_some()) {
        return;
    }
    let (old, new) = (description_body(before), description_body(now));
    let old_texts: Vec<&str> = old.iter().map(|&at| before[at].text.as_str()).collect();
    let new_texts: Vec<&str> = new.iter().map(|&at| now[at].text.as_str()).collect();
    // 같은 앞 · 뒤는 차례대로 짝짓고 가운데만 맞춘다 — 긴 목록 통째로는 「_」(그림) 같은 되풀이 문단에 짝이 엇나갔다
    // (실물: 한 곳 고침에 203 문단이 지운 것 · 넣은 것으로 나왔다).
    let head = old_texts
        .iter()
        .zip(&new_texts)
        .take_while(|(a, b)| a == b)
        .count();
    let tail = old_texts[head..]
        .iter()
        .rev()
        .zip(new_texts[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let mut paired: Vec<Option<usize>> = (0..old.len())
        .map(|j| match j {
            j if j < head => Some(j),
            j if j >= old.len() - tail => Some(j + new.len() - old.len()),
            _ => None,
        })
        .collect();
    let (old_middle, new_middle) = (
        &old_texts[head..old.len() - tail],
        &new_texts[head..new.len() - tail],
    );
    let mut matcher = SequenceMatcher::new(old_middle, new_middle);
    for op in matcher.get_opcodes() {
        let pairs = (op.first_end - op.first_start).min(op.second_end - op.second_start);
        for k in 0..pairs {
            paired[head + op.first_start + k] = Some(head + op.second_start + k);
        }
    }
    for (k, &at) in new.iter().enumerate() {
        now[at].number = Some(format!("{:04}", k + 1));
    }
    for (j, &at) in old.iter().enumerate() {
        before[at].number = Some(match paired[j] {
            Some(k) => format!("{:04}", k + 1),
            None => format!("이전 {:04}", j + 1),
        });
    }
}

/// 문단 id → 그 문단 그림들의 서명(그림 바이트 · 표시 크기). 비면 그림은 견주지 않는다.
pub type FigureSignatures = std::collections::BTreeMap<String, Vec<u64>>;

/// 최종본(`before`) 대비 지금 문단(`now`)의 보정 표시. 글자 차이는 요소 글(줄을 `\n` 으로 이음)끼리 재서 문단 좌표로 돌린다.
/// 그림 정체는 모른다(화면 글 `_` 이 같다) — 도면 교체까지 보려면 `amendment_marks_with`.
pub fn amendment_marks(before: &[EditorParagraph], now: &[EditorParagraph]) -> AmendmentMarks {
    amendment_marks_with(
        before,
        now,
        &FigureSignatures::new(),
        &FigureSignatures::new(),
    )
}

/// 보정 표시 + 도면 교체: 글이 같아도 도면 단위의 그림 서명이 다르면 바뀐 것(그림 문단에 바뀐 줄 표시).
/// 문단 id 는 고치면 밀리니 단위 안 차례로 견준다.
pub fn amendment_marks_with(
    before: &[EditorParagraph],
    now: &[EditorParagraph],
    before_figures: &FigureSignatures,
    now_figures: &FigureSignatures,
) -> AmendmentMarks {
    let compare_figures = !before_figures.is_empty() && !now_figures.is_empty();
    let signature = |unit: &Unit, figures: &FigureSignatures| -> Vec<Vec<u64>> {
        unit.parts
            .iter()
            .map(|part| figures.get(&part.id).cloned().unwrap_or_default())
            .collect()
    };
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
        let figure_changed = compare_figures
            && unit.key.starts_with("figure:")
            && signature(old, before_figures) != signature(unit, now_figures);
        if old_text == new_text && same_spans && !figure_changed {
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

/// 보정 근거 후보 하나: 기준(보정 전) 명세서의 식별번호 문단.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BasisHit {
    pub number: String,
    /// 구절 낱말이 문단에 든 비율(구절이 통째로 있으면 1).
    pub score: f32,
    /// 구절이 통째로(공백 무시) 들어 있다.
    pub exact: bool,
    /// 낱말을 가장 많이 담은 문장(길면 줄임).
    pub quote: String,
    /// 그 문단의 「도 N」.
    pub figures: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PhraseBasis {
    pub text: String,
    pub hits: Vec<BasisHit>,
}

/// 바뀐(또는 새) 청구항 하나의 새로 넣은 말과 그 근거 후보.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClaimBasis {
    pub claim: u32,
    pub added: bool,
    pub phrases: Vec<PhraseBasis>,
}

/// 보정 근거 초안: 바뀐 청구항마다 새로 넣은 말(낱말 경계까지 넓힌 구절) → 보정 전 명세서 식별번호 문단 후보(위 3개).
// ponytail: 낱말 포함 비율일 뿐 뜻은 안 본다 — 형태소 분석은 실물 재현율이 모자랄 때.
pub fn amendment_basis(before: &[EditorParagraph], now: &[EditorParagraph]) -> Vec<ClaimBasis> {
    let old_units = units(before);
    // 【발명을 실시하기 위한 구체적인 내용】 아래 식별번호 — 같은 점수면 청구항을 되풀이한 「발명의 내용」보다 실시예 단락이 근거다(실물 보고서).
    let mut detailed = std::collections::BTreeSet::new();
    let mut inside = false;
    for paragraph in before {
        let text = paragraph.text.trim();
        if let Some(name) = text
            .strip_prefix('【')
            .and_then(|rest| rest.strip_suffix('】'))
        {
            if name == "발명을 실시하기 위한 구체적인 내용" {
                inside = true;
            } else if matches!(
                name,
                "부호의 설명"
                    | "청구범위"
                    | "특허청구범위"
                    | "요약서"
                    | "도면"
                    | "발명의 내용"
                    | "기술분야"
                    | "배경기술"
                    | "선행기술문헌"
                    | "도면의 간단한 설명"
                    | "발명의 설명"
                    | "발명의 명칭"
            ) {
                inside = false;
            }
        } else if inside && let Some(number) = paragraph.number.as_deref() {
            detailed.insert(number.to_string());
        }
    }
    let paragraphs: Vec<(&str, &str, bool)> = old_units
        .iter()
        // 상세설명 문단만(네 자리 · 요약서 「0001a」는 근거가 아니다).
        .filter(|unit| {
            unit.key.len() == "p:0000".len() && unit.key[2..].chars().all(|ch| ch.is_ascii_digit())
        })
        .filter_map(|unit| {
            let number = unit.parts.first()?.number.as_deref()?;
            Some((
                number,
                unit.parts.first()?.text.as_str(),
                detailed.contains(number),
            ))
        })
        .collect();
    let mut out = Vec::new();
    for unit in units(now)
        .iter()
        .filter(|unit| unit.key.starts_with("claim:"))
    {
        let claim: u32 = unit.key["claim:".len()..].parse().unwrap_or(0);
        let texts: Vec<&str> = unit.parts.iter().map(|p| p.text.as_str()).collect();
        let new: Vec<char> = texts.join("\n").chars().collect();
        let old = old_units.iter().find(|old| old.key == unit.key);
        let phrases: Vec<String> = match old {
            None => clauses(&new),
            Some(old) => {
                let texts: Vec<&str> = old.parts.iter().map(|p| p.text.as_str()).collect();
                let old: Vec<char> = texts.join("\n").chars().collect();
                if old == new {
                    continue;
                }
                inserted_phrases(&old, &new)
            }
        };
        // 쉼표 · 「;」로 한 번 더 나눈다(넣은 범위 하나가 여러 구절에 걸친다) · 인용만 바뀐 구절 · 같은 구절은 뺀다.
        let mut seen = std::collections::BTreeSet::new();
        let phrases: Vec<String> = phrases
            .iter()
            .flat_map(|phrase| {
                phrase
                    .split([',', ';'])
                    .map(trim_phrase)
                    .collect::<Vec<_>>()
            })
            .filter(|phrase| citations(phrase).is_empty())
            .filter(|phrase| {
                words(phrase)
                    .iter()
                    .map(|w| w.chars().count())
                    .sum::<usize>()
                    >= 2
            })
            .filter(|phrase| seen.insert(phrase.clone()))
            .collect();
        // 같은 점수면 이 청구항에 새로 넣은 말 전체를 더 많이 담은 단락이 먼저(한 낱말 구절의 동점 풀기).
        let context: Vec<String> = phrases.iter().flat_map(|phrase| words(phrase)).collect();
        let phrases: Vec<PhraseBasis> = phrases
            .into_iter()
            .map(|text| PhraseBasis {
                hits: support(&text, &context, &paragraphs),
                text,
            })
            // 근거가 하나도 없는 한 낱말 구절은 이음말이다(「단계로서」 · 「확인하되」).
            .filter(|phrase| !phrase.hits.is_empty() || words(&phrase.text).len() > 1)
            .collect();
        if !phrases.is_empty() {
            out.push(ClaimBasis {
                claim,
                added: old.is_none(),
                phrases,
            });
        }
    }
    out.sort_by_key(|basis| basis.claim);
    out
}

/// 끝 조사(긴 것 먼저) — 낱말에서 떼고, 조사만 넣은 고침은 구절로 치지 않는다.
const PARTICLES: [&str; 25] = [
    "으로부터",
    "로부터",
    "에서의",
    "에서",
    "으로",
    "에게",
    "까지",
    "부터",
    "하는",
    "되는",
    "하고",
    "하며",
    "된",
    "한",
    "의",
    "을",
    "를",
    "이",
    "가",
    "은",
    "는",
    "에",
    "와",
    "과",
    "로",
];

const BREAKS: [char; 4] = [' ', ',', ';', '\n'];

/// 넣은 범위를 낱말 경계까지 넓힌 구절(겹치거나 공백만 사이에 둔 것은 합친다).
fn inserted_phrases(old: &[char], new: &[char]) -> Vec<String> {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut matcher = SequenceMatcher::new(old, new);
    for op in matcher.get_opcodes() {
        if op.tag == "equal" || op.second_end == op.second_start {
            continue;
        }
        // 조사 · 한 글자만 넣은 고침(「를 → 가」 · 오타 한 자)은 새 말이 아니다.
        let put: String = new[op.second_start..op.second_end]
            .iter()
            .filter(|ch| ch.is_alphanumeric())
            .collect();
        if put.chars().count() <= 1 || PARTICLES.contains(&put.as_str()) {
            continue;
        }
        // 낱말 가운데서 시작 · 끝난 고침만 넓힌다(넣은 글이 공백으로 끝나면 뒤 낱말은 원래 있던 말).
        let mut start = op.second_start;
        while start > 0 && !BREAKS.contains(&new[start]) && !BREAKS.contains(&new[start - 1]) {
            start -= 1;
        }
        let mut end = op.second_end;
        while end < new.len() && !BREAKS.contains(&new[end - 1]) && !BREAKS.contains(&new[end]) {
            end += 1;
        }
        match spans.last_mut() {
            Some(last) if new[last.1..start.max(last.1)].iter().all(|ch| *ch == ' ') => {
                last.1 = last.1.max(end)
            }
            _ => spans.push((start, end)),
        }
    }
    spans
        .into_iter()
        .map(|(a, b)| trim_phrase(&new[a..b].iter().collect::<String>()))
        .filter(|phrase| !phrase.is_empty())
        .collect()
}

/// 새 항: 쉼표 · 「;」 · 줄로 나눈 구절(인용 구절 「제1항에 있어서」는 뺀다).
fn clauses(text: &[char]) -> Vec<String> {
    let text: String = text.iter().collect();
    text.split([',', ';', '\n'])
        .map(trim_phrase)
        .filter(|clause| !clause.is_empty() && citations(clause).is_empty())
        .collect()
}

fn trim_phrase(text: &str) -> String {
    text.trim_matches(|ch: char| ch.is_whitespace() || ",.;:".contains(ch))
        .to_string()
}

/// 구절의 내용 낱말: 문장부호를 떼고 끝 조사 하나를 떼어 2자 이상만 · 흔한 청구항 말은 뺀다.
fn words(phrase: &str) -> Vec<String> {
    const COMMON: [&str; 13] = [
        "단계",
        "상기",
        "또는",
        "및",
        "포함",
        "포함하는",
        "것",
        "있는",
        "경우",
        "적어도",
        "각각",
        "더",
        "통해",
    ];
    phrase
        .split_whitespace()
        .filter_map(|word| {
            let word = word.trim_matches(|ch: char| !ch.is_alphanumeric());
            let stripped = PARTICLES
                .iter()
                .find_map(|particle| {
                    word.strip_suffix(particle)
                        .filter(|rest| rest.chars().count() >= 2)
                })
                .unwrap_or(word);
            (stripped.chars().count() >= 2 && !COMMON.contains(&stripped))
                .then(|| stripped.to_string())
        })
        .collect()
}

/// 구절의 근거 후보: 통째로 들면 1 · 아니면 낱말이 든 비율 0.6 이상 · 위 3개. 같은 점수면 구절과 가장 길게 이어서 같은
/// 단락(「표기 그대로」) → 이 청구항의 새 말을 더 많이 담은 단락 → 번호 차례.
fn support(phrase: &str, context: &[String], paragraphs: &[(&str, &str, bool)]) -> Vec<BasisHit> {
    let compact =
        |text: &str| -> String { text.chars().filter(|ch| !ch.is_whitespace()).collect() };
    let whole = compact(phrase);
    let words = words(phrase);
    let around = |text: &str| {
        context
            .iter()
            .filter(|word| text.contains(word.as_str()))
            .count()
    };
    let whole_chars: Vec<char> = whole.chars().collect();
    let mut hits: Vec<(BasisHit, usize, usize, bool)> = paragraphs
        .iter()
        .filter_map(|(number, text, detailed)| {
            let exact = compact(text).contains(&whole);
            let found = words
                .iter()
                .filter(|word| text.contains(word.as_str()))
                .count();
            let score = if exact {
                1.0
            } else if words.is_empty() {
                0.0
            } else {
                found as f32 / words.len() as f32
            };
            (score >= 0.6).then(|| {
                let hit = BasisHit {
                    number: number.to_string(),
                    score,
                    exact,
                    quote: quote(text, &words),
                    figures: figures(text),
                };
                (
                    hit,
                    longest_common(&whole_chars, &compact(text)),
                    around(text),
                    *detailed,
                )
            })
        })
        .collect();
    hits.sort_by(
        |(a, a_run, a_around, a_detail), (b, b_run, b_around, b_detail)| {
            // 통째로(exact)는 점수 1 · 이어진 길이 최대라 따로 세지 않는다.
            b.score
                .total_cmp(&a.score)
                .then(b_detail.cmp(a_detail))
                .then(b_run.cmp(a_run))
                .then(b_around.cmp(a_around))
                .then(a.number.cmp(&b.number))
        },
    );
    // 위 12개 — 낱말을 다 담은 단락이 여럿이면 셋으로는 보고서가 든 근거가 잘린다(실물: 보고서가 든 두 단락이 8 · 10번째).
    // 화면은 위 3개를 보이고 나머지는 「더 보기」로.
    hits.into_iter().take(12).map(|(hit, ..)| hit).collect()
}

/// 가장 길게 이어서 같은 글자 수(공백 뺀 글끼리).
fn longest_common(phrase: &[char], text: &str) -> usize {
    let text: Vec<char> = text.chars().collect();
    let mut best = 0;
    let mut row = vec![0usize; text.len() + 1];
    for a in phrase {
        let mut previous = 0;
        for (j, b) in text.iter().enumerate() {
            let keep = row[j + 1];
            row[j + 1] = if a == b { previous + 1 } else { 0 };
            best = best.max(row[j + 1]);
            previous = keep;
        }
    }
    best
}

/// 낱말을 가장 많이 담은 문장(「다.」로 끊음) · 160자 넘으면 첫 낱말 둘레로 줄인다.
fn quote(text: &str, words: &[String]) -> String {
    let sentences: Vec<&str> = text
        .split_inclusive("다.")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let best = sentences
        .iter()
        .max_by_key(|sentence| {
            words
                .iter()
                .filter(|word| sentence.contains(word.as_str()))
                .count()
        })
        .copied()
        .unwrap_or(text);
    let chars: Vec<char> = best.chars().collect();
    if chars.len() <= 160 {
        return best.to_string();
    }
    let first = words
        .iter()
        .filter_map(|word| best.find(word.as_str()))
        .min()
        .map_or(0, |byte| best[..byte].chars().count());
    let start = first.saturating_sub(60).min(chars.len() - 160);
    let mut out: String = chars[start..start + 160].iter().collect();
    if start > 0 {
        out.insert(0, '…');
    }
    if start + 160 < chars.len() {
        out.push('…');
    }
    out
}

/// 문단의 「도 N」(「도 3a」 · 「도3」) — 「온도 3」처럼 앞 글자가 한글이면 도면이 아니다.
fn figures(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<String> = Vec::new();
    for at in 0..chars.len() {
        if chars[at] != '도'
            || at
                .checked_sub(1)
                .is_some_and(|prev| ('가'..='힣').contains(&chars[prev]))
        {
            continue;
        }
        let digits_at = at + 1 + spaces(&chars, at + 1);
        let number: String = chars[digits_at..]
            .iter()
            .take_while(|ch| ch.is_ascii_alphanumeric())
            .collect();
        if number.starts_with(|ch: char| ch.is_ascii_digit()) && !out.contains(&number) {
            out.push(number);
        }
    }
    out
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

/// 청구항마다 인용하는 앞 항(작은 번호부터 · 겹침 없이). 인용 읽기는 인용 검사와 같다(「위 항들」 = 앞 항 전부 ·
/// 「제N항 내지 제M항」 · 「청구항 1 내지 3」 묶음). 뒤 항 · 없는 항 인용은 뺀다(그건 인용 검사 몫). 빈 목록 = 독립항.
pub fn claim_parents(claims: &[(u32, String)]) -> std::collections::BTreeMap<u32, Vec<u32>> {
    let numbers: std::collections::BTreeSet<u32> =
        claims.iter().map(|(number, _)| *number).collect();
    claims
        .iter()
        .map(|(number, text)| {
            let read = citations(text)
                .into_iter()
                .flat_map(|citation| match citation {
                    Citation::AllPrevious => (1..*number).collect::<Vec<_>>(),
                    Citation::Range(from, to, _) => (from..=to.unwrap_or(from)).collect(),
                });
            let grouped = groups(text).into_iter().flat_map(|group| group.numbers);
            let parents: std::collections::BTreeSet<u32> = read
                .chain(grouped)
                .filter(|cited| cited < number && numbers.contains(cited))
                .collect();
            (*number, parents.into_iter().collect())
        })
        .collect()
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

    fn plain(texts: &[&str]) -> Vec<EditorParagraph> {
        texts
            .iter()
            .enumerate()
            .map(|(at, text)| paragraph(&format!("paragraph:{at}"), text, None))
            .collect()
    }

    #[test]
    fn amendment_table_lists_changed_added_and_removed_in_document_order() {
        let before = vec![
            paragraph("c", "가나다", Some("0001")),
            paragraph("d", "그대로", Some("0002")),
            paragraph("e", "지울 글", Some("0003")),
            paragraph("a", "【청구항 1】", None),
            paragraph("b", "장치.", None),
        ];
        let now = vec![
            paragraph("c", "가X다", Some("0001")),
            paragraph("d", "그대로", Some("0002")),
            paragraph("a", "【청구항 1】", None),
            paragraph("b", "장치.", None),
            paragraph("f", "【청구항 2】", None),
            paragraph("g", "새 항.", None),
        ];
        let rows = amendment_table(
            &before,
            &now,
            &FigureSignatures::new(),
            &FigureSignatures::new(),
        );
        let labels: Vec<&str> = rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(labels, ["【0001】", "【0003】", "청구항 2"]);
        assert_eq!(
            (rows[0].before.as_str(), rows[0].after.as_str()),
            ("가나다", "가X다")
        );
        assert_eq!(
            (rows[0].deleted.clone(), rows[0].inserted.clone()),
            (vec![(1, 2)], vec![(1, 2)])
        );
        assert_eq!(
            (
                rows[1].before.as_str(),
                rows[1].after.as_str(),
                rows[1].deleted.clone()
            ),
            ("지울 글", "", vec![(0, 4)])
        );
        assert_eq!(
            (
                rows[2].before.as_str(),
                rows[2].after.as_str(),
                rows[2].inserted.clone()
            ),
            ("", "새 항.", vec![(0, 4)])
        );
    }

    #[test]
    fn unnumbered_files_pair_description_paragraphs_by_text_order() {
        let mut before = plain(&[
            "【발명의 설명】",
            "【발명의 명칭】",
            "장치",
            "【기술분야】",
            "가나다",
            "지울 문단",
            "라마바",
            "【표 1】",
            "【청구범위】",
            "【청구항 1】",
            "장치.",
        ]);
        let mut now = plain(&[
            "【발명의 설명】",
            "【발명의 명칭】",
            "장치",
            "【기술분야】",
            "가나다",
            "라마바사",
            "넣은 문단",
            "【표 1】",
            "【청구범위】",
            "【청구항 1】",
            "장치.",
        ]);
        number_unnumbered(&mut before, &mut now);
        let numbers = |paragraphs: &[EditorParagraph]| -> Vec<Option<String>> {
            paragraphs.iter().map(|p| p.number.clone()).collect()
        };
        let n = |text: &str| Some(text.to_string());
        assert_eq!(
            numbers(&now),
            [
                None,
                None,
                None,
                None,
                n("0001"),
                n("0002"),
                n("0003"),
                n("0004"),
                None,
                None,
                None
            ]
        );
        // 「지울 문단」 · 「라마바」 ↔ 「라마바사」 · 「넣은 문단」: 바뀐 자리는 차례대로 짝짓는다.
        assert_eq!(
            numbers(&before),
            [
                None,
                None,
                None,
                None,
                n("0001"),
                n("0002"),
                n("0003"),
                n("0004"),
                None,
                None,
                None
            ]
        );
        let marks = amendment_marks(&before, &now);
        assert_eq!(marks.changed, ["【0002】", "【0003】"]);

        let mut gone = plain(&["【발명의 설명】", "가", "나", "다"]);
        let mut kept = plain(&["【발명의 설명】", "가", "다"]);
        number_unnumbered(&mut gone, &mut kept);
        assert_eq!(numbers(&gone), [None, n("0001"), n("이전 0002"), n("0002")]);
        assert_eq!(amendment_marks(&gone, &kept).removed, ["【이전 0002】"]);

        // 한쪽이라도 번호가 있으면(최종본 · 보정 명세서) 건드리지 않는다.
        let mut numbered = plain(&["【발명의 설명】", "가"]);
        numbered[1].number = n("0001");
        let mut other = plain(&["【발명의 설명】", "가"]);
        number_unnumbered(&mut numbered, &mut other);
        assert_eq!(numbers(&other), [None, None]);
    }

    #[test]
    fn claim_parents_reads_ranges_lists_and_all_previous() {
        let claims: Vec<(u32, String)> = [
            (1, "장치."),
            (2, "제1항에 있어서, 부재."),
            (3, "청구항 1 내지 2 중 어느 한 항에 있어서."),
            (4, "제2항 또는 제3항에 있어서, 상기 청구항 1의 부재."),
            (5, "위 항들 중 어느 한 항에 있어서."),
            (6, "제6항 또는 제9항에 있어서."),
            (8, "제7항에 있어서."),
        ]
        .into_iter()
        .map(|(number, text)| (number, text.to_string()))
        .collect();
        let parents = claim_parents(&claims);
        assert_eq!(parents[&1], Vec::<u32>::new());
        assert_eq!(parents[&2], [1]);
        assert_eq!(parents[&3], [1, 2]);
        assert_eq!(parents[&4], [2, 3], "「상기 청구항 1」은 인용이 아니다");
        assert_eq!(parents[&5], [1, 2, 3, 4]);
        assert_eq!(parents[&6], Vec::<u32>::new(), "자기 · 뒤 항 인용은 뺀다");
        assert_eq!(parents[&8], Vec::<u32>::new(), "없는 항 인용은 뺀다");
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
    fn finds_basis_paragraphs_for_new_claim_words() {
        let before = vec![
            paragraph("a", "【발명의 설명】", None),
            // 낱말은 다 있지만 통째로는 아니다 — 이어진 길이로 0002 뒤에 선다.
            paragraph(
                "p1",
                "본 발명은 바퀴와 고무가 결합된 장치에 관한 것이다.",
                Some("0001"),
            ),
            paragraph(
                "p2",
                "몸체에는 고무 바퀴가 결합된다. 도 3에 바퀴를 보인다.",
                Some("0002"),
            ),
            paragraph(
                "p3",
                "바퀴는 고무로 만든다. 손잡이는 금속이다.",
                Some("0003"),
            ),
            paragraph("p4", "덮개는 손잡이를 감싼다.", Some("0004")),
            paragraph("s", "【청구항 1】", None),
            paragraph("c1", "몸체를 포함하는 장치.", None),
            paragraph("s2", "【청구항 2】", None),
            // 청구항 안 문단은 근거가 아니다(번호가 붙어 있어도).
            paragraph(
                "c2",
                "제1항에 있어서, 손잡이를 포함하는 장치. 손잡이와 덮개를",
                Some("0009"),
            ),
            paragraph("s3", "【청구항 3】", None),
            paragraph("c3", "제1항에 있어서, 바퀴를 갖는 장치.", None),
            paragraph("y", "【요약서】", None),
            // 요약서 문단도 근거가 아니다.
            paragraph("y1", "손잡이와 덮개를 갖는 장치.", Some("0001a")),
        ];
        let mut now = before.clone();
        now[6].text = "고무 바퀴가 결합된 몸체를 포함하는 장치.".into();
        now[8].text = "제1항에 있어서, 손잡이와 덮개를 포함하는 장치. 손잡이와 덮개를".into();
        now[10].text = "제1항에 있어서, 바퀴가 갖는 장치.".into(); // 조사만 바꿈
        now.insert(11, paragraph("s4", "【청구항 4】", None));
        now.insert(
            12,
            paragraph("c4", "제1항에 있어서, 바퀴는 고무로 만든다.", None),
        );
        let basis = amendment_basis(&before, &now);
        let claims: Vec<(u32, bool)> = basis.iter().map(|b| (b.claim, b.added)).collect();
        assert_eq!(
            claims,
            [(1, false), (2, false), (4, true)],
            "청구항 3 = 조사만"
        );
        let numbers = |phrase: &PhraseBasis| -> Vec<String> {
            phrase.hits.iter().map(|hit| hit.number.clone()).collect()
        };
        // 청구항 1: 통째로 든 단락이 1위(도 3 · 그 문장 인용) · 낱말이 흩어진 단락이 다음.
        let first = &basis[0].phrases[0];
        assert_eq!(first.text, "고무 바퀴가 결합된");
        assert_eq!(numbers(first), ["0002", "0001", "0003"]);
        assert!(first.hits[0].exact && !first.hits[1].exact);
        assert_eq!(first.hits[0].figures, ["3"]);
        assert_eq!(first.hits[0].quote, "몸체에는 고무 바퀴가 결합된다.");
        // 청구항 2: 청구항 안(0009) · 요약서(0001a)는 빼고 0004 만(0003 은 낱말 절반).
        assert_eq!(basis[1].phrases[0].text, "손잡이와 덮개를");
        assert_eq!(numbers(&basis[1].phrases[0]), ["0004"]);
        // 새 항: 인용 구절은 빼고 나머지 구절.
        let added: Vec<&str> = basis[2].phrases.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(added, ["바퀴는 고무로 만든다"]);
        assert_eq!(numbers(&basis[2].phrases[0])[0], "0003");
        // 「온도 30」은 도면이 아니다.
        assert_eq!(figures("온도 30에서 도 3a와 도5를"), ["3a", "5"]);
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

    #[test]
    fn basis_reads_word_stems_skips_step_and_prefers_detailed_paragraphs() {
        // 「누락된」 · 「보간하는」 은 줄기로 · 「단계」는 세지 않는다.
        assert_eq!(
            words("누락된 블록데이터를 보간하는 단계"),
            ["누락", "블록데이터", "보간"]
        );
        let before = vec![
            paragraph("a", "【발명의 설명】", None),
            paragraph("b", "【발명의 내용】", None),
            // 청구항을 되풀이한 요약 단락 — 낱말은 다 있다.
            paragraph("p1", "누락 블록데이터를 보간한다.", Some("0001")),
            paragraph("c", "【발명을 실시하기 위한 구체적인 내용】", None),
            paragraph("p2", "실시예에서 누락 블록데이터를 보간한다.", Some("0002")),
            paragraph("s", "【청구항 1】", None),
            paragraph("c1", "가를 포함하는 장치.", None),
        ];
        let mut now = before.clone();
        now[6].text = "가를 포함하고, 누락된 블록데이터를 보간하는 단계를 포함하는 장치.".into();
        let basis = amendment_basis(&before, &now);
        let hits: Vec<&str> = basis[0]
            .phrases
            .iter()
            .find(|phrase| phrase.text.contains("누락"))
            .unwrap()
            .hits
            .iter()
            .map(|hit| hit.number.as_str())
            .collect();
        // 둘 다 낱말을 다 담았다(점수 1) — 실시예 단락이 먼저.
        assert_eq!(hits, ["0002", "0001"]);
    }
}
