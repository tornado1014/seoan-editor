//! 서식 · 구조 점검: 통특기 매뉴얼 6장 오류코드(E-xxx)가 거는 것 — 명칭 꼴 · 식별항목 짝 · 순서 · 필수 · 번호 ·
//! 표제 서식 · 그림 크기 · 청구항 · 표. 명세서에만 건다(의견서 등은 본문에 【】를 인용할 수 있다).
//! 순서 · 필수 · 모르는 표제는 국내 명세서(【발명의 설명】)에만 — PCT 번역문 · 옛 서식은 표제가 자유롭다.
//! 필수 표제는 실물 최종본 228건 모두에 있는 것만 둔다(【발명의 효과】 · 【해결하려는 과제】가 없는 접수본이 있다).

use std::collections::BTreeSet;

use crate::submit_check::heading;
use crate::{EditorParagraph, SubmitIssue, SubmitKind};

/// 그림 한 장(제출 전 점검용).
#[derive(Debug, Clone)]
pub struct CheckImage {
    pub paragraph_id: String,
    /// 표 칸 안 그림(도면 · 대표도 규칙에서 뺀다).
    pub in_cell: bool,
}

/// 문단 밖의 것: 그림 · 표 앵커 · 칸 글.
#[derive(Debug, Clone, Default)]
pub struct CheckObjects {
    pub images: Vec<CheckImage>,
    pub table_anchors: BTreeSet<String>,
    pub cell_texts: Vec<String>,
    /// 최종본(.fin)에서 연 문서 — 보정으로 지운 청구항의 「삭제」는 정상이다.
    pub from_final: bool,
    /// 줄 간격이 표준(280%)이 아닌 본문 문단.
    pub nonstandard_spacing: BTreeSet<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Domestic,
    Loose,
}

/// ⛔ E-218(그림 165×222mm 초과)은 걸지 않는다: 접수된 실물 최종본 XML 그림 2,471장 중 564장이 폭 165mm 를 넘고
/// 최대 208×234mm 다 — 통특기는 제출파일을 만들 때 이 크기를 막지 않는다.
/// 명칭 한글 · 영문 각각의 한도(CP949 바이트) — E-263 · E-270.
const TITLE_BYTES: usize = 540;

/// 국내 명세서 표제 차례(매뉴얼 5.1.1). 「고안」은 「발명」으로 · 옛 이름은 새 이름으로 맞춘 뒤 본다.
const ORDER: [&str; 25] = [
    "발명의 설명",
    "발명의 명칭",
    "기술분야",
    "발명의 배경이 되는 기술",
    "선행기술문헌",
    "특허문헌",
    "비특허문헌",
    "발명의 내용",
    "해결하려는 과제",
    "과제의 해결 수단",
    "발명의 효과",
    "도면의 간단한 설명",
    "발명을 실시하기 위한 구체적인 내용",
    "산업상 이용가능성",
    "부호의 설명",
    "수탁번호",
    "서열목록",
    "청구범위",
    "청구항",
    "요약서",
    "요약",
    "대표도",
    "색인어",
    "도면",
    "도",
];
/// 실물 최종본 228건 모두에 있는 표제.
const REQUIRED: [&str; 8] = [
    "발명의 설명",
    "발명의 명칭",
    "기술분야",
    "발명의 배경이 되는 기술",
    "발명의 내용",
    "발명을 실시하기 위한 구체적인 내용",
    "요약서",
    "요약",
];
/// 바로 아래 하위 표제가 와야 하는 묶음 표제 — 그 사이에 글을 쓸 수 없다(E-249).
const PARENTS: [&str; 6] = [
    "발명의 설명",
    "선행기술문헌",
    "발명의 내용",
    "청구범위",
    "요약서",
    "도면",
];
/// 번호가 꼭 붙는 표제(E-234). 「실시예」는 번호가 없어도 된다.
const NUMBERED: [&str; 9] = [
    "청구항",
    "표",
    "수학식",
    "화학식",
    "반응식",
    "수",
    "화",
    "도",
    "실시예",
];
/// 청구범위 안에 올 수 없는 공통 표제(E-310).
const COMMON: [&str; 7] = ["표", "수학식", "화학식", "반응식", "수", "화", "실시예"];
/// PCT 번역문 · 옛 서식에만 있는 표제.
const LOOSE_MARKERS: [&str; 7] = [
    "명세서",
    "발명의 상세한 설명",
    "기술적 과제",
    "기술적 해결방법",
    "발명의 실시를 위한 형태",
    "발명의 실시를 위한 최선의 형태",
    "청구의 범위",
];

/// 번호 없는 표준 표제의 작성지침 차례(번호 표제 · 모르는 이름은 `None`). 「고안」 · 옛 이름도 받는다.
pub(crate) fn standard_rank(name: &str) -> Option<usize> {
    let name = normalize(name);
    let (base, number) = split_number(&name);
    if number.is_some() || NUMBERED.contains(&base) {
        return None;
    }
    ORDER.iter().position(|o| *o == base)
}

fn normalize(name: &str) -> String {
    let name = name.trim().replace("고안", "발명");
    match name.as_str() {
        "특허청구범위" | "실용신안등록청구범위" => "청구범위".into(),
        "배경기술" => "발명의 배경이 되는 기술".into(),
        "해결하고자 하는 과제" => "해결하려는 과제".into(),
        _ => name,
    }
}

/// 표제 이름 → (밑 이름, 번호). 번호 표제가 아니면 번호 없음.
fn split_number(name: &str) -> (&str, Option<&str>) {
    for base in NUMBERED {
        if name == base {
            return (base, None);
        }
        if let Some(rest) = name.strip_prefix(base).and_then(|r| r.strip_prefix(' ')) {
            return (base, Some(rest.trim()));
        }
    }
    (name, None)
}

/// 번호 꼴: 숫자, 또는 숫자(3자리까지) + 영소문자 하나(E-232 · E-318). 청구항은 숫자만(E-231).
fn number_ok(base: &str, number: &str) -> bool {
    let digits = number.chars().take_while(char::is_ascii_digit).count();
    let rest: Vec<char> = number.chars().skip(digits).collect();
    if digits == 0 {
        return false;
    }
    match rest.as_slice() {
        [] => true,
        [letter] if base != "청구항" => digits <= 3 && letter.is_ascii_lowercase(),
        _ => false,
    }
}

fn kind(paragraphs: &[EditorParagraph]) -> Option<Kind> {
    let names: Vec<String> = paragraphs
        .iter()
        .filter_map(|p| heading(&p.text))
        .map(normalize)
        .collect();
    let spec = names
        .iter()
        .any(|n| n == "발명의 설명" || n == "DESCRIPTION" || n == "명세서");
    if !spec {
        return None;
    }
    let loose = names
        .iter()
        .any(|n| LOOSE_MARKERS.contains(&n.as_str()) || n.is_ascii());
    Some(if loose { Kind::Loose } else { Kind::Domestic })
}

fn cp949_len(text: &str) -> usize {
    encoding_rs::EUC_KR.encode(text).0.len()
}

/// 그림 자리표(`_`)와 공백을 뺀 글.
fn words(text: &str, images: usize) -> String {
    let mut left = images;
    text.chars()
        .filter(|ch| !ch.is_whitespace())
        .filter(|ch| {
            if *ch == '_' && left > 0 {
                left -= 1;
                false
            } else {
                true
            }
        })
        .collect()
}

fn preview(text: &str) -> String {
    let text = text.trim();
    let mut out: String = text.chars().take(24).collect();
    if text.chars().count() > 24 {
        out.push('…');
    }
    out
}

/// 문서에 있는 표제 이름(정규화 · 번호 뗀 것). 빈 이름 · 따옴표 이름은 뺀다(E-211 · E-233 이 따로 건다).
fn present_bases(headings: &[(usize, String)]) -> BTreeSet<String> {
    headings
        .iter()
        .map(|(_, raw)| raw)
        .filter(|raw| !raw.trim().is_empty() && !raw.contains(['\'', '"', '‘', '’', '“', '”']))
        .map(|raw| split_number(&normalize(raw)).0.to_string())
        .collect()
}

/// 빠진 필수 표제(E-245) — 국내 명세서 꼴일 때만. 제출파일 관문(`hlt-cli export-fin`)과 점검 목록이 같이 쓴다.
pub fn missing_required_headings(paragraphs: &[EditorParagraph]) -> Vec<&'static str> {
    if kind(paragraphs) != Some(Kind::Domestic) {
        return Vec::new();
    }
    let headings: Vec<(usize, String)> = paragraphs
        .iter()
        .enumerate()
        .filter_map(|(at, p)| heading(&p.text).map(|name| (at, name.to_string())))
        .collect();
    let present = present_bases(&headings);
    REQUIRED
        .into_iter()
        .filter(|name| !present.contains(*name))
        .collect()
}

/// 서식 · 구조 점검(명세서가 아니면 빈 목록).
pub fn format_checks(paragraphs: &[EditorParagraph], objects: &CheckObjects) -> Vec<SubmitIssue> {
    let Some(kind) = kind(paragraphs) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut push =
        |kind: SubmitKind, code: &'static str, p: Option<&EditorParagraph>, detail: String| {
            out.push(SubmitIssue {
                kind,
                code: Some(code),
                paragraph_id: p.map(|p| p.id.clone()),
                detail,
            })
        };
    let images_in = |p: &EditorParagraph| {
        objects
            .images
            .iter()
            .filter(|image| !image.in_cell && image.paragraph_id == p.id)
            .count()
    };
    let filled = |p: &EditorParagraph| p.has_objects || !p.text.trim().is_empty();
    let headings: Vec<(usize, String)> = paragraphs
        .iter()
        .enumerate()
        .filter_map(|(at, p)| heading(&p.text).map(|name| (at, name.to_string())))
        .collect();
    // 표제 다음 문단들(다음 표제 앞까지).
    let body = |at: usize| -> &[EditorParagraph] {
        let end = paragraphs[at + 1..]
            .iter()
            .position(|p| heading(&p.text).is_some())
            .map_or(paragraphs.len(), |n| at + 1 + n);
        &paragraphs[at + 1..end]
    };

    // 2. 식별기호 · 식별항목 꼴.
    for p in paragraphs {
        let text = p.text.trim();
        let (opens, closes) = (text.matches('【').count(), text.matches('】').count());
        if opens == 0 && closes == 0 {
            continue;
        }
        if let Some(name) = heading(text) {
            if name.trim().is_empty() {
                push(
                    SubmitKind::Structure,
                    "E-211",
                    Some(p),
                    "【 】 — 식별항목 이름이 없습니다".into(),
                );
            } else if name.contains(['\'', '"', '‘', '’', '“', '”']) {
                push(
                    SubmitKind::Structure,
                    "E-233",
                    Some(p),
                    format!("【{name}】 — 이름에 따옴표를 쓸 수 없습니다"),
                );
            }
            continue;
        }
        let paired = opens == closes && {
            let mut depth = 0i32;
            text.chars().all(|ch| {
                match ch {
                    '【' => depth += 1,
                    '】' => depth -= 1,
                    _ => {}
                }
                (0..=1).contains(&depth)
            })
        };
        let (code, what) = if !paired {
            if closes == 0 {
                ("E-213", "식별기호가 닫히지 않았습니다")
            } else {
                ("E-212", "【 와 】 의 짝이 맞지 않습니다")
            }
        } else if text.starts_with('【') {
            ("E-214", "식별항목 뒤에 글 — 내용은 다음 문단에 씁니다")
        } else if text.ends_with('】') {
            ("E-303", "식별항목 앞에 글 — 내용은 다음 문단에 씁니다")
        } else {
            ("E-206", "식별항목이 아닌 곳에 【 】 를 쓸 수 없습니다")
        };
        push(
            SubmitKind::Structure,
            code,
            Some(p),
            format!("「{}」 — {what}", preview(text)),
        );
    }

    // 2. 번호 · 순서 · 묶음 · 필수 · 모르는 표제 · 【도 N】 뒤 빈 문단.
    let mut rank_seen = 0usize;
    let present = present_bases(&headings);
    let mut in_claims = false;
    let mut last_claim: Option<u64> = None;
    for &(at, ref raw) in &headings {
        let p = &paragraphs[at];
        // 빈 이름 · 따옴표 이름은 위(E-211 · E-233)에서 이미 걸었다.
        if raw.trim().is_empty() || raw.contains(['\'', '"', '‘', '’', '“', '”']) {
            continue;
        }
        let name = normalize(raw);
        let (base, number) = split_number(&name);
        match (base, number) {
            ("실시예", _) => {}
            (b, None) if NUMBERED.contains(&b) => push(
                SubmitKind::Structure,
                "E-234",
                Some(p),
                format!("【{raw}】 — 일련번호가 없습니다"),
            ),
            (b, Some(n)) if NUMBERED.contains(&b) && !number_ok(b, n) => push(
                SubmitKind::Structure,
                if b == "청구항" { "E-231" } else { "E-232" },
                Some(p),
                format!("【{raw}】 — 번호는 숫자 또는 숫자(3자리까지)+영소문자 하나"),
            ),
            _ => {}
        }
        if kind == Kind::Domestic && !COMMON.contains(&base) {
            match ORDER.iter().position(|o| *o == base) {
                None => push(
                    SubmitKind::Structure,
                    "E-201",
                    Some(p),
                    format!("【{raw}】 — 작성지침에 없는 식별항목"),
                ),
                Some(rank) if rank < rank_seen => push(
                    SubmitKind::Structure,
                    "E-244",
                    Some(p),
                    format!("【{raw}】 — 작성지침과 차례가 다릅니다"),
                ),
                Some(rank) => rank_seen = rank,
            }
        }
        if PARENTS.contains(&base)
            && let Some(stray) = body(at).iter().find(|q| filled(q))
        {
            push(
                SubmitKind::Structure,
                "E-249",
                Some(stray),
                format!("【{raw}】 바로 아래에는 하위 식별항목이 와야 합니다"),
            );
        }
        if base == "도"
            && number.is_some()
            && body(at).first().is_some_and(|q| !filled(q))
            && body(at).iter().any(filled)
        {
            push(
                SubmitKind::Structure,
                "S-010",
                Some(p),
                format!("【{raw}】 — 식별항목과 도면 사이에 빈 문단"),
            );
        }
        // 3. 표제 서식(E-260) · 명칭 · 색인어 · 대표도 내용 서식(E-261).
        if p.spans.iter().any(|s| s.style != 0) {
            push(
                SubmitKind::HeadingStyle,
                "E-260",
                Some(p),
                format!("【{raw}】 — 식별항목에 글자 속성"),
            );
        }
        if matches!(base, "발명의 명칭" | "색인어" | "대표도")
            && let Some(q) = body(at)
                .iter()
                .find(|q| q.spans.iter().any(|s| s.style != 0))
        {
            push(
                SubmitKind::HeadingStyle,
                "E-261",
                Some(q),
                format!("【{raw}】 내용에 글자 속성(굵게 · 기울임 · 밑줄 · 첨자)"),
            );
        }
        // 5. 청구항.
        match base {
            "청구범위" => in_claims = true,
            "요약서" | "요약" | "도면" => in_claims = false,
            _ => {}
        }
        if in_claims && COMMON.contains(&base) {
            push(
                SubmitKind::ClaimBody,
                "E-310",
                Some(p),
                format!("【{raw}】 — 청구항 안에 식별항목을 쓸 수 없습니다"),
            );
        }
        if base == "청구항"
            && let Some(n) = number.and_then(|n| n.parse::<u64>().ok())
        {
            let expected = last_claim.map_or(1, |l| l + 1);
            // 최종본은 보정으로 지운 항 번호가 빈다(실물 163건) — 새로 쓰는 명세서에만 건다.
            if n != expected && Some(n) != last_claim && !objects.from_final {
                push(
                    SubmitKind::ClaimBody,
                    "E-252",
                    Some(p),
                    format!("【{raw}】 — 청구항 {expected} 이 올 자리"),
                );
            }
            last_claim = Some(n);
            let text: String = body(at)
                .iter()
                .flat_map(|q| q.text.chars())
                .filter(|c| !c.is_whitespace())
                .collect();
            if text == "삭제" && !objects.from_final {
                push(
                    SubmitKind::ClaimBody,
                    "E-307",
                    Some(p),
                    format!("【{raw}】 — 내용으로 「삭제」는 쓸 수 없습니다"),
                );
            }
        }
        // 1. 명칭.
        if kind == Kind::Domestic && base == "발명의 명칭" {
            title_issues(
                &mut push,
                raw,
                body(at).iter().filter(|q| filled(q)).collect(),
            );
        }
        // 4. 도 · 대표도.
        if base == "도" && number.is_some() {
            let lines: Vec<&EditorParagraph> = body(at).iter().filter(|q| filled(q)).collect();
            let images: usize = lines.iter().map(|q| images_in(q)).sum();
            let text = lines
                .iter()
                .any(|q| !words(&q.text, images_in(q)).is_empty());
            if !lines.is_empty() && (images != 1 || text) {
                push(
                    SubmitKind::Image,
                    "E-240",
                    Some(p),
                    format!(
                        "【{raw}】 — 다음 행에 그림 1개만(지금 그림 {images}개{})",
                        if text { " · 글 있음" } else { "" }
                    ),
                );
            }
        }
        if base == "대표도" {
            let lines: Vec<&EditorParagraph> = body(at).iter().filter(|q| filled(q)).collect();
            let images: usize = lines.iter().map(|q| images_in(q)).sum();
            let text = lines
                .iter()
                .any(|q| !words(&q.text, images_in(q)).is_empty());
            if images > 0 && (text || images > 1) {
                push(
                    SubmitKind::Image,
                    "E-242",
                    Some(p),
                    "【대표도】 — 도 번호 하나 또는 그림 1개만".into(),
                );
            }
        }
        // 6. 표.
        if base == "표" && number.is_some() {
            let lines: Vec<&EditorParagraph> = body(at).iter().filter(|q| filled(q)).collect();
            let titles = lines.iter().take_while(|q| !q.has_objects).count();
            // 제목 뒤에 표 · 그림이 올 때만 — 표를 글줄로 적은 접수본이 있다.
            if titles > 1 && titles < lines.len() {
                push(
                    SubmitKind::Table,
                    "E-215",
                    Some(p),
                    format!("【{raw}】 — 표 제목은 한 문단까지(지금 {titles}문단)"),
                );
            }
            let images: usize = lines.iter().map(|q| images_in(q)).sum();
            let table = lines.iter().any(|q| objects.table_anchors.contains(&q.id));
            if !table && images > 1 {
                push(
                    SubmitKind::Table,
                    "E-271",
                    Some(p),
                    format!("【{raw}】 — 표를 그림으로 넣을 때는 1개만(지금 {images}개)"),
                );
            }
        }
    }
    for name in missing_required_headings(paragraphs) {
        push(
            SubmitKind::Structure,
            "E-245",
            None,
            format!("【{name}】이 없습니다"),
        );
    }
    if kind == Kind::Domestic {
        if present.contains("청구범위") && !present.contains("청구항") {
            push(
                SubmitKind::Structure,
                "E-245",
                None,
                "【청구범위】 아래 【청구항 1】이 없습니다".into(),
            );
        }
        if present.contains("도") {
            for name in ["도면의 간단한 설명", "도면"] {
                if !present.contains(name) {
                    push(
                        SubmitKind::Structure,
                        "E-251",
                        None,
                        format!("【도 N】이 있는데 【{name}】이 없습니다"),
                    );
                }
            }
        }
    }

    // 6. 표 칸 안 【】.
    for text in &objects.cell_texts {
        if text.contains(['【', '】']) {
            push(
                SubmitKind::Table,
                "E-203",
                None,
                format!(
                    "표 칸 「{}」 — 칸 안에 【 】 를 쓸 수 없습니다",
                    preview(text)
                ),
            );
        }
    }
    // 표준서식(매뉴얼 4.3.3 · 5.1.1): 본문 줄 간격 280% · 글자 12pt. 실물 228건에 하나도 없다.
    for p in paragraphs {
        let spacing = objects.nonstandard_spacing.contains(&p.id);
        let sized: BTreeSet<u32> = p
            .spans
            .iter()
            .filter_map(|s| s.size)
            .filter(|z| *z != 12)
            .collect();
        if spacing || !sized.is_empty() {
            let mut what = Vec::new();
            if spacing {
                what.push("줄 간격이 280%가 아닙니다".to_string());
            }
            if !sized.is_empty() {
                let list: Vec<String> = sized.iter().map(|z| format!("{z}pt")).collect();
                what.push(format!("글자 크기 {}", list.join(" · ")));
            }
            out.push(SubmitIssue {
                kind: SubmitKind::Format,
                code: None,
                paragraph_id: Some(p.id.clone()),
                detail: format!("「{}」 — {}", preview(&p.text), what.join(" · ")),
            });
        }
    }
    // 매뉴얼 규칙이지만 이대로 접수된 실물이 있는 것(실물 최종본 228건 중 E-266 5 · E-249 2 · E-271 3 · E-215 3건) — 확인용.
    for issue in &mut out {
        if matches!(issue.code, Some("E-266" | "E-249" | "E-271" | "E-215")) {
            issue
                .detail
                .push_str(" (확인용 — 이대로 접수된 실물도 있습니다)");
        }
    }
    out
}

/// 【발명의 명칭】 내용: 한 문단 · 「한글 명칭{English Title}」 · 각 540바이트.
fn title_issues(
    push: &mut impl FnMut(SubmitKind, &'static str, Option<&EditorParagraph>, String),
    raw: &str,
    lines: Vec<&EditorParagraph>,
) {
    let Some(first) = lines.first() else {
        return;
    };
    let mut say = |code, what: String| push(SubmitKind::TitleFormat, code, Some(first), what);
    if lines.len() > 1 {
        say(
            "E-262",
            format!("【{raw}】 내용은 한 문단 — 지금 {}문단", lines.len()),
        );
    }
    if first.has_objects {
        say(
            "E-247",
            format!("【{raw}】에는 표 · 그림을 넣을 수 없습니다"),
        );
    }
    let text = first.text.trim();
    let Some(open) = text.find('{') else {
        say(
            "E-266",
            "한글 명칭 오른쪽에 {영문 명칭}을 함께 씁니다".into(),
        );
        return;
    };
    let korean = text[..open].trim();
    if korean.is_empty() {
        say("E-300", "한글 명칭이 없습니다".into());
    }
    let after = &text[open + 1..];
    let Some(close) = after.rfind('}') else {
        say("E-269", "영문 명칭에 닫는 괄호 } 가 없습니다".into());
        return;
    };
    let english = &after[..close];
    let tail = after[close + 1..].trim();
    if !tail.is_empty() {
        say(
            "E-268",
            format!("닫는 괄호 }} 뒤에 글 「{}」", preview(tail)),
        );
    }
    if let Some(bad) = english.chars().find(|ch| !ch.is_ascii()) {
        say(
            "E-267",
            format!("영문 명칭에 영문 · 기호 아닌 글자 「{bad}」"),
        );
    }
    if cp949_len(korean) > TITLE_BYTES {
        say(
            "E-263",
            format!("한글 명칭 {}바이트 — 최대 {TITLE_BYTES}", cp949_len(korean)),
        );
    }
    if english.len() > TITLE_BYTES {
        say(
            "E-270",
            format!("영문 명칭 {}바이트 — 최대 {TITLE_BYTES}", english.len()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EditorSpan;

    fn doc(rows: &[&str]) -> Vec<EditorParagraph> {
        rows.iter()
            .enumerate()
            .map(|(i, text)| EditorParagraph {
                id: format!("p{i}"),
                text: (*text).into(),
                wipo_id: None,
                number: None,
                has_objects: *text == "_",
                spans: Vec::new(),
            })
            .collect()
    }

    /// 매뉴얼 5.1.1 차례를 다 갖춘 국내 명세서(걸리는 것 없음).
    fn good() -> Vec<&'static str> {
        vec![
            "【발명의 설명】",
            "【발명의 명칭】",
            "시험 장치{TEST APPARATUS}",
            "【기술분야】",
            "본 발명은 시험 장치에 관한 것이다.",
            "【발명의 배경이 되는 기술】",
            "배경.",
            "【발명의 내용】",
            "【해결하려는 과제】",
            "과제.",
            "【과제의 해결 수단】",
            "수단.",
            "【발명의 효과】",
            "효과.",
            "【도면의 간단한 설명】",
            "도 1은 장치이다.",
            "【발명을 실시하기 위한 구체적인 내용】",
            "몸체가 있다.",
            "【표 1】",
            "표 제목",
            "【청구범위】",
            "【청구항 1】",
            "몸체를 포함하는 장치.",
            "【청구항 2】",
            "제1항에 있어서, 덮개를 더 포함하는 장치.",
            "【요약서】",
            "【요약】",
            "시험 장치.",
            "【대표도】",
            "도 1",
            "【도면】",
            "【도 1】",
            "_",
        ]
    }

    fn objects(paragraphs: &[EditorParagraph]) -> CheckObjects {
        CheckObjects {
            images: paragraphs
                .iter()
                .filter(|p| p.text == "_")
                .map(|p| CheckImage {
                    paragraph_id: p.id.clone(),
                    in_cell: false,
                })
                .collect(),
            ..Default::default()
        }
    }

    fn codes(rows: &[&str]) -> Vec<&'static str> {
        let paragraphs = doc(rows);
        let objects = objects(&paragraphs);
        format_checks(&paragraphs, &objects)
            .into_iter()
            .filter_map(|issue| issue.code)
            .collect()
    }

    fn with(edit: impl FnOnce(&mut Vec<&'static str>)) -> Vec<&'static str> {
        let mut rows = good();
        edit(&mut rows);
        codes(&rows)
    }

    fn replace(rows: &mut [&'static str], from: &str, to: &'static str) {
        let at = rows.iter().position(|r| *r == from).unwrap();
        rows[at] = to;
    }

    #[test]
    fn a_complete_domestic_specification_passes() {
        assert_eq!(codes(&good()), Vec::<&str>::new());
    }

    #[test]
    fn title_format_follows_the_manual() {
        let title = |t: &'static str| with(|r| replace(r, "시험 장치{TEST APPARATUS}", t));
        assert_eq!(title("시험 장치"), ["E-266"]);
        assert_eq!(title("{TEST APPARATUS}"), ["E-300"]);
        assert_eq!(title("시험 장치{TEST APPARATUS"), ["E-269"]);
        assert_eq!(title("시험 장치{TEST APPARATUS} 끝"), ["E-268"]);
        assert_eq!(title("시험 장치{TEST 장치}"), ["E-267"]);
        let long: &'static str = Box::leak(format!("{}{{A}}", "가".repeat(271)).into_boxed_str());
        assert_eq!(title(long), ["E-263"]);
        let long: &'static str = Box::leak(format!("가{{{}}}", "A".repeat(541)).into_boxed_str());
        assert_eq!(title(long), ["E-270"]);
        assert_eq!(
            with(|r| {
                let at = r
                    .iter()
                    .position(|x| *x == "시험 장치{TEST APPARATUS}")
                    .unwrap();
                r.insert(at + 1, "{MORE}");
            }),
            ["E-262"]
        );
    }

    #[test]
    fn identifier_brackets_follow_the_manual() {
        let body = |t: &'static str| with(|r| replace(r, "몸체가 있다.", t));
        assert_eq!(body("【발명의 명칭】】"), ["E-212"]);
        assert_eq!(body("【기술분야"), ["E-213"]);
        assert_eq!(body("【청구항 2】 내용"), ["E-214"]);
        assert_eq!(body("내용【청구항 1】"), ["E-303"]);
        assert_eq!(body("요점 【적용된 기술의 요점】 요점"), ["E-206"]);
        assert_eq!(body("【 】"), ["E-211"]);
        assert_eq!(with(|r| replace(r, "【표 1】", "【표 “1”】")), ["E-233"]);
    }

    #[test]
    fn numbers_order_and_required_headings_follow_the_manual() {
        assert_eq!(with(|r| replace(r, "【표 1】", "【표】")), ["E-234"]);
        assert_eq!(
            with(|r| replace(r, "【청구항 2】", "【청구항 2a】")),
            ["E-231"]
        );
        assert_eq!(with(|r| replace(r, "【도 1】", "【도 1aa】")), ["E-232"]);
        assert_eq!(with(|r| replace(r, "【표 1】", "【그림 1】")), ["E-201"]);
        // 배경기술을 기술분야 앞으로.
        assert_eq!(
            with(|r| {
                let at = r
                    .iter()
                    .position(|x| *x == "【발명의 배경이 되는 기술】")
                    .unwrap();
                let moved: Vec<_> = r.drain(at..at + 2).collect();
                r.splice(3..3, moved);
            }),
            ["E-244"]
        );
        assert_eq!(
            with(|r| {
                let at = r.iter().position(|x| *x == "【기술분야】").unwrap();
                r.drain(at..at + 2);
            }),
            ["E-245"]
        );
        assert_eq!(
            with(|r| {
                let at = r.iter().position(|x| *x == "【발명의 내용】").unwrap();
                r.insert(at + 1, "내용 글");
            }),
            ["E-249"]
        );
        assert_eq!(
            with(|r| {
                let at = r
                    .iter()
                    .position(|x| *x == "【도면의 간단한 설명】")
                    .unwrap();
                r.drain(at..at + 2);
            }),
            ["E-251"]
        );
        // 옛 이름 · 고안은 같은 표제다.
        assert_eq!(
            with(|r| replace(r, "【발명의 배경이 되는 기술】", "【배경기술】")),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn heading_and_title_styles_are_refused() {
        let styled = |text: &str, style: u32| {
            let mut paragraphs = doc(&good());
            let p = paragraphs.iter_mut().find(|p| p.text == text).unwrap();
            p.spans = vec![EditorSpan {
                start: 0,
                end: 2,
                style,
                size: None,
            }];
            let objects = objects(&paragraphs);
            format_checks(&paragraphs, &objects)
                .into_iter()
                .filter_map(|i| i.code)
                .collect::<Vec<_>>()
        };
        assert_eq!(styled("【기술분야】", 2), ["E-260"]);
        assert_eq!(styled("시험 장치{TEST APPARATUS}", 4), ["E-261"]);
        assert_eq!(styled("몸체가 있다.", 2), Vec::<&str>::new());
    }

    #[test]
    fn claims_and_tables_follow_the_manual() {
        assert_eq!(
            with(|r| {
                let at = r.iter().position(|x| *x == "【요약서】").unwrap();
                r.splice(at..at, ["【표 2】", "표"]);
            }),
            ["E-310"]
        );
        assert_eq!(
            with(|r| replace(r, "【청구항 2】", "【청구항 3】")),
            ["E-252"]
        );
        assert_eq!(
            with(|r| replace(r, "제1항에 있어서, 덮개를 더 포함하는 장치.", "삭 제")),
            ["E-307"]
        );
        assert_eq!(
            with(|r| {
                let at = r.iter().position(|x| *x == "표 제목").unwrap();
                r.splice(at + 1..at + 1, ["표 제목 둘째 줄", "_"]);
            }),
            ["E-215"]
        );
        // 최종본에서 연 문서의 「삭제」는 보정으로 지운 항이다.
        let mut rows = good();
        replace(
            &mut rows,
            "제1항에 있어서, 덮개를 더 포함하는 장치.",
            "삭제",
        );
        // 지운 항 번호가 빈 것(2 → 3)도 최종본에서는 정상.
        replace(&mut rows, "【청구항 2】", "【청구항 3】");
        let paragraphs = doc(&rows);
        let mut objects = objects(&paragraphs);
        objects.from_final = true;
        objects.cell_texts = vec!["【제출건수】".into()];
        let found: Vec<_> = format_checks(&paragraphs, &objects)
            .into_iter()
            .filter_map(|i| i.code)
            .collect();
        assert_eq!(found, ["E-203"]);
    }

    #[test]
    fn figures_and_images_follow_the_manual() {
        assert_eq!(with(|r| replace(r, "_", "평면도")), ["E-240"]);
        assert_eq!(
            with(|r| {
                let at = r.iter().position(|x| *x == "【도 1】").unwrap();
                r.insert(at + 1, "");
            }),
            ["S-010"]
        );
    }

    #[test]
    fn opinions_are_not_checked() {
        assert!(codes(&["【의견내용】", "【청구항 1】 은 명확하다 【표】", "_"]).is_empty());
    }
}
