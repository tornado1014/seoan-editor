//! CP949 에 없는 글자 → CP949 에 있는 대응 글자. HLT 는 글을 CP949 로 저장하므로
//! Word · KIPO XML · 붙여넣기에서 들어온 글을 굽기 전에 여기서 한 번 바꾼다.
//! 대응표에도 없는 글자는 바꾸지 않고 전부 모아 거부한다(조용히 잃지 않는다).

use std::collections::BTreeMap;

use encoding_rs::EUC_KR;

use crate::{CoreError, ErrorCode, HmlDocument, HmlInline};

/// 바꾼 글자 하나(같은 대응은 한 번 · 횟수).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Cp949Substitution {
    pub from: char,
    pub to: String,
    pub count: usize,
}

/// XML 그림 자리표(보이지 않는 구분자) — 글이 아니라 그림 자리라 그대로 둔다.
const IMAGE_MARK: char = '\u{2063}';

/// 낱글자 대응. 빈 글 = 지운다.
const TABLE: &[(char, &str)] = &[
    // 대시 · 빼기
    ('\u{2010}', "-"),
    ('\u{2011}', "-"),
    ('\u{2012}', "-"),
    ('\u{2013}', "-"),
    ('\u{2212}', "-"),
    ('\u{2014}', "\u{2015}"),
    // 점 · 기호
    ('\u{2022}', "\u{00B7}"),
    ('\u{2219}', "\u{00B7}"),
    ('\u{00B5}', "\u{03BC}"),
    ('\u{00C5}', "\u{212B}"),
    ('\u{2266}', "\u{2264}"),
    ('\u{2267}', "\u{2265}"),
    ('\u{301C}', "\u{FF5E}"),
    ('\u{00A9}', "(c)"),
    ('\u{00A2}', "\u{FFE0}"),
    ('\u{00A3}', "\u{FFE1}"),
    ('\u{00A5}', "\u{FFE5}"),
    ('\u{0110}', "D"),
    ('\u{017F}', "s"),
    // 공백류 → 보통 공백
    ('\u{00A0}', " "),
    ('\u{2002}', " "),
    ('\u{2003}', " "),
    ('\u{2004}', " "),
    ('\u{2005}', " "),
    ('\u{2006}', " "),
    ('\u{2007}', " "),
    ('\u{2008}', " "),
    ('\u{2009}', " "),
    ('\u{200A}', " "),
    ('\u{202F}', " "),
    // 보이지 않는 글자 → 지움
    ('\u{200B}', ""),
    ('\u{200C}', ""),
    ('\u{200D}', ""),
    ('\u{2060}', ""),
    ('\u{FEFF}', ""),
    // 합자
    ('\u{FB00}', "ff"),
    ('\u{FB01}', "fi"),
    ('\u{FB02}', "fl"),
    ('\u{FB03}', "ffi"),
    ('\u{FB04}', "ffl"),
];

/// 악센트 라틴(Latin-1 · Latin Extended-A 중 CP949 에 없는 것) → 바탕 글자. 두 줄이 한 글자씩 짝이다.
const ACCENTED: &str = "ÀÁÂÃÄÇÈÉÊËÌÍÎÏÑÒÓÔÕÖÙÚÛÜÝàáâãäåçèéêëìíîïñòóôõöùúûüýÿĀāĂăĄąĆćĈĉĊċČčĎďĒēĔĕĖėĘęĚěĜĝĞğĠġĢģĤĥĨĩĪīĬĭĮįİĴĵĶķĹĺĻļĽľŃńŅņŇňŌōŎŏŐőŔŕŖŗŘřŚśŜŝŞşŠšŢţŤťŨũŪūŬŭŮůŰűŲųŴŵŶŷŸŹźŻżŽž";
const ACCENTED_BASE: &str = "AAAAACEEEEIIIINOOOOOUUUUYaaaaaaceeeeiiiinooooouuuuyyAaAaAaCcCcCcCcDdEeEeEeEeEeGgGgGgGgHhIiIiIiIiIJjKkLlLlLlNnNnNnOoOoOoRrRrRrSsSsSsSsTtTtUuUuUuUuUuUuWwYyYZzZzZz";

fn encodable(ch: char) -> bool {
    let mut buffer = [0_u8; 4];
    !EUC_KR.encode(ch.encode_utf8(&mut buffer)).2
}

/// 대응 글자. `None` = 대응표에 없다.
fn substitute(ch: char) -> Option<String> {
    if let Some((_, to)) = TABLE.iter().find(|(from, _)| *from == ch) {
        return Some((*to).to_string());
    }
    if let Some(at) = ACCENTED.chars().position(|value| value == ch) {
        return ACCENTED_BASE.chars().nth(at).map(String::from);
    }
    // 결합 부호(풀어쓴 악센트) → 지움.
    ('\u{0300}'..='\u{036F}').contains(&ch).then(String::new)
}

/// 바꾼 글 · 바꾼 글자들 · 대응표에 없는 글자들.
pub fn cp949_safe_text(text: &str) -> (String, Vec<(char, String)>, Vec<char>) {
    let composed = crate::export_fin::compose_hangul(text);
    let (mut out, mut changed, mut unknown) = (String::new(), Vec::new(), Vec::new());
    for ch in composed.chars() {
        if ch == IMAGE_MARK || encodable(ch) {
            out.push(ch);
        } else if let Some(to) = substitute(ch) {
            out.push_str(&to);
            changed.push((ch, to));
        } else {
            out.push(ch);
            unknown.push(ch);
        }
    }
    (out, changed, unknown)
}

/// 의미 모델의 글(문단 · 표제 · 칸 · 절 제목)을 모두 CP949 로 쓸 수 있게 바꾼다.
/// 문단 글과 런(`inlines`)은 같은 규칙으로 바꿔 서식 범위가 어긋나지 않는다.
/// 대응표에 없는 글자가 있으면 `Cp949Invalid`(`details.chars` = 그런 글자 전부)로 거부한다 — 그때 문서는 반쯤 바뀌어 있으니 버려라
/// (두 호출부 모두 고치던 사본이라 버린다).
pub fn make_cp949_safe(document: &mut HmlDocument) -> Result<Vec<Cp949Substitution>, CoreError> {
    let mut counts: BTreeMap<(char, String), usize> = BTreeMap::new();
    let mut unknown = std::collections::BTreeSet::new();
    // 횟수는 문단 · 칸 글에서만 센다(런 · 표제 · 절 제목은 같은 글의 사본이라 두 번 세게 된다).
    let mut fix = |text: &mut String, counted: bool| {
        if text.is_ascii() {
            return;
        }
        let (safe, changed, missing) = cp949_safe_text(text);
        if counted {
            for pair in changed {
                *counts.entry(pair).or_default() += 1;
            }
        }
        for ch in missing {
            unknown.insert(ch);
        }
        *text = safe;
    };
    let fix_inlines = |inlines: &mut Vec<HmlInline>, fix: &mut dyn FnMut(&mut String, bool)| {
        for inline in inlines.iter_mut() {
            if let HmlInline::Text { text, .. } = inline {
                fix(text, false);
            }
        }
        inlines.retain(|inline| !matches!(inline, HmlInline::Text { text, .. } if text.is_empty()));
    };
    for section in &mut document.sections {
        fix(&mut section.title, false);
    }
    for paragraph in &mut document.paragraphs {
        if let Some(marker) = &mut paragraph.marker {
            fix(marker, false);
        }
        fix(&mut paragraph.text, true);
        fix(&mut paragraph.tail_text, true);
        fix_inlines(&mut paragraph.inlines, &mut fix);
    }
    for cell in &mut document.cells {
        fix(&mut cell.text, true);
        fix_inlines(&mut cell.inlines, &mut fix);
    }
    if !unknown.is_empty() {
        let mut error = CoreError::new(ErrorCode::Cp949Invalid, "cp949_unmapped", false);
        let list: Vec<String> = unknown
            .iter()
            .map(|ch| format!("U+{:04X} {ch}", *ch as u32))
            .collect();
        error.details.insert("chars".into(), list.join(", "));
        return Err(error);
    }
    Ok(counts
        .into_iter()
        .map(|((from, to), count)| Cp949Substitution { from, to, count })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_substitute_is_itself_cp949() {
        assert_eq!(ACCENTED.chars().count(), ACCENTED_BASE.chars().count());
        let froms = TABLE.iter().map(|(from, _)| *from).chain(ACCENTED.chars());
        for from in froms {
            assert!(
                !encodable(from),
                "U+{:04X} 은 이미 CP949 다 — 표에서 빼라",
                from as u32
            );
            let to = substitute(from).unwrap();
            assert!(to.chars().all(encodable), "U+{:04X} → {to:?}", from as u32);
        }
    }

    #[test]
    fn replaces_known_and_reports_unknown() {
        let (text, changed, unknown) =
            cp949_safe_text("5 \u{2212} 3 café\u{00A0}x\u{200B}y \u{1F600}");
        assert_eq!(text, "5 - 3 cafe xy \u{1F600}");
        assert_eq!(changed.len(), 4);
        assert_eq!(unknown, ['\u{1F600}']);
        // CP949 에 없는 첨자(⁵ ₉ ⁻)는 보통 글자로 바꾸면 첨자 뜻을 잃으므로 모르는 글자로 멈춘다.
        assert_eq!(
            cp949_safe_text("x\u{2075}y\u{2089}\u{207B}").2,
            ['\u{2075}', '\u{2089}', '\u{207B}']
        );
        // CP949 에 있는 첨자(¹ ₂)는 그대로.
        assert_eq!(cp949_safe_text("\u{00B9}\u{2082}").0, "\u{00B9}\u{2082}");
        // 풀어쓴 한글 자모는 음절로 모은다(바꾼 것으로 세지 않는다).
        assert_eq!(cp949_safe_text("\u{1100}\u{1161}").0, "가");
    }
}
