//! HLT → KIPO XML 의견서(`OpinionDOC`). 문서 종류(헤더 0x20 하위 15비트) 7.
//!
//! 통특기 의견서 제출 XML 과 같은 규칙:
//! - `【의견내용】` 표제 = `<Opinion>` 요소. 그 밖의 문단 하나 = `<P indent=… align=…>` 하나(글자는 줄을 이어 붙인다).
//! - 빈 문단은 앞 `<P>` 끝에 `<BR/>` 로 붙는다.
//! - `indent` = 첫 줄 들여쓰기 twip → mm 반올림(799 → 14) · `align` = 문단모양 정렬 대문자(JUSTIFIED · LEFT · CENTER · RIGHT).
//! - 글자 서식은 대문자 태그(`<B>` · `<U>` · `<I>`), 안쪽부터 U, I, B.
//! - 쪽이 바뀌는 줄 첫 글자 앞에 `<DP n="쪽" type="SOFT"/>` — 쪽 매김은 P3 모델(`paginate::page_layout`). 문단 첫 줄이면 `<P` 앞.

use std::path::Path;

use crate::{
    CharacterFormat, CoreError, ErrorCode, InspectDocument, InspectOptions, KipoExport,
    KipoExportOptions, inspect_format_inventories, inspect_path,
};

/// 쪽 바뀜 자리 표시(문단 글자에 끼워 넣었다가 태그로 바꾼다). 문서 글자에 나올 수 없는 사용자 영역 글자.
const PAGE_MARK: char = '\u{E000}';

pub(crate) fn is_opinion(document: &InspectDocument) -> bool {
    document.summary.document_type_raw & 0x7fff == 7
}

pub(crate) fn export_opinion_xml_path(
    path: &Path,
    options: &KipoExportOptions,
) -> Result<KipoExport, CoreError> {
    let document = inspect_path(
        path,
        InspectOptions {
            include_image_payloads: true,
            ..InspectOptions::default()
        },
    )?;
    let (formats, _) = inspect_format_inventories(path)?;
    let (_, inflated) = crate::inflate_hlt(path)?;
    let (_, _, pages) = crate::page_layout(&document, &inflated);
    let line_index: std::collections::BTreeMap<&str, usize> = document
        .lines
        .iter()
        .map(|line| (line.id.as_str(), line.index as usize))
        .collect();
    let line_chars = |id: &str| -> Result<(usize, usize), CoreError> {
        let line = &document.lines[line_index[id]];
        let raw =
            &inflated[line.source_range.start_byte as usize..line.source_range.end_byte as usize];
        let (text, _, _) = encoding_rs::EUC_KR.decode(raw);
        let chars = text.chars().filter(|c| *c >= ' ' || *c == '\t').count();
        Ok((line.index as usize, chars))
    };

    let mut body = String::new();
    let mut open = false;
    for paragraph in &document.paragraphs {
        // 제어 글자는 버린다(탭 빼고) — 서식 좌표와 같은 글자열. 통특기도 0x0f 같은 떠돌이 제어 글자를 안 싣는다(opinion).
        let text: String = paragraph
            .text
            .chars()
            .filter(|c| *c >= ' ' || *c == '\t')
            .collect();
        let text = text.as_str();
        if text.starts_with('【') && text.ends_with('】') {
            continue;
        }
        // 줄마다 (글자 수, 이 줄에서 쪽이 바뀌면 그 쪽). 끝의 빈 줄은 하나마다 `<BR/>`(빈 줄 수 = BR 수, 3/3).
        let mut lines = Vec::new();
        for id in &paragraph.line_ids {
            let (line, chars) = line_chars(id)?;
            let mark = (line > 0 && pages.get(line) != pages.get(line - 1)).then(|| pages[line]);
            lines.push((chars, mark));
        }
        let text_lines = lines
            .iter()
            .rposition(|(chars, _)| *chars > 0)
            .map_or(0, |at| at + 1);
        let mut marks = Vec::new();
        let mut offset = 0;
        for (chars, mark) in &lines[..text_lines] {
            if let Some(page) = mark {
                marks.push((offset, *page));
            }
            offset += chars;
        }
        if offset != text.chars().count() {
            return Err(opinion_error("line_char_count"));
        }
        // 빈 줄에서 쪽이 바뀌면 통특기는 `</P>` 로 닫고 `<DP/><BR/>` 를 문단 밖에 쓴다(opinion2).
        let mut breaks = String::new();
        let close_before_break = |breaks: &mut String, open: &mut bool, mark: Option<usize>| {
            if let Some(page) = mark {
                if *open {
                    breaks.push_str("</P>\n");
                    *open = false;
                }
                breaks.push_str(&page_tag(page));
            }
            breaks.push_str("<BR/>");
        };
        if text.is_empty() {
            if !open && body.is_empty() {
                return Err(opinion_error("leading_empty_paragraph"));
            }
            for (_, mark) in &lines {
                close_before_break(&mut breaks, &mut open, *mark);
            }
            body.push_str(&breaks);
            continue;
        }
        if open {
            body.push_str("</P>\n");
        }
        let lead = marks
            .first()
            .filter(|(at, _)| *at == 0)
            .map(|(_, page)| *page);
        if let Some(page) = lead {
            body.push_str(&page_tag(page));
        }
        let inner: Vec<(usize, usize)> = marks.into_iter().filter(|(at, _)| *at > 0).collect();
        let spans = formats
            .paragraphs
            .iter()
            .find(|record| record.paragraph_id == paragraph.id)
            .map(|record| record.spans.as_slice())
            .unwrap_or_default();
        let markup = paragraph_markup(text, spans, &inner);
        let indent = (f64::from(paragraph.indent_raw) * 25.4 / 1440.0).round() as u32;
        let align = match paragraph.align.as_str() {
            "left" => "LEFT",
            "center" => "CENTER",
            "right" => "RIGHT",
            _ => "JUSTIFIED",
        };
        body.push_str(&format!(
            "<P indent=\"{indent}\" align=\"{align}\">{markup}"
        ));
        open = true;
        for (_, mark) in &lines[text_lines..] {
            close_before_break(&mut breaks, &mut open, *mark);
        }
        body.push_str(&breaks);
    }
    if open {
        body.push_str("</P>\n");
    }
    let page_count = options
        .page_count
        .map(|value| format!(" pageCount=\"{value}\""))
        .unwrap_or_default();
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<KIPO keapsVersion=\"{}\" editorKind=\"{}\"{page_count} xmlns=\"http://www.kipo.go.kr\">\n<OpinionDOC>\n<Opinion>{body}</Opinion>\n</OpinionDOC>\n</KIPO>",
        options.keaps_version, options.editor_kind
    );
    Ok(KipoExport {
        xml_sha256: crate::sha256_hex(xml.as_bytes()),
        xml,
        claims: Vec::new(),
        paragraph_count: 0,
        claim_count: 0,
        figure_count: 0,
        numbered_by_id: false,
        images: Vec::new(),
        sequence_file: None,
    })
}

fn page_tag(page: usize) -> String {
    format!("<DP n=\"{page}\" type=\"SOFT\"/>")
}

/// 글자에 서식을 입히고 쪽 바뀜 자리(`marks` = (글자 자리, 쪽))에 `<DP/>` 를 넣는다.
fn paragraph_markup(
    text: &str,
    spans: &[crate::CharacterFormatSpan],
    marks: &[(usize, usize)],
) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut cursor = 0;
    let mut pieces: Vec<(usize, usize, &[CharacterFormat])> = Vec::new();
    for span in spans {
        let (start, end) = (span.start as usize, (span.end as usize).min(chars.len()));
        if start >= end || start < cursor {
            continue;
        }
        if start > cursor {
            pieces.push((cursor, start, &[]));
        }
        pieces.push((start, end, &span.formats));
        cursor = end;
    }
    if cursor < chars.len() {
        pieces.push((cursor, chars.len(), &[]));
    }
    let mut pages = marks.iter().peekable();
    for (start, end, formats) in pieces {
        let mut run = String::new();
        for (at, ch) in chars.iter().enumerate().take(end).skip(start) {
            while let Some((_, page)) = pages.next_if(|(mark, _)| *mark == at) {
                run.push(PAGE_MARK);
                run.push_str(&page.to_string());
                run.push(PAGE_MARK);
            }
            run.push(*ch);
        }
        out.push_str(&wrap(formats, &escape(&run)));
    }
    // 쪽 표시를 태그로.
    let mut result = String::new();
    let mut parts = out.split(PAGE_MARK);
    if let Some(first) = parts.next() {
        result.push_str(first);
    }
    while let (Some(page), Some(rest)) = (parts.next(), parts.next()) {
        result.push_str(&page_tag(page.parse().unwrap_or(0)));
        result.push_str(rest);
    }
    result
}

fn wrap(formats: &[CharacterFormat], inner: &str) -> String {
    let rank = |format: &CharacterFormat| match format {
        CharacterFormat::Superscript | CharacterFormat::Subscript => 0,
        CharacterFormat::Underline => 1,
        CharacterFormat::Italic => 2,
        CharacterFormat::Bold => 3,
    };
    let mut ordered: Vec<&CharacterFormat> = formats.iter().collect();
    ordered.sort_by_key(|format| rank(format));
    let mut out = inner.to_string();
    for format in ordered {
        let tag = match format {
            CharacterFormat::Bold => "B",
            CharacterFormat::Italic => "I",
            CharacterFormat::Underline => "U",
            CharacterFormat::Superscript => "SUP",
            CharacterFormat::Subscript => "SUB",
        };
        out = format!("<{tag}>{out}</{tag}>");
    }
    out
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn opinion_error(reason: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::StructureInvalid, "export_opinion", false);
    error.details.insert("reason".into(), reason.into());
    error
}
