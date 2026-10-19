//! KIPO XML 의견서(`OpinionDOC`) → `.HLT` — 의견서 `.hlz` 를 열 때(2026-10-16 사용자 「서안 의견서 hlz 가 서안에서 안 열린다」).
//!
//! `export_opinion` 규칙의 역이다:
//! - `<Opinion>` = 【의견내용】 표제(글자로는 안 실린다) · 그 안 `<P indent=mm align=…>` 하나 = 본문 문단 하나.
//! - `<B>` · `<I>` · `<U>` · `<SUP>` · `<SUB>` = 글자 서식 · `<BR/>` = 빈 줄 하나(= 빈 문단) · `<DP/>` = 쪽 표시(버린다 · 쪽은 다시 흘린다).
//!
//! 굽는 틀은 「새 의견서」(통특기 `Opinion.frm` + 종류 7)와 같다 — 편집 세션의 공개 고침(글 · 서식 · 정렬)으로 채워 저장한다.

use std::path::Path;

use crate::kipo_xml::{XmlContent, XmlNode, parse_xml};
use crate::{CharacterFormat, CoreError, EditorSession, ErrorCode, WriteOptions};

/// 본문 문단 하나. `runs` = (글, 서식) · 빈 줄이면 `runs` 가 비었다.
#[derive(Debug, Default, PartialEq)]
struct OpinionParagraph {
    runs: Vec<(String, Vec<CharacterFormat>)>,
    align: Option<String>,
    indent_mm: Option<u32>,
}

impl OpinionParagraph {
    fn text(&self) -> String {
        self.runs.iter().map(|(text, _)| text.as_str()).collect()
    }
}

/// XML 이 의견서인가(`<OpinionDOC>`).
pub(crate) fn is_opinion_xml(xml: &str) -> bool {
    xml.contains("<OpinionDOC")
}

/// 의견서 XML 을 새 `.HLT` 로 굽는다(`work` = 틀을 둘 빈 폴더).
pub(crate) fn hlt_from_opinion_xml(xml: &str, work: &Path, output: &Path) -> Result<(), CoreError> {
    hlt_from_paragraphs(&parse_opinion(xml)?, work, output)
}

/// 글로 새 의견서: 줄 하나 = 【의견내용】 아래 본문 문단 하나(빈 줄 = 빈 문단 · 서식 없음).
pub fn opinion_hlt_from_text(text: &str, output: &Path) -> Result<(), CoreError> {
    let paragraphs: Vec<OpinionParagraph> = text
        .lines()
        .map(|line| OpinionParagraph {
            runs: if line.is_empty() {
                Vec::new()
            } else {
                vec![(line.to_string(), Vec::new())]
            },
            ..OpinionParagraph::default()
        })
        .collect();
    if paragraphs.is_empty() {
        return crate::save_new_opinion_as_new(output, WriteOptions::default()).map(|_| ());
    }
    let work = tempfile::tempdir().map_err(|_| import_error("work"))?;
    hlt_from_paragraphs(&paragraphs, work.path(), output)
}

fn hlt_from_paragraphs(
    paragraphs: &[OpinionParagraph],
    work: &Path,
    output: &Path,
) -> Result<(), CoreError> {
    let frame = work.join("opinion-frame.HLT");
    crate::save_new_opinion_as_new(&frame, WriteOptions::default())?;
    let mut session = EditorSession::open(&frame)?;
    // 틀 = 【의견내용】 + 빈 문단들. 빈 문단들을 하나로 합친 뒤 본문 전체를 문단 나누기로 넣는다.
    // 판독 끝의 줄 없는 빈 문단(파싱 부산물 · 모델엔 없다)은 고칠 수 없다 — 줄이 있는 마지막 문단까지.
    let template = session.paragraphs();
    let last = session
        .document
        .paragraphs
        .iter()
        .rev()
        .find(|paragraph| !paragraph.line_ids.is_empty())
        .and_then(|record| template.iter().find(|paragraph| paragraph.id == record.id));
    let (Some(first), Some(last)) = (template.get(1), last) else {
        return Err(import_error("frame"));
    };
    let template_indent = session
        .document
        .paragraphs
        .get(1)
        .map(|paragraph| paragraph.indent_raw as i32)
        .unwrap_or(0);
    let (first, last) = (first.id.clone(), last.id.clone());
    if first != last {
        session.delete_range(&first, 0, &last, 0)?;
    }
    let body: Vec<String> = paragraphs.iter().map(OpinionParagraph::text).collect();
    session.replace_text(&first, &body.join("\n"))?;
    let ids: Vec<String> = session.document.paragraphs[1..]
        .iter()
        .filter(|paragraph| !paragraph.line_ids.is_empty())
        .map(|paragraph| paragraph.id.clone())
        .collect();
    if ids.len() != paragraphs.len() {
        return Err(import_error("paragraph_count"));
    }
    // 서식: 종류마다 한 번(되돌리기 · 다시 굽기 수를 줄인다). 처음엔 서식이 없으니 켜기만 한다.
    for format in [
        CharacterFormat::Bold,
        CharacterFormat::Italic,
        CharacterFormat::Underline,
        CharacterFormat::Superscript,
        CharacterFormat::Subscript,
    ] {
        let mut spans = Vec::new();
        for (id, paragraph) in ids.iter().zip(paragraphs) {
            let mut at = 0;
            for (text, formats) in &paragraph.runs {
                let length = text.chars().count();
                if formats.contains(&format) && length > 0 {
                    spans.push((id.clone(), at, at + length));
                }
                at += length;
            }
        }
        if !spans.is_empty() {
            session.toggle_format_spans(&spans, format)?;
        }
    }
    // 정렬 · 들여쓰기: 글이 있는 문단만(빈 줄은 틀 모양 그대로). mm 가 틀 들여쓰기와 같으면 틀 값(799 · 800)을 쓴다.
    let to_mm = |raw: i32| (f64::from(raw) * 25.4 / 1440.0).round() as u32;
    let shapes: Vec<(String, String, i32)> = ids
        .iter()
        .zip(paragraphs)
        .filter(|(_, paragraph)| !paragraph.runs.is_empty())
        .map(|(id, paragraph)| {
            let align = match paragraph.align.as_deref() {
                Some("CENTER") => "center",
                Some("LEFT") => "left",
                // RIGHT 는 생성기 모양표에 없다 — 양쪽으로.
                _ => "justified",
            };
            let indent = match paragraph.indent_mm {
                Some(mm) if mm == to_mm(template_indent) => template_indent,
                Some(mm) => (f64::from(mm) * 1440.0 / 25.4).round() as i32,
                None => template_indent,
            };
            (id.clone(), align.to_string(), indent)
        })
        .collect();
    if !shapes.is_empty() {
        session.set_paragraph_shapes(&shapes)?;
    }
    session.save_as(output)
}

fn parse_opinion(xml: &str) -> Result<Vec<OpinionParagraph>, CoreError> {
    let root = parse_xml(xml, "opinion.xml")?;
    let opinion = find(&root, "Opinion").ok_or_else(|| import_error("no_opinion"))?;
    let mut out = Vec::new();
    for content in &opinion.content {
        match content {
            XmlContent::Element(node) if node.name == "P" => {
                let mut paragraph = OpinionParagraph {
                    align: node.attributes.get("align").cloned(),
                    indent_mm: node
                        .attributes
                        .get("indent")
                        .and_then(|value| value.parse().ok()),
                    ..OpinionParagraph::default()
                };
                let mut breaks = 0;
                collect(node, &mut Vec::new(), &mut paragraph.runs, &mut breaks);
                if paragraph.text().is_empty() {
                    paragraph.runs.clear();
                }
                out.push(paragraph);
                out.extend((0..breaks).map(|_| OpinionParagraph::default()));
            }
            // 문단 밖 빈 줄(쪽이 바뀐 빈 줄 — export_opinion 이 `</P>` 로 닫고 쓴다).
            XmlContent::Element(node) if node.name == "BR" => out.push(OpinionParagraph::default()),
            _ => {}
        }
    }
    Ok(out)
}

/// `node` 안 글을 서식과 함께 모은다. `<BR/>` 은 문단 끝 빈 줄로 센다(통특기는 문단 끝에만 쓴다).
fn collect(
    node: &XmlNode,
    formats: &mut Vec<CharacterFormat>,
    runs: &mut Vec<(String, Vec<CharacterFormat>)>,
    breaks: &mut usize,
) {
    for content in &node.content {
        match content {
            XmlContent::Text(text) => {
                // 태그 사이 줄바꿈(직렬화기)은 글이 아니다.
                let text: String = text
                    .chars()
                    .filter(|ch| *ch != '\n' && *ch != '\r')
                    .collect();
                if text.is_empty() {
                    continue;
                }
                match runs.last_mut() {
                    Some((last, last_formats)) if last_formats == formats => last.push_str(&text),
                    _ => runs.push((text, formats.clone())),
                }
            }
            XmlContent::Element(child) => {
                let format = match child.name.as_str() {
                    "B" => Some(CharacterFormat::Bold),
                    "I" => Some(CharacterFormat::Italic),
                    "U" => Some(CharacterFormat::Underline),
                    "SUP" => Some(CharacterFormat::Superscript),
                    "SUB" => Some(CharacterFormat::Subscript),
                    "BR" => {
                        *breaks += 1;
                        continue;
                    }
                    _ => None,
                };
                if let Some(format) = format {
                    formats.push(format);
                    collect(child, formats, runs, breaks);
                    formats.pop();
                } else {
                    collect(child, formats, runs, breaks);
                }
            }
        }
    }
}

fn find<'a>(node: &'a XmlNode, name: &str) -> Option<&'a XmlNode> {
    if node.name == name {
        return Some(node);
    }
    node.content.iter().find_map(|content| match content {
        XmlContent::Element(child) => find(child, name),
        XmlContent::Text(_) => None,
    })
}

fn import_error(reason: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::StructureInvalid, "import_opinion", false);
    error.details.insert("reason".into(), reason.into());
    error
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_paragraphs_formats_and_breaks() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<KIPO keapsVersion="5.6" editorKind="K" pageCount="1" xmlns="http://www.kipo.go.kr">
<OpinionDOC>
<Opinion><P indent="14" align="JUSTIFIED"><B>Ⅰ. 통지내용</B></P>
<P indent="14" align="CENTER">첫째, <B><U>N:1</U></B> &amp; 끝<DP n="2" type="SOFT"/>.<BR/><BR/></P>
<DP n="3" type="SOFT"/><BR/></Opinion>
</OpinionDOC>
</KIPO>"#;
        let paragraphs = parse_opinion(xml).unwrap();
        assert_eq!(
            paragraphs.len(),
            5,
            "제목 · 본문 · 빈 줄 둘 · 문단 밖 빈 줄"
        );
        assert_eq!(
            paragraphs[0].runs,
            vec![("Ⅰ. 통지내용".into(), vec![CharacterFormat::Bold])]
        );
        assert_eq!(paragraphs[1].text(), "첫째, N:1 & 끝.");
        assert_eq!(paragraphs[1].align.as_deref(), Some("CENTER"));
        assert_eq!(paragraphs[1].indent_mm, Some(14));
        assert_eq!(
            paragraphs[1].runs[1],
            (
                "N:1".into(),
                vec![CharacterFormat::Bold, CharacterFormat::Underline]
            )
        );
        assert!(
            paragraphs[2..]
                .iter()
                .all(|paragraph| paragraph.runs.is_empty())
        );
    }
}
