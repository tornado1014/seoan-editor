//! 인쇄 줄나눔 — 통특기는 인쇄할 때 프린터 글꼴 폭(600dpi 정수)으로 저장 줄을 다시 잰다.
//! 들어가면 그대로, 넘치면 넘친 부분을 다음 저장 줄 앞으로 민다. 아래첨자가 많은 줄이 화면 폭과 어긋나 넘친다.
//!
//! 글자 폭(px600): 한글·전각 100 · ASCII 50 · 공백 50 · 첨자 ASCII 31·한글 62 ·
//! 탭 = 300 격자 · 그림 = 표시 폭 pt × 600/72 · 본문 폭 3,896.

/// 본문 폭(px600).
pub(crate) const WIDTH: f64 = 3896.0;
const FULL: f64 = 100.0;
const HALF: f64 = 50.0;
const SUB: f64 = 31.0;
const TAB: f64 = 300.0;
/// 첫 줄 들여쓰기(twip) → px600.
pub(crate) const TWIP: f64 = 600.0 / 1440.0;
/// 그림 표시 폭(pt) → px600.
pub(crate) const PT: f64 = 600.0 / 72.0;

const CLOSE: &[u8] = b",.)]}:;!?%'\"";
const OPEN: &[u8] = b"([{";
/// 토큰 안에서 끊을 수 있는 자리(그 글자 뒤). 인쇄는 하이픈·쉼표, 저장 줄나눔은 하이픈만
/// (쉼표를 빼면 인쇄와 어긋나는 문단이 크게 는다).
const SUBTOKEN_END: &[u8] = b"-,";
const STORED_SUBTOKEN_END: &[u8] = b"-";

/// 인쇄 줄을 이루는 한 칸.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Item {
    /// 1바이트 글자(제어 문자 포함) · 첨자인가.
    Ascii(u8, bool),
    /// 2바이트 글자 · 첨자인가.
    Wide(bool),
    /// 그림 · 폭(px600).
    Image(f64),
}

impl Item {
    fn byte(self) -> Option<u8> {
        match self {
            Item::Ascii(byte, _) => Some(byte),
            _ => None,
        }
    }

    fn is_blank(self) -> bool {
        matches!(self.byte(), Some(b' ' | b'\t'))
    }

    fn is_ascii(self) -> bool {
        matches!(self, Item::Ascii(byte, _) if byte != b' ')
    }

    fn is_wide(self) -> bool {
        matches!(self, Item::Wide(_))
    }

    fn is_open(self) -> bool {
        self.byte().is_some_and(|byte| OPEN.contains(&byte))
    }

    /// 제어 문자가 아니다(그림·2바이트·0x20 이상).
    pub(crate) fn is_visible(self) -> bool {
        !matches!(self, Item::Ascii(byte, _) if byte < 0x20)
    }

    /// 잉크가 있다(공백·탭·제어 문자 아님).
    pub(crate) fn has_ink(self) -> bool {
        self.is_visible() && !self.is_blank()
    }

    pub(crate) fn width(self) -> f64 {
        match self {
            Item::Image(width) => width,
            Item::Ascii(byte, _) if byte < 0x20 && byte != b'\t' => 0.0,
            Item::Ascii(_, sub) => {
                if sub {
                    SUB
                } else {
                    HALF
                }
            }
            Item::Wide(sub) => {
                if sub {
                    2.0 * SUB
                } else {
                    FULL
                }
            }
        }
    }
}

/// `items[b]` 를 품는 ASCII 런의 시작(토큰 안 하이픈·쉼표 뒤에서는 끊긴다).
fn run_start(items: &[Item], mut b: usize, start: usize, subtoken: &[u8]) -> usize {
    while b > start && items[b - 1].is_ascii() {
        let c = items[b - 1].byte().unwrap();
        if subtoken.contains(&c)
            && b >= 2
            && items[b - 2].byte() != Some(b' ')
            && !(c == b'-' && items[b - 2].byte() == Some(b' '))
        {
            break;
        }
        b -= 1;
    }
    b
}

pub(crate) fn next_tab(width: f64) -> f64 {
    ((width / TAB).floor() + 1.0) * TAB
}

/// `items[a..b]` 의 자연폭(끝 공백 제외).
fn line_width(items: &[Item], a: usize, b: usize, indent: f64) -> f64 {
    let (mut width, mut last) = (indent, indent);
    for item in &items[a..b] {
        if item.byte() == Some(b'\t') {
            width = next_tab(width);
            continue;
        }
        width += item.width();
        if item.byte() != Some(b' ') {
            last = width;
        }
    }
    last
}

/// (끊을 자리, 폭 `limit` 에 처음 안 들어가는 칸). 다 들어가면 `None`.
fn cut(
    items: &[Item],
    start: usize,
    indent: f64,
    limit: f64,
    subtoken: &[u8],
) -> Option<(usize, usize)> {
    let mut width = indent;
    let mut overflow = None;
    for (i, item) in items.iter().enumerate().skip(start) {
        if item.byte() == Some(b'\t') {
            width = next_tab(width);
            continue;
        }
        let w = item.width();
        if item.byte() != Some(b' ') && width + w > limit && i > start {
            overflow = Some(i);
            break;
        }
        width += w;
    }
    let f = overflow?;
    let item = items[f];
    let mut b = f;
    if item.byte() == Some(b'%')
        && f >= 2
        && items[f - 1].byte() == Some(b' ')
        && items[f - 2].is_ascii()
    {
        b = run_start(items, f - 2, start, subtoken);
    } else if item.is_ascii() {
        b = run_start(items, f, start, subtoken);
        let close = item.byte().is_some_and(|byte| CLOSE.contains(&byte));
        if close
            && b == f
            && f > start + 1
            && (items[f - 1].is_wide() || matches!(items[f - 1], Item::Image(_)))
        {
            b = f - 1;
        } else if items[b..=f].iter().any(|item| item.is_open())
            && b > start
            && items[b - 1].is_wide()
        {
            b -= 1;
        }
    } else if (item.is_wide() || matches!(item, Item::Image(_)))
        && f > start + 1
        && items[f - 1].is_open()
    {
        b = run_start(items, f - 1, start, subtoken);
    }
    while b > start + 1 && items[b - 1].is_open() {
        b = run_start(items, b - 1, start, subtoken);
    }
    if b <= start {
        b = if f > start { f } else { start + 1 };
    }
    Some((b, f))
}

/// 저장 줄 `items[pos..end]` 가 들어갈 때 다음 저장 줄을 끌어올려 끊는 자리. 넘침 자리가 조정 없는 자연 끊김(한글)일 때만.
fn pull_to(items: &[Item], pos: usize, end: usize, indent: f64) -> Option<usize> {
    if end >= items.len() || end <= pos {
        return None;
    }
    let (b, f) = cut(items, pos, indent, WIDTH, SUBTOKEN_END)?;
    (b > end && b == f && items[f].is_wide()).then_some(b)
}

fn skip_blanks(items: &[Item], mut pos: usize, limit: usize) -> usize {
    while pos < limit && items[pos].is_blank() {
        pos += 1;
    }
    pos
}

/// 인쇄 줄 = [(시작, 끝)]. `bounds` = 저장 줄 끝 자리들.
pub(crate) fn reflow(items: &[Item], bounds: &[usize], indent: f64) -> Vec<(usize, usize)> {
    let n = items.len();
    let mut lines: Vec<(usize, usize)> = Vec::new();
    let (mut pos, mut pulled) = (0_usize, false);
    for &end in bounds {
        let end = end.min(n);
        if end < pos || (end == pos && pulled) {
            continue;
        }
        pulled = false;
        let ind = if lines.is_empty() { indent } else { 0.0 };
        if end == pos || line_width(items, pos, end, ind) <= WIDTH {
            match pull_to(items, pos, end, ind) {
                None => {
                    lines.push((pos, end));
                    pos = end;
                }
                Some(b) => {
                    lines.push((pos, b));
                    pos = skip_blanks(items, b, n);
                    pulled = true;
                }
            }
        } else {
            match cut(&items[..end], pos, ind, WIDTH, SUBTOKEN_END) {
                None => {
                    lines.push((pos, end));
                    pos = end;
                }
                Some((b, _)) => {
                    lines.push((pos, b));
                    pos = skip_blanks(items, b, end);
                }
            }
        }
    }
    while pos < n && items[pos..].iter().any(|item| item.has_ink()) {
        let ind = if lines.is_empty() { indent } else { 0.0 };
        match cut(items, pos, ind, WIDTH, SUBTOKEN_END) {
            None => {
                lines.push((pos, n));
                break;
            }
            Some((b, _)) => {
                lines.push((pos, b));
                pos = skip_blanks(items, b, n);
            }
        }
    }
    lines
}

/// 저장 줄나눔 — 통특기가 편집기 폭으로 문단을 처음부터 다시 흘리는 자리(서식 열기 · P3-2). 끊기 규칙은 인쇄와 같고
/// 폭만 다르다(`limit` · `indent` 는 이 칸들의 단위로). 줄 끝 자리들 — 공백은 넘침으로 치지 않으니 앞 줄 끝에 남는다.
pub(crate) fn wrap(items: &[Item], indent: f64, limit: f64) -> Vec<usize> {
    let (mut ends, mut pos, mut indent) = (Vec::new(), 0, indent);
    while let Some((b, _)) = cut(items, pos, indent, limit, STORED_SUBTOKEN_END) {
        pos = b;
        ends.push(pos);
        indent = 0.0;
    }
    ends.push(items.len());
    ends
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wide(n: usize) -> Vec<Item> {
        vec![Item::Wide(false); n]
    }

    #[test]
    fn keeps_fitting_lines_and_pushes_overflow_forward() {
        // 한글 38자 = 3,800 ≤ 3,896 → 저장 줄 그대로.
        assert_eq!(reflow(&wide(38), &[38], 0.0), vec![(0, 38)]);
        // 40자 한 줄 → 38 에서 넘쳐 둘로.
        assert_eq!(reflow(&wide(40), &[40], 0.0), vec![(0, 38), (38, 40)]);
        // 첨자 한글 62 → 62자까지 들어간다(3,844).
        assert_eq!(
            reflow(&vec![Item::Wide(true); 62], &[62], 0.0),
            vec![(0, 62)]
        );
    }

    #[test]
    fn pulls_up_only_at_a_natural_hangul_break() {
        // 저장 줄 10자 + 다음 저장 줄 40자: 첫 줄에 여유가 있고 넘침이 한글 → 끌어올림(38에서 끊김).
        let items = wide(50);
        assert_eq!(reflow(&items, &[10, 50], 0.0), vec![(0, 38), (38, 50)]);
        // 넘침 자리가 ASCII 토큰이면 끌어올리지 않는다.
        let mut items = wide(10);
        items.extend(vec![Item::Ascii(b'a', false); 80]);
        assert_eq!(reflow(&items, &[10, 90], 0.0)[0], (0, 10));
    }

    #[test]
    fn wraps_hangul_per_character_and_hangs_spaces() {
        // 한글 3자 + 공백 2 + 한글 3자, 폭 = 한글 4자: 공백은 앞 줄 끝에 매달고 다음 줄은 글자에서 시작한다.
        let mut items = wide(3);
        items.extend([Item::Ascii(b' ', false); 2]);
        items.extend(wide(3));
        assert_eq!(wrap(&items, 0.0, 400.0), vec![5, 8]);
        // 5자 낱말도 글자 단위로 끊긴다.
        assert_eq!(wrap(&wide(5), 0.0, 400.0), vec![4, 5]);
    }
}
