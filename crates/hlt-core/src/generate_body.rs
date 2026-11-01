use encoding_rs::EUC_KR;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::{
    CharacterFormat, CoreError, ErrorCode, GeneratedCellLineRange, GenerationDialect, HmlDocument,
    HmlInline,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedFormatSpan {
    pub start: u32,
    pub end: u32,
    pub formats: Vec<CharacterFormat>,
    /// 원본 런이 명시한 글자 크기(포인트). 없으면 방언 기본값이다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u32>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneratedLineEnding {
    Continuation,
    Paragraph,
    Cell,
    Row,
    PageBreak,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedLine {
    pub paragraph_id: String,
    pub cell_id: Option<String>,
    pub text: String,
    pub shape_index: u32,
    pub wipo_id: Option<u32>,
    pub formats: Vec<GeneratedFormatSpan>,
    pub ending: GeneratedLineEnding,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedImageAnchor {
    pub image_id: String,
    pub line_index: u32,
    pub character_index: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HltBodyPlan {
    pub section_table: Vec<u8>,
    pub body: Vec<u8>,
    pub lines: Vec<GeneratedLine>,
    pub images: Vec<GeneratedImageAnchor>,
    pub cell_ranges: Vec<GeneratedCellLineRange>,
}

#[derive(Clone)]
struct LogicalChar {
    ch: char,
    formats: Vec<CharacterFormat>,
    size: Option<u32>,
    image_id: Option<String>,
}

pub fn plan_hlt_body(
    document: &HmlDocument,
    dialect: GenerationDialect,
) -> Result<HltBodyPlan, CoreError> {
    let section_table = section_table(document)?;
    let mut body = Vec::new();
    let mut lines = Vec::new();
    let mut images = Vec::new();
    let mut cell_ranges = Vec::new();
    let mut wipo = 0_u32;
    // 절 표제 문단(앵커)은 들여쓰기하지 않는다. 통특기 저장본 4건(pct_a, pct, opinion, pct_b)의 앵커 305개가
    // 전부 들여쓰기 0(가운데 정렬 포함)이고, 유일한 예외였던 domestic 정본(40개 전부 800)은
    // 사용자가 결함으로 판정했다(2026-09-06). 절 테이블에 레코드가 없는 표제(【표 1】, 【도 12】)는
    // 본문 문단이라 원래 들여쓰기를 지킨다(【표 2】 인쇄 89.8pt).
    let anchors: BTreeSet<&str> = document
        .sections
        .iter()
        .filter_map(|section| section.paragraph_ids.first().map(String::as_str))
        .collect();
    for paragraph in &document.paragraphs {
        if let Some(table_id) = &paragraph.table_id {
            emit_table_body(
                document,
                table_id,
                paragraph,
                dialect,
                &mut body,
                &mut lines,
                &mut images,
                &mut cell_ranges,
                &mut wipo,
            )?;
            continue;
        }
        let marker = paragraph.marker.is_some();
        let logical = logical_chars(paragraph)?;
        let visible: String = logical
            .iter()
            .filter(|c| c.image_id.is_none())
            .map(|c| c.ch)
            .collect();
        if visible != paragraph.text {
            return Err(structure_error("paragraph_text_mismatch"));
        }
        // 앵커는 왼쪽 정렬 + 들여쓰기 0. 소수(【발명의 설명】, 【청구범위】, 【요약서】, 【도면】)만 가운데 정렬 + 들여쓰기 0.
        // ⛔ 원본 HLT 에서 그대로 온 표제(원본 줄나눔을 안다)는 그 파일의 모양을 지킨다 — 국문 견본은 통특기 인쇄도
        // 【청구항 N】 · 【도 N】 을 본문 첫 줄만큼(파일 값 800) 들여 찍는다. 이 규칙은 모양을 모르는 입력(KIPO XML · docx)에만.
        // (전엔 문단 하나만 고쳐도 다시 구운 저장본의 표제가 전부 0 이 됐다 — 화면에선 「시간이 지나면 들여쓰기가 사라진다」.)
        // 가운데 정렬 식별항목은 원본에서 왔어도 0 — 통특기 가운데 정렬은 들여쓰기 뒤 남는 폭의 반을 밀어, 파일 값 800 이면
        // 정가운데보다 20pt 오른쪽에 찍는다(국문 견본 【발명의 설명】 · 【청구범위】 · 【요약서】 · 사용자 결정 10-15).
        let is_anchor = paragraph.marker.is_some()
            && anchors.contains(paragraph.id.as_str())
            && (paragraph.source_line_bytes.is_empty() || paragraph.align == "center");
        let (indent, align) = if is_anchor {
            (
                0,
                if paragraph.align == "center" {
                    "center"
                } else {
                    "left"
                },
            )
        } else {
            (paragraph.indent_raw, paragraph.align.as_str())
        };
        let shape = shape_index(dialect, indent, align, paragraph.lspace);
        // 표제 이름으로 쪽을 나누는 규칙은 **원본 줄나눔을 모르는 입력에만** 쓴다.
        // ⛔ HLT 를 되굽는 왕복에서 이 규칙이 없던 페이지브레이크 줄을 만들었다
        // (국내 명세서: 줄 866 이 868 로 늘었다. 그 파일에는 `0x0c` 가 하나도 없다).
        // `source_line_bytes` 가 있으면 원본이 그 문단을 몇 줄로 흘렸는지 아는 것이므로
        // 짐작하지 않는다.
        // 서식마다 쪽을 나누는 자리가 다르다(통특기 저장본 본문 `0x0c` 전수):
        //   PCT(113): 【청구범위】·【요약서】·【도면】 앞 (3곳, 도면마다는 안 나눈다)
        //   국내(140): 【도면】 앞 하나뿐이 구조다. 청구범위·요약서 앞은 없다.
        //     도면 사이의 `0x0c` 7개는 **배치 결과**다(【도 2】 앞엔 있고 【도 3】 앞엔 없다 — 그림이 남은
        //     자리에 들어가면 안 나눈다). 우리는 넣지 않고 통특기 흐름에 맡긴다(인쇄 쪽별 글자 동일).
        let text = paragraph.text.as_str();
        //   ⇒ 사용자 결정(2026-10-16): 【청구범위】·【요약서】·【도면】은 서식 · 입력과 상관없이 **늘 새 쪽**.
        //     국내 정본과 쪽 수가 갈리는 것(51 → 늘어남)을 알고 고른 것이다. 원본 HLT 에 이미 있던
        //     쪽나눔은 `page_break` 로 읽히므로 겹쳐 두 번 나누지 않는다.
        let synthetic_break = marker && matches!(text, "【청구범위】" | "【요약서】" | "【도면】");
        let needs_break = paragraph.page_break || synthetic_break;
        if needs_break {
            body.extend_from_slice(b"\x0c\r\n");
            lines.push(GeneratedLine {
                paragraph_id: paragraph.id.clone(),
                cell_id: None,
                text: "\x0c".into(),
                shape_index: shape,
                wipo_id: None,
                formats: vec![],
                ending: GeneratedLineEnding::PageBreak,
            });
        }
        let paragraph_wipo = if marker {
            None
        } else {
            wipo = wipo
                .checked_add(1)
                .ok_or_else(|| structure_error("wipo_overflow"))?;
            Some(wipo)
        };
        // 원본 줄나눔이 없는 문단(편집기에서 고친 문단)은 통특기 저장 줄나눔(R15 · `reflow::wrap`)으로 흘린다.
        let targets = if paragraph.source_line_bytes.is_empty() {
            stored_wrap_targets(&logical, paragraph).unwrap_or_default()
        } else {
            paragraph.source_line_bytes.clone()
        };
        let wrapped = wrap(logical, &targets);
        for (local, line_chars) in wrapped.iter().enumerate() {
            if local > 0 {
                body.extend_from_slice(b"\r\n");
            }
            let line_index =
                u32::try_from(lines.len()).map_err(|_| structure_error("line_overflow"))?;
            let mut text = String::new();
            for (index, item) in line_chars.iter().enumerate() {
                text.push(item.ch);
                encode_char(item.ch, &mut body)?;
                if let Some(image_id) = &item.image_id {
                    images.push(GeneratedImageAnchor {
                        image_id: image_id.clone(),
                        line_index,
                        character_index: index as u32,
                    });
                }
            }
            lines.push(GeneratedLine {
                paragraph_id: paragraph.id.clone(),
                cell_id: None,
                text,
                shape_index: shape,
                wipo_id: paragraph_wipo,
                formats: format_spans(line_chars),
                ending: if local + 1 == wrapped.len() {
                    GeneratedLineEnding::Paragraph
                } else {
                    GeneratedLineEnding::Continuation
                },
            });
        }
        body.extend_from_slice(b"\x15\r\n");
    }
    Ok(HltBodyPlan {
        section_table,
        body,
        lines,
        images,
        cell_ranges,
    })
}

#[allow(clippy::too_many_arguments)]
fn emit_table_body(
    document: &HmlDocument,
    table_id: &str,
    paragraph: &crate::HmlParagraph,
    dialect: GenerationDialect,
    body: &mut Vec<u8>,
    lines: &mut Vec<GeneratedLine>,
    images: &mut Vec<GeneratedImageAnchor>,
    cell_ranges: &mut Vec<GeneratedCellLineRange>,
    wipo: &mut u32,
) -> Result<(), CoreError> {
    let table = document
        .tables
        .iter()
        .find(|t| t.id == table_id)
        .ok_or_else(|| structure_error("missing_table"))?;
    let cell_map: BTreeMap<_, _> = document.cells.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut rows: BTreeMap<u32, Vec<_>> = BTreeMap::new();
    for id in &table.cell_ids {
        let cell = *cell_map
            .get(id.as_str())
            .ok_or_else(|| structure_error("missing_cell"))?;
        rows.entry(cell.row).or_default().push(cell);
    }
    for cells in rows.values() {
        let mut last = None;
        for cell in cells {
            let logical = logical_inlines(&cell.inlines)?;
            let visible: String = logical
                .iter()
                .filter(|c| c.image_id.is_none())
                .map(|c| c.ch)
                .collect();
            if visible != cell.text {
                return Err(structure_error("cell_text_mismatch"));
            }
            let first_line =
                u32::try_from(lines.len()).map_err(|_| structure_error("line_overflow"))?;
            let shape = table_cell_shape_index(dialect, cell.indent_raw, &cell.align);
            // 칸 글도 통특기 저장 줄나눔(R15 · 칸 폭)으로 흘린다 — 한 줄로 쓰면 긴 칸 글이 칸 밖으로 나간다.
            let segments = cell_wrap(&logical, cell);
            let count = segments.len();
            for (part, segment) in segments.into_iter().enumerate() {
                let line_index =
                    u32::try_from(lines.len()).map_err(|_| structure_error("line_overflow"))?;
                let mut text = String::new();
                for (index, item) in segment.iter().enumerate() {
                    text.push(item.ch);
                    encode_char(item.ch, body)?;
                    if let Some(image_id) = &item.image_id {
                        images.push(GeneratedImageAnchor {
                            image_id: image_id.clone(),
                            line_index,
                            character_index: index as u32,
                        });
                    }
                }
                let last_part = part + 1 == count;
                body.extend_from_slice(if last_part { b"\x13\r\n" } else { b"\r\n" });
                lines.push(GeneratedLine {
                    paragraph_id: paragraph.id.clone(),
                    cell_id: Some(cell.id.clone()),
                    text,
                    shape_index: shape,
                    wipo_id: None,
                    formats: format_spans(&segment),
                    ending: if last_part {
                        GeneratedLineEnding::Cell
                    } else {
                        GeneratedLineEnding::Continuation
                    },
                });
            }
            let last_line =
                u32::try_from(lines.len() - 1).map_err(|_| structure_error("line_overflow"))?;
            cell_ranges.push(GeneratedCellLineRange {
                cell_id: cell.id.clone(),
                line_start: first_line,
                line_end: last_line,
            });
            last = Some(cell.id.clone());
        }
        body.extend_from_slice(b"\x12\r\n");
        lines.push(GeneratedLine {
            paragraph_id: paragraph.id.clone(),
            cell_id: last,
            text: "\x12".into(),
            shape_index: shape_index(dialect, 0, "left", 130),
            wipo_id: None,
            formats: vec![],
            ending: GeneratedLineEnding::Row,
        });
    }
    let logical = logical_chars(paragraph)?;
    let visible: String = logical
        .iter()
        .filter(|c| c.image_id.is_none())
        .map(|c| c.ch)
        .collect();
    if visible != paragraph.text {
        return Err(structure_error("paragraph_text_mismatch"));
    }
    if logical.is_empty() {
        // 빈 표 꼬리 줄은 글자 없이 문단 끝만 — 통특기 저장본 `12 0d 0a 15 0d 0a`(2026-10-16 domestic 표 1 · 0127).
        // 공백을 넣으면 내보내기가 `<p num> </p>` 로 읽어 최종본(`<p num></p>`)과 달라 보정서에 거짓 `p A` 가 생겼다.
        lines.push(GeneratedLine {
            paragraph_id: paragraph.id.clone(),
            cell_id: None,
            text: String::new(),
            shape_index: shape_index(
                dialect,
                paragraph.indent_raw,
                &paragraph.align,
                paragraph.lspace,
            ),
            wipo_id: None,
            formats: vec![],
            ending: GeneratedLineEnding::Paragraph,
        });
    } else {
        *wipo = wipo
            .checked_add(1)
            .ok_or_else(|| structure_error("wipo_overflow"))?;
        let id = Some(*wipo);
        let wrapped = wrap(logical, &[]);
        for (local, line_chars) in wrapped.iter().enumerate() {
            if local > 0 {
                body.extend_from_slice(b"\r\n");
            }
            let line_index = lines.len() as u32;
            let mut text = String::new();
            for (index, item) in line_chars.iter().enumerate() {
                text.push(item.ch);
                encode_char(item.ch, body)?;
                if let Some(image_id) = &item.image_id {
                    images.push(GeneratedImageAnchor {
                        image_id: image_id.clone(),
                        line_index,
                        character_index: index as u32,
                    });
                }
            }
            lines.push(GeneratedLine {
                paragraph_id: paragraph.id.clone(),
                cell_id: None,
                text,
                shape_index: shape_index(
                    dialect,
                    paragraph.indent_raw,
                    &paragraph.align,
                    paragraph.lspace,
                ),
                wipo_id: id,
                formats: format_spans(line_chars),
                ending: if local + 1 == wrapped.len() {
                    GeneratedLineEnding::Paragraph
                } else {
                    GeneratedLineEnding::Continuation
                },
            });
        }
    }
    body.extend_from_slice(b"\x15\r\n");
    Ok(())
}

fn logical_inlines(inlines: &[HmlInline]) -> Result<Vec<LogicalChar>, CoreError> {
    let mut out = Vec::new();
    for inline in inlines {
        match inline {
            HmlInline::Text {
                text,
                formats,
                size,
            } => {
                for ch in text.chars() {
                    if matches!(ch, '\0' | '\r' | '\n' | '\x0c' | '\x12' | '\x13' | '\x15') {
                        return Err(structure_error("hidden_control"));
                    }
                    out.push(LogicalChar {
                        ch,
                        formats: formats.clone(),
                        size: *size,
                        image_id: None,
                    });
                }
            }
            HmlInline::Image { image_id } => out.push(LogicalChar {
                ch: '_',
                formats: vec![],
                size: None,
                image_id: Some(image_id.clone()),
            }),
        }
    }
    Ok(out)
}

fn logical_chars(paragraph: &crate::HmlParagraph) -> Result<Vec<LogicalChar>, CoreError> {
    logical_inlines(&paragraph.inlines)
}

/// 줄나눔. `targets` 가 있으면 원본 HLT 의 줄 길이(EUC-KR 바이트)를 그대로 따르고,
/// 없으면 폭으로 흘린다.
///
/// ⛔ **폭 규칙은 통특기를 재현하지 못한다.** 네이티브 PCT 줄 경계 1,845개에서
/// 다음 글자가 한글인 경계만 봐도 줄 폭 최대가 39.5, 최소가 33.0 이라 한계값 하나로
/// 둘을 설명할 수 없다(비례 폰트 메트릭이다). 그래서 원본이 있을 때는 계산하지 않는다.
/// 폭 규칙은 원본 줄나눔이 없는 입력(`.docx`, `.hml`, KIPO XML)과 표 칸에만 남는다.
///
/// 다른 PCT 문서(경계 450개, 줄끝 공백 제외)에서도: 통특기가 **허용한**
/// 줄 폭 최대 39.0, **거부한** 폭 최소 34.0 이라 역시 단일 한계값이 없다. 39.0 은
/// 그 「허용 최대」와 같아서 통특기보다 **일찍** 끊는 일은 없고 늦게 끊는 일만 남는다.
/// 늦게 끊은 줄은 통특기가 열 때 다시 흘려 고친다(생성본을 통특기로 되저장하면
/// 저장 줄 996 -> 1017 = 통특기 산출본과 같은 수, 다른 블록 28 -> 5). 남는 5 줄은
/// 통특기가 「들어가니 그대로 둔다」로 지키는 줄이다(45자 vs 통특기 42자).
/// 저장 본문 폭(twip) — 생성 서식의 A4 여백(왼 0.984in · 오 0.7868in)을 뺀 폭. template R15 와 같은 식.
const STORED_BODY_TWIPS: f64 = (210.0 / 25.4 - 0.984 - 0.7868) * 1440.0;

/// R15 저장 줄나눔의 줄마다 바이트 수. 그림·첨자·12pt 아닌 글자가 섞이면 `None`(폭 미확정 · 옛 39칸 규칙으로).
fn stored_wrap_targets(chars: &[LogicalChar], paragraph: &crate::HmlParagraph) -> Option<Vec<u32>> {
    use crate::reflow::{self, Item};
    let mut items = Vec::with_capacity(chars.len());
    for item in chars {
        let plain_size = item.size.is_none_or(|size| size == 12);
        let script = item.formats.iter().any(|format| {
            matches!(
                format,
                CharacterFormat::Superscript | CharacterFormat::Subscript
            )
        });
        if item.image_id.is_some() || !plain_size || script {
            return None;
        }
        items.push(if item.ch.is_ascii() {
            Item::Ascii(item.ch as u8, false)
        } else {
            Item::Wide(false)
        });
    }
    // 폭 단위: 한글 100 = 12pt × 20 twip → twip × 5 / 12.
    let scale = 5.0 / 12.0;
    let width =
        STORED_BODY_TWIPS - f64::from(paragraph.lmargin_raw) - f64::from(paragraph.rmargin_raw);
    let ends = reflow::wrap(
        &items,
        f64::from(paragraph.indent_raw) * scale,
        width * scale,
    );
    let mut targets = Vec::with_capacity(ends.len());
    let mut from = 0;
    for end in ends {
        targets.push(
            chars[from..end]
                .iter()
                .map(|item| if item.ch.is_ascii() { 1 } else { 2 })
                .sum(),
        );
        from = end;
    }
    Some(targets)
}

/// 칸 글 저장 줄나눔(R15): 폭 = 칸 폭(twip, 칸 레코드 +20) − 첫 줄 들여쓰기 · 글자 = 크기(기본 10pt).
/// 그림·첨자가 섞이거나 칸 폭을 모르면 한 줄(옛 방식).
fn cell_wrap(chars: &[LogicalChar], cell: &crate::HmlCell) -> Vec<Vec<LogicalChar>> {
    use crate::reflow::{self, Item};
    let one = || vec![chars.to_vec()];
    let Some(width) = cell.width_raw.filter(|width| *width > 0) else {
        return one();
    };
    let size = chars.iter().find_map(|item| item.size).unwrap_or(10);
    let mut items = Vec::with_capacity(chars.len());
    for item in chars {
        let script = item.formats.iter().any(|format| {
            matches!(
                format,
                CharacterFormat::Superscript | CharacterFormat::Subscript
            )
        });
        if item.image_id.is_some() || script || item.size.is_some_and(|value| value != size) {
            return one();
        }
        items.push(if item.ch.is_ascii() {
            Item::Ascii(item.ch as u8, false)
        } else {
            Item::Wide(false)
        });
    }
    if items.is_empty() {
        return one();
    }
    let scale = 5.0 / f64::from(size);
    let ends = reflow::wrap(
        &items,
        f64::from(cell.indent_raw) * scale,
        f64::from(width) * scale,
    );
    let mut out = Vec::with_capacity(ends.len());
    let mut from = 0;
    for end in ends {
        out.push(chars[from..end].to_vec());
        from = end;
    }
    out
}

fn wrap(chars: Vec<LogicalChar>, targets: &[u32]) -> Vec<Vec<LogicalChar>> {
    if chars.is_empty() {
        return vec![vec![]];
    }
    let mut out = vec![Vec::new()];
    let mut units = 0.0;
    let mut bytes = 0_u32;
    let mut target = targets.iter();
    let mut want = target.next().copied();
    for item in chars {
        let width = if item.ch.is_ascii() { 0.5 } else { 1.0 };
        let size = if item.ch.is_ascii() { 1 } else { 2 };
        let full = match want {
            // 판독기가 문단 앞뒤 ASCII 공백을 잘라내므로 길이가 몇 바이트 어긋날 수 있다.
            // 그래도 줄이 그만큼 밀릴 뿐 줄 수는 원본과 같다.
            Some(limit) => bytes + size > limit,
            // 목표를 다 썼는데 글자가 남으면 마지막 줄에 붙인다.
            None if targets.is_empty() => units + width > 39.0,
            None => false,
        };
        if full && !out.last().unwrap().is_empty() {
            // 통특기는 공백을 줄끝에 매단다(예: 줄 「…보다 」). 넘치는
            // 글자가 공백이면 현재 줄 끝에 두고 다음 글자부터 새 줄. 전에는 다음 줄이 공백으로 시작했다.
            let hanging_space = item.ch == ' ';
            if hanging_space {
                out.last_mut().unwrap().push(item);
                out.push(Vec::new());
                units = 0.0;
                bytes = 0;
                if want.is_some() {
                    want = target.next().copied();
                }
                continue;
            }
            out.push(Vec::new());
            units = 0.0;
            bytes = 0;
            if want.is_some() {
                want = target.next().copied();
            }
        }
        units += width;
        bytes += size;
        out.last_mut().unwrap().push(item);
    }
    // 원본이 문단 끝에 빈 줄(0바이트)을 달았으면 그대로 둔다 — 글이 줄 폭에 꼭 차면 통특기가 빈 줄을 하나 더 만든다
    // (의견서 실물 opinion.HLT 문단 15: [.., 15, 0]). 버리면 줄 수가 하나 준다.
    out.extend(target.take_while(|&&limit| limit == 0).map(|_| Vec::new()));
    out
}

fn format_spans(chars: &[LogicalChar]) -> Vec<GeneratedFormatSpan> {
    let mut out = Vec::new();
    let mut start = 0_usize;
    while start < chars.len() {
        let formats = &chars[start].formats;
        let size = chars[start].size;
        let mut end = start + 1;
        while end < chars.len() && chars[end].formats == *formats && chars[end].size == size {
            end += 1;
        }
        // 크기만 다른 구간도 문자모양이 갈린다 (네이티브 표 셀 attr=0/font=10).
        if !formats.is_empty() || size.is_some() {
            out.push(GeneratedFormatSpan {
                start: start as u32,
                end: end as u32,
                formats: formats.clone(),
                size,
            });
        }
        start = end;
    }
    out
}

fn encode_char(ch: char, out: &mut Vec<u8>) -> Result<(), CoreError> {
    let s = ch.to_string();
    let (bytes, _, bad) = EUC_KR.encode(&s);
    if bad {
        // 어떤 글자가 막혔는지 남긴다(입력기 · 붙여넣기가 들인 글자를 기록 파일에서 찾게).
        let mut error = CoreError::new(ErrorCode::Cp949Invalid, "generate_body", false);
        error
            .details
            .insert("char".into(), format!("U+{:04X}", ch as u32));
        return Err(error);
    }
    out.extend_from_slice(&bytes);
    Ok(())
}

fn section_table(document: &HmlDocument) -> Result<Vec<u8>, CoreError> {
    let mut out = Vec::with_capacity(document.sections.len() * 67);
    for (i, section) in document.sections.iter().enumerate() {
        let (enc, _, bad) = EUC_KR.encode(&section.title);
        if bad || enc.len() > 51 {
            return Err(CoreError::new(
                ErrorCode::Cp949Invalid,
                "generate_sections",
                false,
            ));
        }
        let mut rec = [0_u8; 67];
        rec[..enc.len()].copy_from_slice(&enc);
        rec[51..55].copy_from_slice(&(enc.len() as u32).to_le_bytes());
        rec[55..59].copy_from_slice(&((i + 1) as u32).to_le_bytes());
        rec[59..63].copy_from_slice(&(0x1DF2F60_u32 - 0x50 * i as u32).to_le_bytes());
        let prev = if i == 0 {
            0
        } else {
            0x39CCD70_u32 - 0x50 * (i as u32 - 1)
        };
        rec[63..67].copy_from_slice(&prev.to_le_bytes());
        out.extend_from_slice(&rec);
    }
    Ok(out)
}

/// 문단 모양 줄지도. `(들여쓰기, 정렬, 줄 높이, 그 프로필의 모양 번호)`.
/// ⛔ 이 표는 프로필 자산(`*_shapes.bin`)이 그 번호에 실제로 담고 있는 값과
/// 반드시 같아야 한다. 어긋나면 우리가 고른 줄 높이와 통특기가 그리는 줄 높이가
/// 갈린다(2026-08-27 국내 표가 쪽을 넘긴 원인). 계약은
/// `the_shape_map_matches_the_profile_asset` 가 지킨다.
pub(crate) const DOMESTIC_SHAPE_MAP: &[(i32, u32, u32, u32)] = &[
    (0, 1, 160, 1),
    (800, 2048, 160, 2),
    (0, 2048, 280, 4),
    (800, 2048, 280, 5),
    (800, 1024, 280, 6),
    (0, 1024, 280, 7),
    (800, 1, 280, 8),
    (0, 1024, 130, 9),
    (0, 2048, 130, 10),
    (0, 1, 280, 11),
];

pub(crate) const PCT_SHAPE_MAP: &[(i32, u32, u32, u32)] = &[
    (0, 1, 160, 1),
    (800, 2048, 160, 2),
    (0, 1, 280, 3),
    (0, 2048, 280, 4),
    (800, 2048, 280, 5),
    (800, 1024, 280, 6),
    (0, 1024, 280, 7),
    (800, 1, 280, 8),
    (0, 2048, 130, 14),
    (0, 1024, 130, 15),
];

fn shape_index(dialect: GenerationDialect, indent: i32, align: &str, lspace: u32) -> u32 {
    let indent = ((indent as f64) * 0.8).round() as i32;
    let align = match align {
        "center" => 1,
        "left" => 1024,
        _ => 2048,
    };
    let (shapes, fallback): (&[(i32, u32, u32, u32)], u32) = match dialect {
        GenerationDialect::PatentSpecificationKo => (DOMESTIC_SHAPE_MAP, 2),
        GenerationDialect::PctPatent => (PCT_SHAPE_MAP, 4),
    };
    if let Some((_, _, _, index)) =
        shapes
            .iter()
            .find(|(candidate_indent, candidate_align, candidate_lspace, _)| {
                (*candidate_indent, *candidate_align, *candidate_lspace) == (indent, align, lspace)
            })
    {
        return *index;
    }
    shapes
        .iter()
        .filter(|(_, candidate_align, _, _)| *candidate_align == align)
        .min_by_key(|(candidate_indent, _, candidate_lspace, _)| {
            (
                candidate_lspace.abs_diff(lspace),
                candidate_indent.abs_diff(indent),
            )
        })
        .map(|(_, _, _, index)| *index)
        .unwrap_or(fallback)
}

/// 표 안 문단 전용 줄지도. 이 모양들은 레코드 필드 7 에 2 를 달고 있고, 그 표시가
/// 있어야 마지막 행 뒤의 일반 문단이 표 밖에 놓인다.
pub(crate) const DOMESTIC_TABLE_SHAPE_MAP: &[(i32, u32, u32, u32)] = &[
    (0, 1024, TABLE_CELL_LSPACE, 9),
    (0, 2048, TABLE_CELL_LSPACE, 10),
    // 2026-09-06 실물: 통특기로 noop_v15.HLT 표 1 셀 하나를 가운데 정렬해 저장한
    // center_cell.HLT 가 새 모양 12 = (indent 0, center 1, lspace 130, field7 2) 를
    // 만들었다(줄 654). 그 전에는
    // 130 짜리 가운데 모양이 없어 160 모양 1 로 폴백했다.
    (0, 1, TABLE_CELL_LSPACE, 12),
];

pub(crate) const PCT_TABLE_SHAPE_MAP: &[(i32, u32, u32, u32)] = &[
    (0, 2048, TABLE_CELL_LSPACE, 14),
    (0, 1024, TABLE_CELL_LSPACE, 15),
    // 2026-09-06 실물 2건(noop_v15 → center_cell.HLT, pct_a → pct_a_center_cell.HLT): 통특기는 셀을
    // 가운데 정렬하면 그 셀의 모양 레코드를 복사해 정렬만 1 로 바꾼 새 모양을 만든다(줄간격 130,
    // field7 은 원래 셀 모양 그대로). 이 프로필의 셀 모양 14 를 복사한 22 가 그것이다.
    (0, 1, TABLE_CELL_LSPACE, 22),
];

/// 표 칸에 든 줄의 줄 높이. **실물 12건 1,160줄 전수에서 예외 없이 130 이다**
/// (서식 111, 113, 140 전부).
pub(crate) const TABLE_CELL_LSPACE: u32 = 130;

/// 셀 문단의 문단모양. 줄 높이는 입력이 무어라 하든 [`TABLE_CELL_LSPACE`] 로 정한다.
///
/// ⛔ 입력이 셀의 줄 높이를 안 실어 오는 경로가 있다(`.docx`, KIPO XML 은 본문 값 280 을
/// 그대로 준다). 그것을 그대로 믿으면 표가 통특기에서 배 넘게 길어져 마지막 행이
/// 다음 쪽으로 넘어간다(2026-08-27 결함 A).
///
/// ⛔ 예전에는 PCT 셀이 `lspace == 280` 일 때 모양 14, 18 을 그냥 돌려줬다. 그런데 14 는
/// 줄 높이가 130 짜리다. 그 어긋남은 판독기가 셀의 줄 높이를 한 레코드 앞에서 읽어
/// 130 을 280 으로 내던 결함을 상쇄하던 것이었다. 판독기를 고친 지금은 값으로 맞춘다.
fn table_cell_shape_index(dialect: GenerationDialect, indent: i32, align: &str) -> u32 {
    let table_map = match dialect {
        GenerationDialect::PatentSpecificationKo => DOMESTIC_TABLE_SHAPE_MAP,
        GenerationDialect::PctPatent => PCT_TABLE_SHAPE_MAP,
    };
    let wanted = match align {
        "center" => 1,
        "left" => 1024,
        _ => 2048,
    };
    if let Some((_, _, _, index)) = table_map.iter().find(|(_, a, _, _)| *a == wanted) {
        return *index;
    }
    shape_index(dialect, indent, align, TABLE_CELL_LSPACE)
}
fn structure_error(reason: &str) -> CoreError {
    let mut e = CoreError::new(ErrorCode::StructureInvalid, "generate_body", false);
    e.details.insert("reason".into(), reason.into());
    e
}

#[cfg(test)]
mod shape_map_tests {
    use super::{DOMESTIC_SHAPE_MAP, DOMESTIC_TABLE_SHAPE_MAP, PCT_SHAPE_MAP, PCT_TABLE_SHAPE_MAP};
    use crate::{GenerationDialect, generation_profile};

    /// 프로필 자산의 모양 레코드는 72바이트다. 파일에서는 `shape_count` 뒤 8바이트가
    /// 0번 레코드의 앞 두 필드라서, 자산 안에서 레코드 `i` 의 필드 `j` 는
    /// `72*i + 4*j - 8` 에 있다.
    fn field(shapes: &[u8], index: u32, slot: usize) -> u32 {
        let at = 72 * index as usize + 4 * slot - 8;
        u32::from_le_bytes(shapes[at..at + 4].try_into().expect("shape field"))
    }

    fn mismatches(
        dialect: GenerationDialect,
        map: &[(i32, u32, u32, u32)],
        label: &str,
    ) -> Vec<String> {
        let profile = generation_profile(dialect);
        let mut wrong = Vec::new();
        for (indent, align, lspace, index) in map {
            let got = (
                field(&profile.shapes, *index, 2) as i32,
                field(&profile.shapes, *index, 3),
                field(&profile.shapes, *index, 10),
            );
            if got != (*indent, *align, *lspace) {
                wrong.push(format!(
                    "{label} 모양 {index}: 줄지도 {:?}, 자산 {got:?}",
                    (*indent, *align, *lspace)
                ));
            }
        }
        wrong
    }

    /// ⛔ 줄지도가 자산에 없는 값을 적어 두면 우리가 고른 줄 높이와 통특기가 그리는
    /// 줄 높이가 갈린다. 2026-08-27 에 국내 표가 정본보다 1.8배 높아져 마지막 행이
    /// 다음 쪽으로 넘어간 원인이 이것이었다.
    #[test]
    fn the_shape_map_matches_the_profile_asset() {
        let mut found = Vec::new();
        for (dialect, map, label) in [
            (
                GenerationDialect::PatentSpecificationKo,
                DOMESTIC_SHAPE_MAP,
                "국내",
            ),
            (
                GenerationDialect::PatentSpecificationKo,
                DOMESTIC_TABLE_SHAPE_MAP,
                "국내 표",
            ),
            (GenerationDialect::PctPatent, PCT_SHAPE_MAP, "PCT"),
            (GenerationDialect::PctPatent, PCT_TABLE_SHAPE_MAP, "PCT 표"),
        ] {
            found.extend(mismatches(dialect, map, label));
        }
        assert!(
            found.is_empty(),
            "줄지도와 프로필 자산이 어긋난다:
{}",
            found.join(
                "
"
            )
        );
    }

    /// 가운데 정렬 식별항목(들여쓰기 0 · 줄간격 280)은 두 방언 모두 들여쓰기 0 모양으로 간다.
    /// ⛔ PCT 줄지도에 자산 모양 3 이 빠져 있어 가장 가까운 「가운데 · 800」(모양 8)로 떨어졌다 —
    /// pct_e 를 다시 구우면 【발명의 설명】 이 원본 0 에서 800 으로 바뀌었다(2026-10-15).
    #[test]
    fn centered_identification_items_get_a_zero_indent_shape() {
        use super::shape_index;
        let profile = |dialect| generation_profile(dialect);
        for dialect in [
            GenerationDialect::PatentSpecificationKo,
            GenerationDialect::PctPatent,
        ] {
            let index = shape_index(dialect, 0, "center", 280);
            let shapes = &profile(dialect).shapes;
            let got = (
                field(shapes, index, 2),
                field(shapes, index, 3),
                field(shapes, index, 10),
            );
            assert_eq!(got, (0, 1, 280), "{dialect:?} 모양 {index}");
        }
    }

    /// 표 전용 모양은 레코드 필드 7 에 2 를 달아야 한다. 그 표시가 있어야 마지막 행
    /// 뒤의 일반 문단이 표 밖에 놓인다(PCT 저장본).
    #[test]
    fn every_table_shape_carries_the_table_boundary_flag() {
        for (dialect, map, label) in [
            (
                GenerationDialect::PatentSpecificationKo,
                DOMESTIC_TABLE_SHAPE_MAP,
                "국내 표",
            ),
            (GenerationDialect::PctPatent, PCT_TABLE_SHAPE_MAP, "PCT 표"),
        ] {
            let profile = generation_profile(dialect);
            for (_, _, _, index) in map {
                assert_eq!(
                    field(&profile.shapes, *index, 7),
                    2,
                    "{label} 모양 {index} 에 표 경계 표시가 없다"
                );
            }
        }
    }
}
