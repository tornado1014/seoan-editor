//! 원문 PDF → 문단 글(서안 「원문 나란히」 문장 짝짓기의 재료).
//!
//! 글자마다 자리(쪽 위 x · y · 크기)를 받아 줄 → 문단으로 다시 짠다. 규칙:
//! - 줄: 같은 쪽에서 기준선 y 가 글자 크기의 절반 안이면 한 줄 · 줄 안은 x 순 · 낱말 사이 틈 > 0.1em 이면 공백.
//! - 덩어리: 한 줄 안에서 틈이 3em 넘으면 덩어리를 나눈다(여백의 줄 번호를 본문과 떼어 놓기 위해).
//! - 줄 번호: 본문 칸(긴 덩어리의 왼 · 오른 끝) 바깥에 있는 3자리 이하 숫자만의 덩어리.
//! - 머리말 · 꼬리말: 쪽 위 · 아래 10% 띠 안의 줄 가운데, 쪽의 과반에서 같은 자리(3pt) · 같은 글(숫자만 다름)이거나
//!   쪽 번호 꼴(`3` · `- 3 -` · `Page 3 of 9`)인 것.
//! - 문단 나눔: `[0001]` · `1.` 꼴로 시작 · 줄 사이가 보통의 1.4배 넘게 벌어짐 · 첫 줄 들여쓰기 · 앞 줄이 짧게 끝남.
//!   쪽을 넘는 문단은 하나로 잇는다. 줄 끝 하이픈은 잇지 않고 공백 하나로 잇는다.
//!
//! ponytail: 2단 조판은 따로 다루지 않는다(y 순으로 섞인다) — 필요하면 틈 기둥 감지를 얹는다.

use std::{
    collections::HashMap,
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
    sync::LazyLock,
};

use pdf_extract::{MediaBox, OutputDev, OutputError, Transform};
use regex::Regex;

use crate::{
    CoreError, ErrorCode,
    error::{io_error, safe_source},
};

/// 이 크기를 넘는 PDF 는 열지 않는다.
const MAX_BYTES: u64 = 200 * 1024 * 1024;
/// 이 쪽 수를 넘는 PDF 는 읽지 않는다.
const MAX_PAGES: usize = 5_000;
/// 머리말 · 꼬리말 띠(쪽 높이 비율).
const BAND: f64 = 0.10;

static PAGE_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^[-–—\s]*(page\s*)?\d{1,4}(\s*(/|of)\s*\d{1,4})?[-–—\s]*$").unwrap()
});
static PARAGRAPH_LABEL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\[\d{4}\]|\d{1,3}\.\s)").unwrap());

struct Glyph {
    x: f64,
    y: f64,
    end: f64,
    size: f64,
    text: String,
}

#[derive(Default)]
struct Collector {
    pages: Vec<(f64, Vec<Glyph>)>,
    flip: Option<Transform>,
}

impl OutputDev for Collector {
    fn begin_page(
        &mut self,
        _page_num: u32,
        media_box: &MediaBox,
        _art_box: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), OutputError> {
        let height = media_box.ury - media_box.lly;
        self.flip = Some(Transform::row_major(1., 0., 0., -1., 0., height));
        self.pages.push((height, Vec::new()));
        Ok(())
    }
    fn end_page(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
    fn output_character(
        &mut self,
        trm: &Transform,
        width: f64,
        _spacing: f64,
        font_size: f64,
        char: &str,
    ) -> Result<(), OutputError> {
        let (Some(flip), Some((_, glyphs))) = (self.flip, self.pages.last_mut()) else {
            return Ok(());
        };
        let position = trm.post_transform(&flip);
        // 글자 크기 벡터 (fs, fs) 를 변환한 두 성분의 기하평균(pdf-extract 의 PlainTextOutput 과 같다).
        let scaled_x = font_size * (trm.m11 + trm.m21);
        let scaled_y = font_size * (trm.m12 + trm.m22);
        let size = (scaled_x * scaled_y).abs().sqrt();
        glyphs.push(Glyph {
            x: position.m31,
            y: position.m32,
            end: position.m31 + width * size,
            size,
            text: char.to_string(),
        });
        Ok(())
    }
    fn begin_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
    fn end_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
    fn end_line(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
}

fn error(code: ErrorCode, phase: &str, message: &str, source: &Path) -> CoreError {
    let mut error = CoreError::new(code, phase, false);
    error.message = message.to_string();
    error.source = safe_source(&source.to_string_lossy());
    error
}

fn unreadable(path: &Path) -> CoreError {
    error(
        ErrorCode::UnsupportedFormat,
        "pdf_text",
        "PDF 를 읽을 수 없습니다(손상되었거나 PDF 가 아닌 파일입니다).",
        path,
    )
}

/// 쪽마다 글자 자리를 모은다. 라이브러리 안의 패닉은 오류로 바꾼다.
fn collect(path: &Path, bytes: &[u8]) -> Result<Vec<(f64, Vec<Glyph>)>, CoreError> {
    let run = catch_unwind(AssertUnwindSafe(|| {
        let mut document = lopdf::Document::load_mem(bytes).map_err(|_| unreadable(path))?;
        if document.is_encrypted() && document.decrypt("").is_err() {
            return Err(error(
                ErrorCode::UnsupportedFormat,
                "pdf_text_encrypted",
                "암호가 걸린 PDF 입니다.",
                path,
            ));
        }
        if document.get_pages().len() > MAX_PAGES {
            return Err(error(
                ErrorCode::ResourceTooLarge,
                "pdf_text_pages",
                "쪽 수가 너무 많은 PDF 입니다(5,000쪽 초과).",
                path,
            ));
        }
        let mut collector = Collector::default();
        pdf_extract::output_doc(&document, &mut collector).map_err(|_| unreadable(path))?;
        Ok(collector.pages)
    }));
    run.unwrap_or_else(|_| Err(unreadable(path)))
}

struct Line {
    y: f64,
    x0: f64,
    x1: f64,
    size: f64,
    text: String,
}

fn is_digits(text: &str) -> bool {
    (1..=3).contains(&text.chars().count()) && text.chars().all(|c| c.is_ascii_digit())
}

struct Segment {
    x0: f64,
    x1: f64,
    text: String,
}

/// 한 쪽의 글자 → 줄(위에서 아래). 여백의 줄 번호는 여기서 뗀다.
fn page_lines(glyphs: &[Glyph]) -> Vec<Line> {
    let mut rows: Vec<Vec<&Glyph>> = Vec::new();
    for glyph in glyphs {
        let row = rows
            .iter_mut()
            .rev()
            .take(8)
            .find(|row| (row[0].y - glyph.y).abs() < 0.5 * glyph.size.max(row[0].size));
        match row {
            Some(row) => row.push(glyph),
            None => rows.push(vec![glyph]),
        }
    }
    // 덩어리 = 한 줄 안에서 3em 넘는 틈으로 갈린 글.
    let mut lines: Vec<(f64, f64, Vec<Segment>)> = Vec::new();
    for row in rows {
        // 글자는 스트림 차례를 지킨다(같은 글줄 안에서 x 가 1 안팎으로 어긋나는 PDF 가 있다) — 오른쪽으로 크게 뛰거나
        // 왼쪽으로 되돌아가면 덩어리를 나누고, 덩어리만 x 순으로 놓는다.
        let y = row[0].y;
        let size = row.iter().map(|g| g.size).fold(0.0, f64::max);
        let mut segments: Vec<Segment> = Vec::new();
        let mut last_end = f64::NEG_INFINITY;
        for glyph in row {
            let gap = glyph.x - last_end;
            match segments.last_mut() {
                Some(segment) if (-glyph.size..=3.0 * glyph.size).contains(&gap) => {
                    if gap > 0.1 * glyph.size {
                        segment.text.push(' ');
                    }
                    segment.text.push_str(&glyph.text);
                    segment.x1 = glyph.end;
                }
                _ => segments.push(Segment {
                    x0: glyph.x,
                    x1: glyph.end,
                    text: glyph.text.clone(),
                }),
            }
            last_end = glyph.end;
        }
        segments.sort_by(|a, b| a.x0.total_cmp(&b.x0));
        lines.push((y, size, segments));
    }
    // 본문 칸 = 15자 넘는 덩어리의 왼 · 오른 끝. 그 바깥의 짧은 숫자 덩어리가 줄 번호다.
    let long: Vec<&Segment> = lines
        .iter()
        .flat_map(|line| &line.2)
        .filter(|segment| segment.text.trim().chars().count() >= 15)
        .collect();
    let left = long.iter().map(|s| s.x0).fold(f64::INFINITY, f64::min);
    let right = long.iter().map(|s| s.x1).fold(f64::NEG_INFINITY, f64::max);
    let mut out: Vec<Line> = Vec::new();
    for (y, size, segments) in lines {
        let kept: Vec<&Segment> = segments
            .iter()
            .filter(|segment| {
                let outside = segment.x1 < left - 0.5 * size || segment.x0 > right + 0.5 * size;
                !(outside && is_digits(segment.text.trim()))
            })
            .collect();
        let text = kept
            .iter()
            .map(|segment| segment.text.trim())
            .collect::<Vec<_>>()
            .join(" ");
        if text.is_empty() {
            continue;
        }
        out.push(Line {
            y,
            x0: kept.iter().map(|s| s.x0).fold(f64::INFINITY, f64::min),
            x1: kept.iter().map(|s| s.x1).fold(f64::NEG_INFINITY, f64::max),
            size,
            text,
        });
    }
    out.sort_by(|a, b| a.y.total_cmp(&b.y));
    out
}

fn normalized(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_ascii_digit() { '#' } else { c })
        .collect()
}

/// 머리말 · 꼬리말 · 쪽 번호를 걷어 낸다. `pages` = (쪽 높이, 줄들).
fn drop_running_lines(pages: &mut [(f64, Vec<Line>)]) {
    let in_band = |height: f64, y: f64| y < height * BAND || y > height * (1.0 - BAND);
    let mut seen: HashMap<(i64, String), usize> = HashMap::new();
    for (height, lines) in pages.iter() {
        let mut once = std::collections::HashSet::new();
        for line in lines.iter().filter(|line| in_band(*height, line.y)) {
            let key = ((line.y / 3.0).round() as i64, normalized(&line.text));
            if once.insert(key.clone()) {
                *seen.entry(key).or_default() += 1;
            }
        }
    }
    let count = pages.len();
    for (height, lines) in pages.iter_mut() {
        let height = *height;
        lines.retain(|line| {
            if !in_band(height, line.y) {
                return true;
            }
            let key = ((line.y / 3.0).round() as i64, normalized(&line.text));
            let repeated = count >= 2 && seen.get(&key).is_some_and(|n| n * 2 > count);
            !(repeated || PAGE_NUMBER.is_match(line.text.trim()))
        });
    }
}

/// 줄들 → 문단. 쪽 경계를 넘어 이어 붙인다.
fn paragraphs(pages: &[(f64, Vec<Line>)]) -> Vec<String> {
    // 「짧은 줄」 기준 = 15자 넘는 줄 오른끝의 90백분위에서 8em 안쪽.
    let mut rights: Vec<f64> = pages
        .iter()
        .flat_map(|(_, lines)| lines)
        .filter(|line| line.text.chars().count() >= 15)
        .map(|line| line.x1)
        .collect();
    rights.sort_by(f64::total_cmp);
    let right = rights
        .get(rights.len().saturating_sub(1) * 9 / 10)
        .copied()
        .unwrap_or(f64::INFINITY);
    let mut out: Vec<String> = Vec::new();
    let mut previous: Option<&Line> = None;
    for (_, lines) in pages {
        let left = lines
            .iter()
            .filter(|line| line.text.chars().count() >= 15)
            .map(|line| line.x0)
            .fold(f64::INFINITY, f64::min);
        let mut gaps: Vec<f64> = lines
            .windows(2)
            .map(|pair| pair[1].y - pair[0].y)
            .filter(|gap| *gap > 0.0)
            .collect();
        gaps.sort_by(f64::total_cmp);
        let pitch = gaps.get(gaps.len() / 2).copied();
        for (index, line) in lines.iter().enumerate() {
            let new_paragraph = match previous {
                None => true,
                Some(before) => {
                    let spaced = index > 0
                        && pitch.is_some_and(|pitch| line.y - lines[index - 1].y > 1.4 * pitch);
                    let indented = line.x0 > left + 1.2 * line.size;
                    let after_short = before.x1 < right - 8.0 * before.size;
                    PARAGRAPH_LABEL.is_match(&line.text) || spaced || indented || after_short
                }
            };
            match out.last_mut() {
                Some(last) if !new_paragraph => {
                    last.push(' ');
                    last.push_str(&line.text);
                }
                _ => out.push(line.text.clone()),
            }
            previous = Some(line);
        }
    }
    out.iter()
        .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|text| !text.is_empty())
        .collect()
}

/// 원문 PDF 에서 머리말 · 꼬리말 · 쪽 번호 · 줄 번호를 걸러 낸 문단 글을 뽑는다.
pub fn pdf_paragraphs(path: &Path) -> Result<Vec<String>, CoreError> {
    let size = std::fs::metadata(path)
        .map_err(|e| io_error(&e, "pdf_text_read", &path.to_string_lossy()))?
        .len();
    if size > MAX_BYTES {
        return Err(error(
            ErrorCode::ResourceTooLarge,
            "pdf_text_size",
            "PDF 가 너무 큽니다(200MB 초과).",
            path,
        ));
    }
    let bytes =
        std::fs::read(path).map_err(|e| io_error(&e, "pdf_text_read", &path.to_string_lossy()))?;
    let mut pages: Vec<(f64, Vec<Line>)> = collect(path, &bytes)?
        .into_iter()
        .map(|(height, glyphs)| (height, page_lines(&glyphs)))
        .collect();
    drop_running_lines(&mut pages);
    let result = paragraphs(&pages);
    if result.is_empty() {
        return Err(error(
            ErrorCode::UnsupportedFormat,
            "pdf_text_no_text",
            "글을 읽을 수 없는 PDF 입니다(스캔본은 지원하지 않습니다).",
            path,
        ));
    }
    Ok(result)
}
