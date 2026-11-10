//! XLIFF 1.2 계열(.xlf · .xliff · .txlf · .mxliff) 세그먼트 읽기 + 편집 문서 문장에 붙잡기.
//! 세그먼트 짝은 CAT 이 낸 근거라 숫자 검문을 거치지 않는다. 대신 틀린 짝은 짝이 없는 것보다 나쁘다 —
//! 글자 그대로 같은 것만 맞추고, 안 맞은 세그먼트는 앞뒤가 확실할 때만 끼운다.

use std::collections::HashMap;
use std::path::Path;

use quick_xml::{Reader, events::Event};

use crate::align::{target_claims, target_sentence_ranges};
use crate::error::io_error;
use crate::{
    Alignment, CoreError, EditorParagraph, ErrorCode, Grade, Pair, Pin, SourcePara, paragraph_keys,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub source: String,
    pub target: String,
    /// 같은 값 = 같은 문단(XTM · Wordfast 의 `group`, Phrase 의 `para-id`). 없으면 모른다.
    pub group: Option<String>,
}

struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    kids: Vec<Kid>,
}
enum Kid {
    Text(String),
    El(Node),
}

impl Node {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    fn child(&self, name: &str) -> Option<&Node> {
        self.kids.iter().find_map(|k| match k {
            Kid::El(n) if n.name == name => Some(n),
            _ => None,
        })
    }
}

fn error(code: ErrorCode, message: &str, source: &Path) -> CoreError {
    let mut e = CoreError::new(code, "xliff_read", false);
    e.message = message.to_string();
    e.source = source.file_name().map(|n| n.to_string_lossy().into_owned());
    e
}

fn parse(text: &str) -> Option<Node> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(false);
    let mut stack: Vec<Node> = Vec::new();
    let mut root = None;
    let local = |raw: &[u8]| {
        let s = String::from_utf8_lossy(raw);
        s.rsplit_once(':')
            .map_or(s.to_string(), |(_, l)| l.to_string())
    };
    let open = |reader: &Reader<&[u8]>, start: &quick_xml::events::BytesStart<'_>| {
        let mut attrs = Vec::new();
        for a in start.attributes() {
            let a = a.ok()?;
            let value = a
                .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder())
                .ok()?;
            attrs.push((local(a.key.as_ref()), value.into_owned()));
        }
        Some(Node {
            name: local(start.name().as_ref()),
            attrs,
            kids: Vec::new(),
        })
    };
    let mut close = |stack: &mut Vec<Node>, node: Node| -> bool {
        match stack.last_mut() {
            Some(parent) => parent.kids.push(Kid::El(node)),
            None if root.is_none() => root = Some(node),
            None => return false,
        }
        true
    };
    loop {
        match reader.read_event().ok()? {
            Event::Start(s) => stack.push(open(&reader, &s)?),
            Event::Empty(s) => {
                let node = open(&reader, &s)?;
                if !close(&mut stack, node) {
                    return None;
                }
            }
            Event::End(_) => {
                let node = stack.pop()?;
                if !close(&mut stack, node) {
                    return None;
                }
            }
            Event::Text(t) => {
                if let Some(p) = stack.last_mut() {
                    p.kids.push(Kid::Text(t.decode().ok()?.into_owned()));
                }
            }
            Event::CData(t) => {
                if let Some(p) = stack.last_mut() {
                    p.kids.push(Kid::Text(t.decode().ok()?.into_owned()));
                }
            }
            Event::GeneralRef(r) => {
                if let Some(p) = stack.last_mut() {
                    let c = crate::docx::resolve_xml_reference(&r.decode().ok()?)?;
                    p.kids.push(Kid::Text(c.to_string()));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if stack.is_empty() { root } else { None }
}

/// 공백 연달은 것 하나 · 앞뒤 공백 없음(전각 · 줄바꿈 포함).
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 인라인 태그를 뺀 글: `g` · `mrk` 는 안의 글을 살리고, 태그 자체(`x` `bx` `ex` `ph` `bpt` `ept` `it` `sub`)는 안의 코드까지 버린다.
fn plain(node: &Node, out: &mut String) {
    for kid in &node.kids {
        match kid {
            Kid::Text(t) => out.push_str(t),
            Kid::El(n) => {
                if !matches!(
                    n.name.as_str(),
                    "x" | "bx" | "ex" | "ph" | "bpt" | "ept" | "it" | "sub" | "ws"
                ) {
                    plain(n, out);
                }
            }
        }
    }
}

fn plain_text(node: &Node) -> String {
    let mut s = String::new();
    plain(node, &mut s);
    collapse(&s)
}

/// `<mrk mtype="seg">` 들(차례대로 · 안쪽은 더 파지 않는다).
fn seg_marks<'a>(node: &'a Node, out: &mut Vec<&'a Node>) {
    for kid in &node.kids {
        if let Kid::El(n) = kid {
            if n.name == "mrk" && n.attr("mtype") == Some("seg") {
                out.push(n);
            } else {
                seg_marks(n, out);
            }
        }
    }
}

fn unit_segments(tu: &Node, group: Option<String>, out: &mut Vec<Segment>) {
    // 표준대로 분할본(`seg-source`)이 있으면 그것을 쓴다.
    let Some(source) = tu.child("seg-source").or_else(|| tu.child("source")) else {
        return;
    };
    let target = tu.child("target");
    let mut marks = Vec::new();
    seg_marks(source, &mut marks);
    if marks.is_empty() {
        out.push(Segment {
            source: plain_text(source),
            target: target.map(plain_text).unwrap_or_default(),
            group,
        });
        return;
    }
    let mut target_marks = Vec::new();
    if let Some(t) = target {
        seg_marks(t, &mut target_marks);
    }
    let lone = marks.len() == 1 && target_marks.is_empty();
    for mark in marks {
        let mid = mark.attr("mid");
        let target_text = if lone {
            target.map(plain_text).unwrap_or_default()
        } else {
            target_marks
                .iter()
                .find(|t| t.attr("mid") == mid)
                .map(|t| plain_text(t))
                .unwrap_or_default()
        };
        out.push(Segment {
            source: plain_text(mark),
            target: target_text,
            group: group.clone(),
        });
    }
}

/// 번역 단위를 모은다. `alt-trans`(대체 번역 후보 — 안에 또 `source` · `target` 이 있다)는 건너뛴다.
/// 문단 묶음: Phrase 의 `para-id` → 없으면 바깥쪽 `group`(Wordfast 의 쪽 묶음 `x-page` 는 빼고 안쪽 문단 묶음).
fn walk(node: &Node, groups: &mut Vec<String>, counter: &mut u32, out: &mut Vec<Segment>) {
    for kid in &node.kids {
        let Kid::El(n) = kid else { continue };
        match n.name.as_str() {
            "alt-trans" | "header" => {}
            "group" => {
                *counter += 1;
                let pushed = n.attr("restype") != Some("x-page");
                if pushed {
                    let id = n.attr("id").map_or(format!("#{counter}"), str::to_string);
                    groups.push(format!("group:{id}"));
                }
                walk(n, groups, counter, out);
                if pushed {
                    groups.pop();
                }
            }
            "trans-unit" => {
                let group = n
                    .attr("para-id")
                    .map(|id| format!("para:{id}"))
                    .or_else(|| groups.last().cloned());
                unit_segments(n, group, out);
            }
            _ => walk(n, groups, counter, out),
        }
    }
}

/// XLIFF 1.2 계열(.xlf · .xliff · .txlf · .mxliff): `trans-unit` 의 `source` / `target` 글(인라인 태그는 글만 ·
/// `mrk mtype="seg"` 가 있으면 그것이 세그먼트). 글은 공백을 하나로 모으고 앞뒤를 다듬어 싣는다.
pub fn read_xliff(path: &Path) -> Result<Vec<Segment>, CoreError> {
    let bytes =
        std::fs::read(path).map_err(|e| io_error(&e, "xliff_read", &path.to_string_lossy()))?;
    let text = match encoding_rs::Encoding::for_bom(&bytes) {
        Some((enc, len)) => enc
            .decode_without_bom_handling(&bytes[len..])
            .0
            .into_owned(),
        None => String::from_utf8(bytes).map_err(|_| {
            error(
                ErrorCode::UnsupportedFormat,
                "XLIFF 파일의 글자 인코딩(UTF-8)을 읽지 못했습니다.",
                path,
            )
        })?,
    };
    let root = parse(&text).ok_or_else(|| {
        error(
            ErrorCode::StructureInvalid,
            "XLIFF 파일을 읽지 못했습니다. 올바른 XML 파일인지 확인해 주세요.",
            path,
        )
    })?;
    if root.name != "xliff" {
        return Err(error(
            ErrorCode::UnsupportedFormat,
            "XLIFF 파일이 아닙니다. .xlf · .xliff · .txlf · .mxliff 파일을 골라 주세요.",
            path,
        ));
    }
    let mut out = Vec::new();
    walk(&root, &mut Vec::new(), &mut 0, &mut out);
    if out.is_empty() {
        return Err(error(
            ErrorCode::StructureInvalid,
            "XLIFF 파일에서 번역 문장을 찾지 못했습니다.",
            path,
        ));
    }
    Ok(out)
}

/// 정규화: 공백(전각 · 줄바꿈 포함) 연달은 것 하나 · 앞뒤 공백 없음.
fn norm(text: &str) -> Vec<char> {
    collapse(text).chars().collect()
}

/// 문서 문단 하나의 정규화 글 + 문장마다 정규화 글 안 `[시작, 끝)`.
struct DocPara {
    text: Vec<char>,
    sentences: Vec<(usize, usize)>,
}

fn doc_para(text: &str, ranges: &[(u32, u32)]) -> DocPara {
    let chars: Vec<char> = text.chars().collect();
    let (mut out, mut at, mut gap) = (Vec::new(), vec![0; chars.len() + 1], false);
    for (i, &c) in chars.iter().enumerate() {
        if c.is_whitespace() {
            gap = true;
            continue;
        }
        if gap && !out.is_empty() {
            out.push(' ');
        }
        gap = false;
        at[i] = out.len();
        out.push(c);
    }
    // 문장은 앞뒤 공백을 뗀 범위라 첫 글자 · 마지막 글자는 공백이 아니다.
    let sentences = ranges
        .iter()
        .map(|&(s, e)| (at[s as usize], at[e as usize - 1] + 1))
        .collect();
    DocPara {
        text: out,
        sentences,
    }
}

/// 문서 `(문단, 문장)` 자리 `from` 부터 처음 나오는, 문장 경계에 딱 맞는 같은 글: `(문단, 첫 문장, 끝 문장)`.
fn find(doc: &[DocPara], needle: &[char], from: (usize, usize)) -> Option<(usize, usize, usize)> {
    for (p, para) in doc.iter().enumerate().skip(from.0) {
        let first = if p == from.0 { from.1 } else { 0 };
        for (i, &(start, _)) in para.sentences.iter().enumerate().skip(first) {
            if para.text[start..].starts_with(needle) {
                let end = start + needle.len();
                if let Some(j) = para.sentences[i..].iter().position(|s| s.1 == end) {
                    return Some((p, i, i + j));
                }
            }
        }
    }
    None
}

#[derive(Clone, Copy, PartialEq)]
enum State {
    /// 맞추지 않는다(원문이 비었거나 `target` 이 비었거나 원문과 같다).
    Skip,
    Open,
    /// 문서 문단 · 문서 문단 안 (끝난 뒤) 문장 차례.
    Hit(usize, u32),
}

#[derive(PartialEq)]
enum Key {
    Group(String),
    Doc(usize),
    Alone(usize),
}

/// 세그먼트를 문서 문장에 붙잡는다: 글자 그대로 같은 것만, 차례를 지켜 앞에서부터(되풀이 문장은 차례로).
/// 안 맞은 세그먼트는 앞뒤 맞은 세그먼트가 같은 문서 문단에 있을 때만 끼운다 — 빈 칸 수가 같으면 문장 짝, 다르면
/// (모두 같은 원문 문단이면) 그 문단 짝 하나. 앞뒤가 다른 문단이면 짝 없음. `pins` 는 쓸 수 없으면 `dropped_pins` 로 돌려준다.
pub fn align_xliff(target: &[EditorParagraph], segments: &[Segment], pins: &[Pin]) -> Alignment {
    let tclaim = target_claims(target, &paragraph_keys(target));
    let tsent0 = target_sentence_ranges(target, &tclaim);
    let doc: Vec<DocPara> = (0..target.len())
        .map(|i| doc_para(&target[i].text, &tsent0[i]))
        .collect();
    let n = segments.len();
    let src: Vec<String> = segments.iter().map(|s| collapse(&s.source)).collect();
    let tgt: Vec<Vec<char>> = segments.iter().map(|s| norm(&s.target)).collect();

    // 1. 맞추기. ponytail: 한 걸음 앞보기만 — 더 먼 꼬임은 못 본다(그땐 안 맞은 쪽으로 남아 안전하다).
    let mut state: Vec<State> = (0..n)
        .map(|k| {
            let skip = src[k].is_empty()
                || tgt[k].is_empty()
                || tgt[k] == src[k].chars().collect::<Vec<_>>();
            if skip { State::Skip } else { State::Open }
        })
        .collect();
    let mut spans: Vec<Option<(usize, usize, usize)>> = vec![None; n];
    let order: Vec<usize> = (0..n).filter(|&k| state[k] == State::Open).collect();
    let mut cur = (0, 0);
    for (at, &k) in order.iter().enumerate() {
        let Some(hit) = find(&doc, &tgt[k], cur) else {
            continue;
        };
        let later = order.get(at + 1).and_then(|&k2| find(&doc, &tgt[k2], cur));
        if later.is_some_and(|l| (l.0, l.1) < (hit.0, hit.1)) {
            continue; // 다음 세그먼트가 더 앞에서 나온다 — 이 맞춤은 먼 곳 우연의 일치다.
        }
        spans[k] = Some(hit);
        cur = (hit.0, hit.2 + 1);
    }

    // 2. 문서 문장 범위: 맞은 세그먼트는 세그먼트 경계, 나머지는 문장 가르기 그대로.
    let mut starts: HashMap<(usize, usize), usize> = HashMap::new();
    for s in spans.iter().flatten() {
        starts.insert((s.0, s.1), s.2);
    }
    let mut final_ranges: Vec<Vec<(u32, u32)>> = vec![Vec::new(); target.len()];
    let mut final_at: HashMap<(usize, usize), u32> = HashMap::new();
    for p in 0..target.len() {
        let mut i = 0;
        while i < tsent0[p].len() {
            let j = starts.get(&(p, i)).map_or(i, |&j| j);
            final_at.insert((p, i), final_ranges[p].len() as u32);
            final_ranges[p].push((tsent0[p][i].0, tsent0[p][j].1));
            i = j + 1;
        }
    }
    for k in 0..n {
        if let Some(s) = spans[k] {
            state[k] = State::Hit(s.0, final_at[&(s.0, s.1)]);
        }
    }

    // 3. 원문 창 문단.
    let prev_hit: Vec<Option<usize>> = {
        let mut last = None;
        (0..n)
            .map(|k| {
                let r = last;
                if matches!(state[k], State::Hit(..)) {
                    last = Some(k);
                }
                r
            })
            .collect()
    };
    let mut next_hit: Vec<Option<usize>> = vec![None; n];
    let mut last = None;
    for k in (0..n).rev() {
        next_hit[k] = last;
        if matches!(state[k], State::Hit(..)) {
            last = Some(k);
        }
    }
    let hit_para = |k: usize| match state[k] {
        State::Hit(p, _) => Some(p),
        _ => None,
    };
    let key = |k: usize| -> Key {
        if let Some(g) = &segments[k].group {
            return Key::Group(g.clone());
        }
        if let State::Hit(p, _) = state[k] {
            return Key::Doc(p);
        }
        match (
            prev_hit[k].and_then(hit_para),
            next_hit[k].and_then(hit_para),
        ) {
            (Some(a), Some(b)) if a == b => Key::Doc(a),
            _ => Key::Alone(k),
        }
    };
    let mut source: Vec<SourcePara> = Vec::new();
    let mut source_keys: Vec<Key> = Vec::new();
    let mut at_source: Vec<Option<(u32, u32)>> = vec![None; n];
    for k in (0..n).filter(|&k| !src[k].is_empty()) {
        let key = key(k);
        if source_keys.last() != Some(&key) {
            source_keys.push(key);
            source.push(SourcePara {
                key: None,
                text: String::new(),
                sentences: Vec::new(),
            });
        }
        let index = source.len() as u32 - 1;
        let para = source.last_mut().expect("방금 넣었다");
        if !para.text.is_empty() {
            para.text.push(' ');
        }
        let start = para.text.chars().count() as u32;
        para.text.push_str(&src[k]);
        para.sentences
            .push((start, start + src[k].chars().count() as u32));
        at_source[k] = Some((index, para.sentences.len() as u32 - 1));
    }

    // 4. 짝: 맞은 것 → 끼운 것 → 문단 짝으로 내린 것.
    let pair = |t: usize, ts: u32, k: usize| {
        let (sp, ss) = at_source[k].expect("원문이 있는 세그먼트만 맞춘다");
        Pair {
            target_id: target[t].id.clone(),
            target_sentence: Some(ts),
            source_paragraph: sp,
            source_sentence: Some(ss),
            grade: Grade::Sentence,
        }
    };
    let mut pairs: Vec<(usize, u32, Pair)> = Vec::new();
    let mut downgrade: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut run: Vec<usize> = Vec::new();
    let mut last_hit: Option<usize> = None;
    for k in 0..n {
        match state[k] {
            State::Skip => {}
            State::Open => run.push(k),
            State::Hit(t, ts) => {
                pairs.push((t, ts, pair(t, ts, k)));
                if let (Some(p), false) = (last_hit, run.is_empty())
                    && let State::Hit(pt, pts) = state[p]
                    && pt == t
                {
                    let gap = (ts - pts - 1) as usize;
                    if gap == run.len() {
                        for (j, &r) in run.iter().enumerate() {
                            let at = pts + 1 + j as u32;
                            pairs.push((t, at, pair(t, at, r)));
                        }
                    } else {
                        downgrade.entry(t).or_default().extend(run.iter().copied());
                    }
                }
                run.clear();
                last_hit = Some(k);
            }
        }
    }
    for (t, runs) in downgrade {
        let mut owners: Vec<u32> = runs
            .iter()
            .filter_map(|&k| at_source[k])
            .map(|a| a.0)
            .collect();
        owners.extend(
            (0..n)
                .filter(|&k| hit_para(k) == Some(t))
                .filter_map(|k| at_source[k])
                .map(|a| a.0),
        );
        owners.sort_unstable();
        owners.dedup();
        if let [only] = owners[..] {
            pairs.retain(|p| p.0 != t);
            pairs.push((
                t,
                0,
                Pair {
                    target_id: target[t].id.clone(),
                    target_sentence: None,
                    source_paragraph: only,
                    source_sentence: None,
                    grade: Grade::Paragraph,
                },
            ));
        }
    }

    // 5. 손 고정: 아직 짝 없는 문장끼리 · 이미 있는 짝들과 차례가 맞을 때만.
    let mut dropped: Vec<Pin> = Vec::new();
    for pin in pins {
        let t = target.iter().position(|p| p.id == pin.target_id);
        let usable = t.is_some_and(|t| {
            (pin.target_sentence as usize) < final_ranges[t].len()
                && source
                    .get(pin.source_paragraph as usize)
                    .is_some_and(|s| (pin.source_sentence as usize) < s.sentences.len())
                && pairs.iter().all(|(pt, _, p)| {
                    let (tk, sk) = (
                        (t, pin.target_sentence),
                        (pin.source_paragraph, pin.source_sentence),
                    );
                    match (p.target_sentence, p.source_sentence) {
                        (Some(ts), Some(ss)) => {
                            tk != (*pt, ts)
                                && sk != (p.source_paragraph, ss)
                                && tk.cmp(&(*pt, ts)) == sk.cmp(&(p.source_paragraph, ss))
                        }
                        _ => {
                            t != *pt
                                && pin.source_paragraph != p.source_paragraph
                                && t.cmp(pt) == pin.source_paragraph.cmp(&p.source_paragraph)
                        }
                    }
                })
        });
        match t.filter(|_| usable) {
            Some(t) => pairs.push((
                t,
                pin.target_sentence,
                Pair {
                    target_id: pin.target_id.clone(),
                    target_sentence: Some(pin.target_sentence),
                    source_paragraph: pin.source_paragraph,
                    source_sentence: Some(pin.source_sentence),
                    grade: Grade::Sentence,
                },
            )),
            None => dropped.push(pin.clone()),
        }
    }
    pairs.sort_by_key(|p| (p.0, p.1));

    Alignment {
        source,
        target_sentences: (0..target.len())
            .filter(|&t| !final_ranges[t].is_empty())
            .map(|t| (target[t].id.clone(), final_ranges[t].clone()))
            .collect(),
        pairs: pairs.into_iter().map(|p| p.2).collect(),
        dropped_pins: dropped,
    }
}
