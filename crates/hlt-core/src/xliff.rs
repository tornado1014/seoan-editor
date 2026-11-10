//! XLIFF 1.2 계열(.xlf · .xliff · .txlf · .mxliff) 세그먼트 읽기 + 편집 문서 문장에 붙잡기.
//! 세그먼트 짝은 CAT 이 낸 근거라 숫자 검문을 거치지 않는다. 대신 틀린 짝은 짝이 없는 것보다 나쁘다 —
//! 글자 그대로 같은 것만 맞추고, 안 맞은 세그먼트는 앞뒤가 확실할 때만 끼운다.

use std::path::Path;

use quick_xml::{Reader, events::Event};

use crate::align::{Given, infer, target_claims, target_sentence_ranges};
use crate::error::io_error;
use crate::{
    Alignment, CoreError, EditorParagraph, ErrorCode, Grade, Lang, Pair, Pin, SourcePara,
    paragraph_keys, sentences,
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
    let before = out.len();
    unit_segments_raw(tu, group, out);
    // 잠긴 · 번역 대상이 아닌 단위는 원문 창에만 싣는다 — target 을 비워 짝에서 뺀다.
    if tu.attr("locked") == Some("true") || tu.attr("translate") == Some("no") {
        out[before..].iter_mut().for_each(|s| s.target.clear());
    }
}

fn unit_segments_raw(tu: &Node, group: Option<String>, out: &mut Vec<Segment>) {
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

/// Phrase(mxliff) 글 속 태그 자리표: `{1}` · 짝 `{b>` `<b}` `{i>` `<i}` `{u>` `<u}` `{bu>` `{^>` `{1>` `<1}` …(실물 구조 확인).
/// 태그만 떼고 사이 글은 남긴다. 세그먼트 쪽에만 적용한다 — 문서 글은 건드리지 않는다.
fn strip_placeholders(text: &str) -> String {
    static TAG: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\{[0-9]+\}|\{[A-Za-z0-9^]{1,3}>|<[A-Za-z0-9^]{1,3}\}").expect("정규식")
    });
    collapse(&TAG.replace_all(text, ""))
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
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("mxliff"))
    {
        for s in &mut out {
            s.source = strip_placeholders(&s.source);
            s.target = strip_placeholders(&s.target);
        }
    }
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

/// 문서 문단 하나: 원래 글자 · 정규화 글 · 정규화 글자 → 원래 글자 자리.
struct DocPara {
    target: usize,
    chars: Vec<char>,
    norm: Vec<char>,
    map: Vec<u32>,
}

fn doc_para(target: usize, text: &str) -> DocPara {
    let chars: Vec<char> = text.chars().collect();
    let (mut norm, mut map, mut gap) = (Vec::new(), Vec::new(), None);
    for (i, &c) in chars.iter().enumerate() {
        if c.is_whitespace() {
            gap.get_or_insert(i as u32);
            continue;
        }
        if let Some(g) = gap.take()
            && !norm.is_empty()
        {
            norm.push(' ');
            map.push(g);
        }
        norm.push(c);
        map.push(i as u32);
    }
    DocPara {
        target,
        chars,
        norm,
        map,
    }
}

impl DocPara {
    /// 정규화 `[s, e)`(앞뒤 공백 없음) → 원래 글자 `[시작, 끝)`.
    fn orig(&self, s: usize, e: usize) -> (u32, u32) {
        (self.map[s], self.map[e - 1] + 1)
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Cursor {
    d: usize,
    o: usize,
}

/// 공백을 건너뛰고, 문단 끝이면 글이 있는 다음 문단 처음으로.
fn settle(docs: &[DocPara], cur: &mut Cursor) {
    while let Some(p) = docs.get(cur.d) {
        while p.norm.get(cur.o) == Some(&' ') {
            cur.o += 1;
        }
        if cur.o < p.norm.len() {
            return;
        }
        *cur = Cursor { d: cur.d + 1, o: 0 };
    }
}

/// 현재 문단과 그 뒤 2문단 안에서만(글 있는 문단 기준) 처음 나오는 자리.
const LOOKAHEAD_PARAGRAPHS: usize = 3;
/// 이보다 짧은 글은 커서 자리가 아니면 찾지 않는다(짧은 되풀이 문장에 낚이지 않게).
const MIN_SEARCH_CHARS: usize = 10;

fn is_punct(c: char) -> bool {
    ".,;:!?)]}」』】”’\"'。、，；：！？）".contains(c)
}

impl DocPara {
    /// 정규화 `[o, e)` 가 오른쪽 경계에서 끝나는가: 문단 끝 · 다음이 공백 · 다음이 문장부호 · 맞춘 글이 부호로 끝남.
    fn ends_on_boundary(&self, e: usize) -> bool {
        self.norm.get(e).is_none_or(|&c| c == ' ' || is_punct(c)) || is_punct(self.norm[e - 1])
    }

    /// 정규화 `o` 가 왼쪽 경계인가: 문단 처음 · 줄바꿈 뒤 · 끝맺음 부호(`.!?。;:`) 뒤 공백 다음.
    fn starts_on_boundary(&self, o: usize) -> bool {
        let mut j = self.map[o] as usize;
        let mut spaced = false;
        while j > 0 && self.chars[j - 1].is_whitespace() {
            if matches!(self.chars[j - 1], '\n' | '\r') {
                return true;
            }
            spaced = true;
            j -= 1;
        }
        j == 0 || (spaced && ".!?。;:".contains(self.chars[j - 1]))
    }
}

fn search(docs: &[DocPara], cur: Cursor, needle: &[char]) -> Option<Cursor> {
    let last = (cur.d + LOOKAHEAD_PARAGRAPHS).min(docs.len());
    (cur.d..last).find_map(|d| {
        let from = if d == cur.d { cur.o } else { 0 };
        let p = &docs[d];
        p.norm[from..]
            .windows(needle.len())
            .enumerate()
            .filter(|(_, w)| *w == needle)
            .map(|(i, _)| from + i)
            .find(|&o| p.starts_on_boundary(o) && p.ends_on_boundary(o + needle.len()))
            .map(|o| Cursor { d, o })
    })
}

#[derive(Clone, Copy)]
struct Hit {
    d: usize,
    start: usize,
    end: usize,
}

#[derive(PartialEq)]
enum Key {
    Group(String),
    Doc(usize),
    Alone(usize),
}

/// 커서와 맞은 자리 사이 틈(한 문단 안): 틈에 낀 안 맞은 세그먼트들.
struct Gap {
    d: usize,
    start: usize,
    end: usize,
    run: Vec<usize>,
    prev: Option<usize>,
    next: usize,
}

/// 세그먼트를 문서 글에 붙잡는다 — 문장 가르기와 무관하게 문서 글을 차례로 먹어 가며(커서), 세그먼트 경계가
/// 곧 서안의 문장이다. 정규화한 `target` 이 커서 자리의 글과 같으면 맞음. 아니면 글이 10자 이상일 때만 현재 문단과
/// 뒤 2문단 안에서 찾는다(틈이 생긴다). 틈 한 문단 안에서 안 맞은 세그먼트 수 = 틈을 `sentences` 로 가른 조각 수면
/// 문장 짝, 다르면 그 문단 짝 하나(이웃의 문장 짝은 그대로). 틈이 문단을 넘으면 짝 없음. 맞은 세그먼트가 하나도 없는
/// 묶음이 p−1 · p+1 문단에 맞은 묶음 사이에 있으면 문단 p 와 문단 짝. `target` 이 비었거나 잠긴 세그먼트는 짝도 틈
/// 셈도 없다. 원문과 같은 `target` 은 커서 자리에 있을 때만 짝(아니면 조용히 건너뜀).
/// 계산량: 세그먼트마다 커서 비교 O(글자) + 최대 3문단 훑기 — 문서 길이에 선형(앞보기 상한 고정).
/// `pins` 는 쓸 수 없으면 `dropped_pins` 로 돌려준다.
pub fn align_xliff(target: &[EditorParagraph], segments: &[Segment], pins: &[Pin]) -> Alignment {
    let tclaim = target_claims(target, &paragraph_keys(target));
    let tsent0 = target_sentence_ranges(target, &tclaim);
    let docs: Vec<DocPara> = (0..target.len())
        .filter(|&i| !tsent0[i].is_empty())
        .map(|i| doc_para(i, &target[i].text))
        .collect();
    let n = segments.len();
    let src: Vec<String> = segments.iter().map(|s| collapse(&s.source)).collect();
    let tgt: Vec<Vec<char>> = segments.iter().map(|s| norm(&s.target)).collect();

    // 1. 맞추기.
    let mut hits: Vec<Option<Hit>> = vec![None; n];
    let mut gaps: Vec<Gap> = Vec::new();
    let (mut cur, mut run, mut last_hit) = (Cursor { d: 0, o: 0 }, Vec::<usize>::new(), None);
    for k in (0..n).filter(|&k| !src[k].is_empty() && !tgt[k].is_empty()) {
        settle(&docs, &mut cur);
        let equal = tgt[k] == src[k].chars().collect::<Vec<_>>();
        let found = if docs.get(cur.d).is_some_and(|p| {
            p.norm[cur.o..].starts_with(&tgt[k]) && p.ends_on_boundary(cur.o + tgt[k].len())
        }) {
            Some(cur)
        } else if !equal && tgt[k].len() >= MIN_SEARCH_CHARS {
            search(&docs, cur, &tgt[k])
        } else {
            None
        };
        let Some(at) = found else {
            if !equal {
                run.push(k);
            }
            continue;
        };
        if !run.is_empty() && at != cur && at.d == cur.d {
            let mut end = at.o;
            while docs[at.d].norm[end - 1] == ' ' {
                end -= 1;
            }
            gaps.push(Gap {
                d: at.d,
                start: cur.o,
                end,
                run: std::mem::take(&mut run),
                prev: last_hit,
                next: k,
            });
        }
        run.clear();
        let end = at.o + tgt[k].len();
        hits[k] = Some(Hit {
            d: at.d,
            start: at.o,
            end,
        });
        cur = Cursor { d: at.d, o: end };
        last_hit = Some(k);
    }

    // 2. 문서 문장 범위: 맞은 세그먼트 범위 · 안 덮인 글은 `sentences` 로.
    let mut by_doc: Vec<Vec<Hit>> = vec![Vec::new(); docs.len()];
    for h in hits.iter().flatten() {
        by_doc[h.d].push(*h);
    }
    let fill = |d: usize, a: u32, b: u32, out: &mut Vec<(u32, u32)>| {
        if a < b {
            let part: String = docs[d].chars[a as usize..b as usize].iter().collect();
            let claim = tclaim[docs[d].target].is_some();
            out.extend(
                sentences(&part, Lang::Ko, claim)
                    .into_iter()
                    .map(|(s, e)| (a + s, a + e)),
            );
        }
    };
    let mut final_ranges: Vec<Vec<(u32, u32)>> = vec![Vec::new(); target.len()];
    for (d, p) in docs.iter().enumerate() {
        let (mut at, mut out) = (0, Vec::new());
        for h in &by_doc[d] {
            let (s, e) = p.orig(h.start, h.end);
            fill(d, at, s, &mut out);
            out.push((s, e));
            at = e;
        }
        fill(d, at, p.chars.len() as u32, &mut out);
        final_ranges[p.target] = out;
    }
    let index_of = |d: usize, start: u32| {
        final_ranges[docs[d].target]
            .binary_search_by_key(&start, |r| r.0)
            .expect("문장 범위의 시작")
    };

    // 3. 원문 창 문단.
    let hit_doc = |k: usize| hits[k].map(|h| h.d);
    let prev_hit: Vec<Option<usize>> = {
        let mut last = None;
        (0..n)
            .map(|k| {
                let r = last;
                if hits[k].is_some() {
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
        if hits[k].is_some() {
            last = Some(k);
        }
    }
    let key = |k: usize| -> Key {
        if let Some(g) = &segments[k].group {
            return Key::Group(g.clone());
        }
        if let Some(h) = hits[k] {
            return Key::Doc(h.d);
        }
        match (prev_hit[k].and_then(hit_doc), next_hit[k].and_then(hit_doc)) {
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

    // 4. 짝. 둘째 칸은 같은 문단 안 차례 열쇠(문단 짝 = 0, 문장 짝 = 문장 번호 + 1).
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
    let paragraph_pair = |t: usize, sp: u32| Pair {
        target_id: target[t].id.clone(),
        target_sentence: None,
        source_paragraph: sp,
        source_sentence: None,
        grade: Grade::Paragraph,
    };
    let mut pairs: Vec<(usize, u32, Pair)> = Vec::new();
    for (k, h) in hits.iter().enumerate() {
        if let Some(h) = *h {
            let ts = index_of(h.d, docs[h.d].orig(h.start, h.end).0) as u32;
            let t = docs[h.d].target;
            pairs.push((t, ts + 1, pair(t, ts, k)));
        }
    }
    for gap in &gaps {
        let p = &docs[gap.d];
        let (a, b) = p.orig(gap.start, gap.end);
        let mut pieces = Vec::new();
        fill(gap.d, a, b, &mut pieces);
        if pieces.len() == gap.run.len() {
            for (&k, piece) in gap.run.iter().zip(&pieces) {
                let ts = index_of(gap.d, piece.0) as u32;
                pairs.push((p.target, ts + 1, pair(p.target, ts, k)));
            }
            continue;
        }
        let mut owners: Vec<u32> = gap
            .run
            .iter()
            .filter_map(|&k| at_source[k])
            .map(|a| a.0)
            .collect();
        owners.sort_unstable();
        owners.dedup();
        let neighbour = |k: Option<usize>| k.and_then(|k| at_source[k]).map(|a| a.0);
        if let [only] = owners[..]
            && (neighbour(gap.prev) == Some(only) || neighbour(Some(gap.next)) == Some(only))
            && !pairs.iter().any(|q| q.0 == p.target && q.1 == 0)
        {
            pairs.push((p.target, 0, paragraph_pair(p.target, only)));
        }
    }
    // 맞은 세그먼트가 없는 묶음이 맞은 이웃 묶음 사이(문단 p−1 · p+1)에 있으면 문단 짝.
    let span_of = |i: usize| {
        let ds: Vec<usize> = (0..n)
            .filter(|&k| at_source[k].is_some_and(|a| a.0 as usize == i))
            .filter_map(hit_doc)
            .collect();
        ds.first().copied().zip(ds.last().copied())
    };
    for (i, key) in source_keys.iter().enumerate().skip(1) {
        if i + 1 >= source.len() || !matches!(key, Key::Group(_)) || span_of(i).is_some() {
            continue;
        }
        if let (Some((_, a)), Some((b, _))) = (span_of(i - 1), span_of(i + 1))
            && b == a + 2
        {
            let t = docs[a + 1].target;
            pairs.push((t, 0, paragraph_pair(t, i as u32)));
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
                pin.target_sentence + 1,
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

    // 6. 2단계: 문장 짝도 문단 짝도 못 받은 문서 문단 — XLIFF 묶음을 원문 문단, 세그먼트를 원문 문장으로 보고
    //    추정 정렬(`infer`: 식별번호 · 청구항 번호 · 표제 닻 → 닻 사이 차례 → 문장 수 같고 숫자 관문 통과 때만 문장 짝).
    //    1단계 짝이 우선이고(이미 짝이 있는 문단은 건드리지 않는다), 1단계가 쓴 묶음은 문장 짝을 못 낸다.
    //    문장 칸에서 뺀다: 번역이 빈 세그먼트, 1단계에 안 맞은 원문과 같은 번역(번호 · 기호류).
    let mut free = vec![true; target.len()];
    let mut used = vec![false; source.len()];
    let mut first_source: std::collections::BTreeMap<usize, u32> =
        std::collections::BTreeMap::new();
    for (t, _, p) in &pairs {
        free[*t] = false;
        used[p.source_paragraph as usize] = true;
        let at = first_source.entry(*t).or_insert(p.source_paragraph);
        *at = (*at).min(p.source_paragraph);
    }
    let fixed: Vec<(usize, usize)> = first_source
        .iter()
        .map(|(&t, &s)| (t, s as usize))
        .collect();
    let mut kept: Vec<Vec<u32>> = vec![Vec::new(); source.len()];
    let mut ssent: Vec<Vec<(u32, u32)>> = vec![Vec::new(); source.len()];
    for k in 0..n {
        let Some((sp, ss)) = at_source[k] else {
            continue;
        };
        let skipped = tgt[k].is_empty()
            || (hits[k].is_none() && tgt[k] == src[k].chars().collect::<Vec<_>>());
        if !skipped {
            kept[sp as usize].push(ss);
            ssent[sp as usize].push(source[sp as usize].sentences[ss as usize]);
        }
    }
    let texts: Vec<String> = source.iter().map(|p| p.text.clone()).collect();
    let given = Given {
        ssent: &ssent,
        free: &free,
        used: &used,
        fixed: &fixed,
    };
    for (t, mut p) in infer(target, &texts, &[], Some(&given)).pairs {
        let order = match p.target_sentence {
            Some(ts) => {
                p.source_sentence = p
                    .source_sentence
                    .map(|j| kept[p.source_paragraph as usize][j as usize]);
                ts + 1
            }
            None => 0,
        };
        pairs.push((t, order, p));
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
