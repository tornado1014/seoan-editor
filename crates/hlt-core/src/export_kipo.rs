//! HLT → KIPO XML (통특기 제출 세트의 `.hlz` 안에 들어가는 그 XML).
//!
//! 통특기 제출 세트와 같은 XML 을 내는 규칙:
//!
//! - 문단번호는 HLT 에 없다. 여기서 `0001` 부터 순번으로 부여하고, 요약은 `0001a`,
//!   대표도는 `0002a` 를 쓴다.
//! - `【…】` 섹션 표제는 번호를 받지 않고 XML 요소가 된다.
//! - `【도면의 간단한 설명】` 안의 문단 전부가 하나의 번호 문단으로 합쳐지고 `<br/>` 로 잇는다.
//! - `【표 N】` 의 제목 문단은 표의 `<title>` 로 흡수되고, 표 본문 문단이 번호를 받으며
//!   표 뒤 꼬리 텍스트가 다음 번호를 받는다.
//! - 표 열 폭은 셀 폭 × 1.25 (3600 → 4500, 3510 → 4388).
//! - 도면 이미지 크기(mm) = 객체 레코드 표시 필드 × 0.3756 (138개 중 130개 정확,
//!   나머지는 ±1mm. 통특기가 mm 를 저장하지 않아 왕복에서 잃는 정밀도다).

use crate::{
    CellFormatInventory, CellFormatRecord, CellRecord, CharacterFormat, CoreError, ErrorCode,
    FormatInventory, ImageOccurrence, InspectDocument, InspectOptions, ParagraphRecord,
    inspect_format_inventories, inspect_path,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};

/// OASIS CALS 표 교환 모델 네임스페이스. KipoEditor 설정의 oasisTableUrl 과 같은 값이다.
const OASIS_TABLE_NS: &str = "http://www.oasis-open.org/tables/exchange/1.0";

const ROW_END: char = '\u{12}';
const CELL_END: char = '\u{13}';
const PAGE_BREAK: char = '\u{c}';

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KipoExportOptions {
    pub keaps_version: String,
    pub editor_kind: String,
    pub document_id: String,
    pub page_count: Option<u32>,
    /// 도면 파일 이름 앞자리. 비우면 문서종류로 정한다 (PCT `pct`, 국내출원 `pat`).
    pub image_stem: String,
    /// 도면 파일 형식. 비우면 문서종류로 정한다 (PCT `jpg`, 국내출원 `tif`).
    pub image_format: String,
    /// 문단 번호를 HLT 식별번호로 매긴다(보정서). 제출 변환은 저장된 식별번호를 무시하고 차례로 센다
    /// (식별번호가 38 을 건너뛰어도 XML 은 0038).
    pub number_by_id: bool,
    /// 문단마다의 식별번호(`<p num>` · 문단 차례) — 서안으로 연 최종본은 HLT 에 식별번호를 싣지 않으니(생성기는
    /// 매달린 번호를 쓰지 않는다) 편집 세션이 든 번호를 여기로 넘긴다. HLT 에 식별번호가 있으면 그것을 쓴다.
    pub paragraph_numbers: Vec<Option<String>>,
}

impl Default for KipoExportOptions {
    fn default() -> Self {
        Self {
            keaps_version: "5.6".into(),
            editor_kind: "K".into(),
            document_id: "0".into(),
            page_count: None,
            image_stem: String::new(),
            image_format: String::new(),
            number_by_id: false,
            paragraph_numbers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KipoExportImage {
    pub image_id: String,
    pub file: String,
    pub width_mm: u32,
    pub height_mm: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KipoExport {
    pub xml: String,
    pub xml_sha256: String,
    /// (청구항 번호, claim-text 안의 마크업). 보정서 대조에 쓴다.
    pub claims: Vec<(String, String)>,
    pub images: Vec<KipoExportImage>,
    pub paragraph_count: u32,
    pub claim_count: u32,
    pub figure_count: u32,
    /// 문단 번호를 HLT 식별번호(`wipo_id`)로 매겼는가(최종본 불러오기로 연 문서). 아니면 차례로 셌다.
    pub numbered_by_id: bool,
    /// 【서열목록】 첨부 줄의 경로 — hlz 에 `st26sdosl.xml` 로 싣는다(`build_hlz`).
    pub sequence_file: Option<String>,
}

/// 최종본 편집에서 새로 생긴 문단의 식별번호(붙여넣기로 쪼갠 문단).
const NEW_PARAGRAPH_ID: u32 = u32::MAX;

/// 표시 필드(객체 레코드 +364/+368, pt 정수) → KIPO XML `wi`/`he` 정수 mm.
///
/// 통특기 제출 변환에 맞춘 반올림 계수. 그림 12장(폭·높이 24값)
/// 과 기존 골든 4값(355→133 · 505→190 · 256→96 · 396→149)을 모두 재현하는 계수 창은
/// [0.375260, 0.375264) 뿐이고 179/477 은 그 창 안의 최소 분모다. 옛 939/2500(0.3756)
/// 은 noop 12장 중 4장을 +1 로 틀렸고, 3/8 은 골든 505→190 과 noop 9번(481→181)을 틀린다.
pub fn display_units_to_mm(units: u32) -> u32 {
    (u64::from(units) * 179 + 238).div_euclid(477) as u32
}

/// import 규약 문서(XML 에서 들어와 상자 높이가 전부 168)의 pt → mm. 통특기는 원래 XML mm 를
/// 되돌린다: 필드 = round(mm × 180/25.4 × 3/8) 의 역 = pt × 25.4/72 × 96/90 = 254/675 반올림.
/// 그림 614장 614/614 (179/477 은 601/614, 13장이 −1).
/// writer/native 문서에는 쓰면 안 된다(12장 중 9장이 +1).
pub fn display_units_to_mm_import(units: u32) -> u32 {
    (u64::from(units) * 508 + 675).div_euclid(1_350) as u32
}

/// 그림 상자 높이(+52)가 전부 168 이면 XML 에서 들어온(import 규약) 문서다
/// (헤더 비트·종횡비로는 가를 수 없다).
pub fn is_import_convention(images: &[ImageOccurrence]) -> bool {
    !images.is_empty() && images.iter().all(|image| image.box_height == 168)
}

struct Section {
    /// 이 절의 요소 이름. 빈 문자열이면 상위 요소 안에 문단만 바로 넣는다.
    element: &'static str,
    /// 이 절을 감싸는 상위 요소. `【발명의 내용】` 계열이 `summary-of-invention` 을 쓴다.
    parent: Option<&'static str>,
    body: String,
}

pub fn export_kipo_xml(
    document: &InspectDocument,
    formats: &FormatInventory,
    cell_formats: &CellFormatInventory,
    options: &KipoExportOptions,
) -> Result<KipoExport, CoreError> {
    let cell_spans: BTreeMap<&str, &CellFormatRecord> = cell_formats
        .cells
        .iter()
        .filter(|record| !record.spans.is_empty())
        .map(|record| (record.cell_id.as_str(), record))
        .collect();
    let format_spans: BTreeMap<&str, &Vec<crate::CharacterFormatSpan>> = formats
        .paragraphs
        .iter()
        .map(|paragraph| (paragraph.paragraph_id.as_str(), &paragraph.spans))
        .collect();
    let cells_by_table: BTreeMap<&str, Vec<&CellRecord>> =
        document
            .tables
            .iter()
            .fold(BTreeMap::new(), |mut map, table| {
                let cells = document
                    .cells
                    .iter()
                    .filter(|cell| cell.table_id == table.id)
                    .collect();
                map.insert(table.id.as_str(), cells);
                map
            });
    // 칸 정렬 = 칸 첫 줄의 문단모양 정렬(entry/colspec `align`).
    let line_shape: BTreeMap<&str, &str> = document
        .lines
        .iter()
        .filter_map(|line| {
            line.shape_id
                .as_deref()
                .map(|shape| (line.id.as_str(), shape))
        })
        .collect();
    let shape_align: BTreeMap<&str, &str> = document
        .shapes
        .iter()
        .map(|shape| (shape.id.as_str(), shape.align.as_str()))
        .collect();
    let cell_aligns: BTreeMap<&str, &'static str> = document
        .cells
        .iter()
        .filter_map(|cell| {
            let first = cell.line_ids.first()?;
            let shape = line_shape.get(first.as_str())?;
            let align = match *shape_align.get(shape)? {
                "left" => "left",
                "center" => "center",
                "right" => "right",
                _ => "justify",
            };
            Some((cell.id.as_str(), align))
        })
        .collect();
    let images_by_paragraph: BTreeMap<&str, Vec<&ImageOccurrence>> =
        document
            .images
            .iter()
            .fold(BTreeMap::new(), |mut map, image| {
                map.entry(image.paragraph_id.as_str())
                    .or_default()
                    .push(image);
                map
            });

    let mut sections: Vec<Section> = Vec::new();
    let mut claims = String::new();
    let mut claim_list: Vec<(String, String)> = Vec::new();
    let mut drawings = String::new();
    let mut summary_parts: Vec<String> = Vec::new();
    let mut abstract_figure = String::new();
    let mut invention_title = String::new();
    let mut export_images = Vec::new();

    let (image_stem, image_format) = image_defaults(document, options);
    let explicit_format = !options.image_format.is_empty();
    let mut counter = 0_u32;
    // 보정서는 문단마다 저장된 식별번호를 XML 번호로 쓴다(최종본 불러오기 문서). 보정에서 문단을 지우면 번호가 비고
    // 뒤 번호는 그대로다(`p D 0009` 하나). 【표 N】·【수학식 N】 표제가 다음 <p> 의 번호를 들고, 표 꼬리는 +1.
    // 번호가 없는 문서(새 명세서)는 차례로 센다 — 번호가 있는 문서도 지우지 않았으면 두 방식이 같다.
    let numbered_by_id = options.number_by_id
        && document
            .paragraphs
            .iter()
            .any(|paragraph| paragraph.wipo_id.is_some_and(|id| id != NEW_PARAGRAPH_ID));
    let sync = |counter: &mut u32, paragraph: &ParagraphRecord| {
        if !numbered_by_id {
            return;
        }
        if let Some(id) = paragraph
            .wipo_id
            .filter(|id| *id != 0 && *id != NEW_PARAGRAPH_ID)
        {
            *counter = id - 1;
        }
    };
    let import_convention = is_import_convention(&document.images);
    let mut marker = String::new();
    // 【서열목록】 첨부 줄: (번호, 경로). 앞 절 문단으로 붙이지 않고 `<description>` 끝 `<sequence-list-doc>` 로 낸다.
    let mut sequence: Option<(u32, String)> = None;
    let mut joined_lines: Vec<String> = Vec::new();
    // 지금 줄을 모으는 절의 요소. 빈 문자열이면 모으지 않는다.
    let mut joined_element: &'static str = "";
    let mut table_title: Option<String> = None;
    let mut table_pending = false;
    let mut claim_open: Option<String> = None;
    let mut claim_parts: Vec<String> = Vec::new();
    let mut claim_count = 0_u32;
    let mut table_index = 0_usize;
    // 절 첫머리의 새 문단(앞에 이을 번호 문단이 없다) — 다음 번호 문단 앞에 붙인다.
    let mut pending_new: Vec<String> = Vec::new();
    // 【수학식 N】·【화학식 N】 표제 바로 뒤 문단 하나만 식이다(뒤 「여기서, …」 문단은 같은 절이어도 본문).
    let mut formula_pending: Option<&'static str> = None;
    let mut figure_count = 0_u32;

    // 표 문단의 꼬리(마지막 \x12 뒤)가 【…】 표제이면 표제 문단으로 따로 세운다.
    // 예: 꼬리가 "【표 2】" 라 통특기는 그것을 절 표제로 읽고, 다음 문단(캡션)을
    // 표 2 의 <title> 로 흡수한다. 꼬리를 본문 문단으로 내면 표제가 번호를 받고 뒤 번호가 +2 밀린다.
    let mut expanded: Vec<ParagraphRecord> = Vec::with_capacity(document.paragraphs.len() + 4);
    // 꼬리를 표제로 떼어 낸 표 문단 — 꼬리 번호 문단을 따로 내지 않는다.
    let mut split_tables = std::collections::BTreeSet::new();
    for paragraph in &document.paragraphs {
        if let Some((head, tail)) = paragraph.text.rsplit_once(ROW_END)
            && is_marker(tail.trim_matches(PAGE_BREAK).trim())
        {
            let mut table_part = paragraph.clone();
            table_part.text = format!("{head}{ROW_END}");
            let mut marker_part = paragraph.clone();
            marker_part.id = format!("{}:tail", paragraph.id);
            marker_part.text = tail.to_string();
            marker_part.line_ids = Vec::new();
            split_tables.insert(paragraph.id.clone());
            expanded.push(table_part);
            expanded.push(marker_part);
        } else {
            expanded.push(paragraph.clone());
        }
    }
    for paragraph in &expanded {
        let raw = paragraph.text.trim_matches(PAGE_BREAK).trim();
        if is_marker(raw) {
            flush_claim(
                &mut claims,
                &mut claim_list,
                &mut claim_open,
                &mut claim_parts,
            );
            flush_joined_lines(
                &mut sections,
                &mut counter,
                &mut joined_lines,
                joined_element,
            );
            sync(&mut counter, paragraph);
            // 새 문단만 있는 절(이을 번호 문단이 없다): 통특기는 절 요소 통째 보정을 내는데 번호가 `65535`(0xFFFFFFFF 의 16비트)다
            // (`<AmendBody elementName="technical-field" status="A"><technical-field><p num="65535">…`). 따라 하지 않고 거절한다.
            if !pending_new.is_empty() {
                return Err(export_error("new_paragraph_alone"));
            }
            marker = raw.to_string();
            joined_element = container_element(&marker)
                .map(|(element, _)| element)
                .filter(|element| joins_lines(element))
                .unwrap_or("");
            table_pending = marker.starts_with("【표");
            formula_pending = if marker.starts_with("【수학식") {
                Some("maths")
            } else if marker.starts_with("【화학식") {
                Some("chemistry")
            } else {
                None
            };
            if let Some((element, parent)) = container_element(&marker) {
                sections.push(Section {
                    element,
                    parent,
                    body: String::new(),
                });
            }
            if marker.starts_with("【청구항") {
                claim_count += 1;
                claim_open = Some(claim_number(&marker));
            }
            if marker.starts_with("【도 ") {
                figure_count += 1;
            }
            continue;
        }
        sync(&mut counter, paragraph);
        if marker == crate::sequence_listing::HEADING {
            // 통특기: 첨부 줄만 번호를 받고(경로는 빼고 「서열목록 전자파일 첨부」) 빈 줄은 안 낸다.
            if let Some(path) = crate::sequence_listing::link_path(raw) {
                counter += 1;
                sequence = Some((counter, path.to_string()));
                continue;
            }
            if raw.is_empty() {
                continue;
            }
        }

        let mut markup = inline_markup(paragraph, format_spans.get(paragraph.id.as_str()).copied());
        // 본문 인라인 그림(화학 구조식 등). 판독기는 그림 자리에 `_` 를 둔다. 자리 수가 그림 수와
        // 같을 때만 자리마다 <img/> 로 바꾸고, 다르면 문단 끝에 붙인다.
        // 셀 안 그림은 표 마크업 몫이라 여기서 빼고, 【도 N】 은 아래 도면 분기가 맡는다.
        if !marker.starts_with("【도 ")
            && let Some(occurrences) = images_by_paragraph.get(paragraph.id.as_str())
        {
            let inline: Vec<&&ImageOccurrence> = occurrences
                .iter()
                .filter(|image| image.cell_id.is_none())
                .collect();
            if !inline.is_empty() {
                let tags: Vec<String> = inline
                    .iter()
                    .map(|image| {
                        let (tag, exported) = image_markup(
                            document,
                            image,
                            &image_stem,
                            &image_format,
                            explicit_format,
                            import_convention,
                            export_images.len() + 1,
                        );
                        export_images.push(exported);
                        tag
                    })
                    .collect();
                markup = place_images(markup, &tags);
            }
        }

        if marker.starts_with("【발명의 명칭】") {
            invention_title = markup;
            continue;
        }
        if marker.starts_with("【청구항") {
            claim_parts.push(markup);
            continue;
        }
        if marker.starts_with("【도 ") {
            let number = figure_number(&marker);
            if let Some(occurrences) = images_by_paragraph.get(paragraph.id.as_str()) {
                for image in occurrences {
                    let (tag, exported) = image_markup(
                        document,
                        image,
                        &image_stem,
                        &image_format,
                        explicit_format,
                        import_convention,
                        export_images.len() + 1,
                    );
                    drawings.push_str(&format!("<figure num=\"{number}\">{tag}</figure>\n"));
                    export_images.push(exported);
                }
            }
            continue;
        }
        if !joined_element.is_empty() {
            joined_lines.push(markup);
            continue;
        }
        // 선행기술문헌 줄 「(특허문헌 1) 글」 → `<patcit num="0001"><text>글</text></patcit>` · 번호 없는 줄(보정 명세서)은
        // 앞 문헌과 같은 `<p>` 에 든다(통특기 최종본: 한 `<p>` 에 문헌 둘 = 번호 한 줄 + 번호 없는 줄).
        let citation = sections.last().and_then(|section| match section.element {
            "patent-literature" => Some(("특허문헌", "patcit")),
            "non-patent-literature" => Some(("비특허문헌", "nplcit")),
            _ => None,
        });
        if let Some((label, element)) = citation
            && let Some(rest) = markup.strip_prefix(&format!("({label} "))
            && let Some((number, text)) = rest.split_once(") ")
        {
            let number: String = number.chars().filter(char::is_ascii_digit).collect();
            // 괄호 뒤 공백 둘 = 서지 꼴(「(특허문헌 1)  KR10-2099-0000001 A」 · 실물 43건 모두 이름 · 날짜 빈 칸).
            let bibliographic = text.strip_prefix(' ').and_then(|rest| {
                let (id, kind) = rest.rsplit_once(' ')?;
                let split = id.find(|ch: char| !ch.is_ascii_uppercase())?;
                (split == 2).then(|| (&id[..2], &id[2..], kind, id))
            });
            let cited = match bibliographic {
                Some((country, doc_number, kind, id)) => format!(
                    "<{element} num=\"{number:0>4}\" dnum=\"{id}\" dnum-type=\"publication number\"><document-id>\
                     <country>{country}</country><doc-number>{doc_number}</doc-number><kind>{kind}</kind>\
                     <name></name><date></date></document-id></{element}>\n"
                ),
                None => {
                    format!("<{element} num=\"{number:0>4}\"><text>{text}</text></{element}>\n")
                }
            };
            let section = sections
                .last_mut()
                .ok_or_else(|| export_error("no_section"))?;
            let continues = numbered_by_id && paragraph.wipo_id.is_none_or(|id| id == 0);
            match section.body.strip_suffix("</p>\n") {
                Some(open) if continues => section.body = format!("{open}{cited}</p>\n"),
                _ => {
                    counter += 1;
                    section
                        .body
                        .push_str(&format!("<p num=\"{counter:04}\">{cited}</p>\n"));
                }
            }
            continue;
        }

        if marker.starts_with("【요약】") {
            // 【요약】 문단 전부가 하나의 0001a 로 합쳐지고 <br/> 로 이어진다
            // (예: "요약<br/>본 발명은 …<br/><img …/>(I)").
            summary_parts.push(markup);
            continue;
        }
        if marker.starts_with("【대표도】") {
            abstract_figure.push_str(&format!(
                "<p num=\"0002a\"><figref num=\"{}\"/></p>\n",
                figure_number(&paragraph.text)
            ));
            continue;
        }
        if table_pending && !paragraph.text.contains(ROW_END) && table_title.is_none() {
            table_title = Some(markup);
            continue;
        }
        if paragraph.text.contains(ROW_END) {
            if !pending_new.is_empty() {
                return Err(export_error("new_paragraph_before_table"));
            }
            // 표 레코드의 앵커는 【표 N】 표제 문단이라 본문 문단과 다르다. 표는 문서 순서대로
            // 나오므로 순차로 소비한다.
            let table = document
                .tables
                .get(table_index)
                .ok_or_else(|| export_error("table_anchor"))?;
            table_index += 1;
            let cells = cells_by_table
                .get(table.id.as_str())
                .ok_or_else(|| export_error("table_cells"))?;
            counter += 1;
            // 셀 안 그림: 셀 id 별로 <img/> 를 미리 만들어 문서 순서대로 수출 목록에 올린다
            // (예: 표 안 구조식 16장이 entry 글자 자리에 <img/> 로 들어간다).
            let mut cell_images: BTreeMap<&str, Vec<String>> = BTreeMap::new();
            if let Some(occurrences) = images_by_paragraph.get(paragraph.id.as_str()) {
                for image in occurrences {
                    if let Some(cell_id) = image.cell_id.as_deref() {
                        let (tag, exported) = image_markup(
                            document,
                            image,
                            &image_stem,
                            &image_format,
                            explicit_format,
                            import_convention,
                            export_images.len() + 1,
                        );
                        export_images.push(exported);
                        cell_images.entry(cell_id).or_default().push(tag);
                    }
                }
            }
            let body = table_markup(
                table.occurrence + 1,
                table_title.take().unwrap_or_default(),
                cells,
                &paragraph.text,
                &cell_spans,
                &cell_images,
                &cell_aligns,
            )?;
            push_paragraph(
                &mut sections,
                format!("<p num=\"{counter:04}\">{body}</p>\n"),
            )?;
            table_pending = false;
            // 표 꼬리는 글자 그대로.
            let tail = paragraph
                .text
                .rsplit(ROW_END)
                .next()
                .unwrap_or_default()
                .trim_matches(PAGE_BREAK)
                .to_string();
            // 꼬리 줄은 비어 있어도 번호를 받는다(`<p num="0127"></p>` · 꼬리가 공백 한 칸이어도 같다).
            // 꼬리가 【…】 표제라 떼어 낸 표는 예외(표 1 → 【표 2】).
            if !split_tables.contains(&paragraph.id) {
                counter += 1;
                push_paragraph(
                    &mut sections,
                    format!(
                        "<p num=\"{counter:04}\">{}</p>\n",
                        table_tail_markup(
                            paragraph,
                            format_spans.get(paragraph.id.as_str()).copied(),
                            &tail
                        )
                    ),
                )?;
            }
            continue;
        }
        // 번호를 못 받은 새 문단은 통특기 보정서가 앞 번호 문단에 <br/> 로 잇는다(`p A 0012` = "새 글<br/>옛 글").
        // 절 첫머리라 앞 번호 문단이 없으면 다음 번호 문단 앞에 붙인다(【기술분야】 첫머리 → `p A 0001` = "새 문단<br/>옛 0001").
        if numbered_by_id && paragraph.wipo_id == Some(NEW_PARAGRAPH_ID) {
            let section = sections
                .last_mut()
                .ok_or_else(|| export_error("no_section"))?;
            match section.body.strip_suffix("</p>\n") {
                Some(open) => section.body = format!("{open}<br/>{markup}</p>\n"),
                None => pending_new.push(markup),
            }
            continue;
        }
        if !pending_new.is_empty() {
            pending_new.push(markup);
            markup = pending_new.join("<br/>");
            pending_new.clear();
        }
        // 식 문단은 `<maths num="N">`·`<chemistry num="N">` 으로 싼다(명세서 XML · 보정서 둘 다).
        if let Some(element) = formula_pending.take() {
            // 보정서는 식 자리에 글이 섞이면 서식 오류로 거절한다(「S-009 식별항목과 그 내용이 서로 일치하지 않습니다」).
            let mut parts = markup.split("<img ");
            let only_images = parts.next().is_some_and(|head| head.trim().is_empty())
                && parts.all(|part| {
                    part.split_once("/>")
                        .is_some_and(|(_, after)| after.trim().is_empty())
                });
            if numbered_by_id && !only_images {
                return Err(export_error("formula_not_image"));
            }
            markup = format!(
                "<{element} num=\"{}\">{markup}</{element}>",
                figure_number(&marker)
            );
        }
        counter += 1;
        push_paragraph(
            &mut sections,
            format!("<p num=\"{counter:04}\">{markup}</p>\n"),
        )?;
    }
    flush_claim(
        &mut claims,
        &mut claim_list,
        &mut claim_open,
        &mut claim_parts,
    );
    flush_joined_lines(
        &mut sections,
        &mut counter,
        &mut joined_lines,
        joined_element,
    );

    let mut description = String::new();
    if !invention_title.is_empty() {
        description.push_str(&format!(
            "<invention-title>{invention_title}</invention-title>\n"
        ));
    }
    let mut open_parent: Option<&'static str> = None;
    for section in &sections {
        if section.body.is_empty() {
            // 보정서는 내용 없는 절을 서식 오류로 거절한다(예: 【기술분야】 유일 문단 삭제 →
            // 「E-248 : (기술분야) 해당내용이 없습니다.」 · 보정서 안 나옴).
            if numbered_by_id && !section.element.is_empty() {
                return Err(export_error("section_empty"));
            }
            continue;
        }
        if open_parent != section.parent {
            if let Some(parent) = open_parent {
                description.push_str(&format!("</{parent}>\n"));
            }
            if let Some(parent) = section.parent {
                description.push_str(&format!("<{parent}>\n"));
            }
            open_parent = section.parent;
        }
        if section.element.is_empty() {
            description.push_str(&section.body);
            continue;
        }
        description.push_str(&format!(
            "<{0}>\n{1}</{0}>\n",
            section.element, section.body
        ));
    }
    if let Some(parent) = open_parent {
        description.push_str(&format!("</{parent}>\n"));
    }
    // 통특기 실측(합성 국내 명세서 · ST.26): `…</description-of-embodiments>\n<sequence-list-doc>…</sequence-list-doc></description>`.
    if let Some((number, _)) = &sequence {
        description.push_str(&format!(
            "<sequence-list-doc><sequence-list file=\"{}\" seq-file-type=\"ST.26\">\n<p num=\"{number:04}\">{}</p>\n</sequence-list></sequence-list-doc>",
            crate::sequence_listing::HLZ_NAME,
            crate::sequence_listing::LINK_PREFIX
        ));
    }

    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let page_count = options
        .page_count
        .map(|value| format!(" pageCount=\"{value}\""))
        .unwrap_or_default();
    xml.push_str(&format!(
        "<KIPO keapsVersion=\"{}\" editorKind=\"{}\"{} imgApply=\"N\" xmlns=\"http://www.kipo.go.kr\">\n",
        options.keaps_version, options.editor_kind, page_count
    ));
    xml.push_str(&format!(
        "<{0}{1} documentID=\"{2}\">\n",
        document_element(document),
        document_attributes(document),
        options.document_id
    ));
    xml.push_str(&format!("<description>\n{description}</description>\n"));
    if !claims.is_empty() {
        xml.push_str(&format!("<claims>\n{claims}</claims>\n"));
    }
    // 문서 끝 빈 문단은 요약에 안 든다(예: `화학식 K.</p>` — 뒤에 `<br/>` 없음).
    while summary_parts.last().is_some_and(|part| part.is_empty()) {
        summary_parts.pop();
    }
    let abstract_summary = if summary_parts.is_empty() {
        String::new()
    } else {
        format!("<p num=\"0001a\">{}", summary_parts.join("<br/>\n"))
    };
    if !abstract_summary.is_empty() || !abstract_figure.is_empty() {
        xml.push_str("<abstract>\n");
        if !abstract_summary.is_empty() {
            xml.push_str(&format!("<summary>\n{abstract_summary}</p>\n</summary>\n"));
        }
        if !abstract_figure.is_empty() {
            xml.push_str(&format!(
                "<abstract-figure>\n{abstract_figure}</abstract-figure>\n"
            ));
        }
        xml.push_str("</abstract>\n");
    }
    if !drawings.is_empty() {
        xml.push_str(&format!("<drawings>\n{drawings}</drawings>\n"));
    }
    xml.push_str(&format!("</{}>\n</KIPO>", document_element(document)));

    Ok(KipoExport {
        claims: claim_list,
        xml_sha256: format!("{:X}", Sha256::digest(xml.as_bytes())),
        xml,
        images: export_images,
        paragraph_count: counter,
        claim_count,
        figure_count,
        numbered_by_id,
        sequence_file: sequence.map(|(_, path)| path),
    })
}

fn document_element(document: &InspectDocument) -> &'static str {
    // ⛔ 원시값을 그대로 비교하지 마라. `0x8000` 표시가 붙기도 안 붙기도 한다.
    // 판정은 `DocumentTreeKind` 한 곳에서 한다.
    match document.summary.document_tree_kind {
        crate::DocumentTreeKind::PctPatent => "PCTPatentCAFDOC",
        _ => "PatentCAFDOC",
    }
}

/// 국내출원: `<PatentCAFDOC docflag="1.0" documentID=…>`.
/// PCT 에는 `docflag` 가 없다 — 판정은 뿌리 요소(PCT 문서 나무)로 한다. 원시 문서 종류(32881)로 가르면
/// 다른 종류 값의 PCT 문서에 docflag 가 붙었다.
fn document_attributes(document: &InspectDocument) -> &'static str {
    match document.summary.document_tree_kind {
        crate::DocumentTreeKind::PctPatent => "",
        _ => " docflag=\"1.0\"",
    }
}

/// 도면 파일 형식 기본값. PCT 는 `pct00001.jpg`, 국내출원은 `pat00001.tif` 다.
fn image_defaults(document: &InspectDocument, options: &KipoExportOptions) -> (String, String) {
    let pct = document.summary.document_tree_kind == crate::DocumentTreeKind::PctPatent;
    let stem = if options.image_stem.is_empty() {
        if pct { "pct" } else { "pat" }.to_string()
    } else {
        options.image_stem.clone()
    };
    let format = if options.image_format.is_empty() {
        if pct { "jpg" } else { "tif" }.to_string()
    } else {
        options.image_format.clone()
    };
    (stem, format)
}

/// 도면 하나의 파일 형식. 문서별 기본값이 아니라 **그림마다 갈린다.**
///
/// 국내 제출 세트의 `.hlz` 는 `pat00001.jpg` 하나와
/// `pat00002.tif` ~ `pat00012.tif` 열하나를 섞어 담고 XML 의 `img-format` 도 따라 갈린다.
/// 판독기는 `tiff_geometry` 에서 `bits % 8 != 0` 을 거르므로
/// **1비트 그림은 압축 여부와 무관하게 픽셀을 못 낸다**(G4 압축이든
/// 무압축이든 1비트면 똑같이 못 푼다). 국내 문서는 대개 도면이 1비트고,
/// PCT 는 전부 8비트라 전부 `pct00001.jpg` 다.
///
/// 규칙: 풀리면 JPEG 로 굽고, 안 풀리면 원본 TIFF 를 그대로 싣는다.
/// 페이로드를 안 읽고 부른 경우(`export_kipo_xml_path`)와 호출자가 형식을 못 박은 경우는
/// 종전 기본값을 그대로 쓴다.
/// 그림 자리 `_` 를 <img/> 로. 자리 수가 그림 수와 같을 때만 자리마다, 아니면 끝에 붙인다.
fn place_images(markup: String, tags: &[String]) -> String {
    if markup.matches('_').count() != tags.len() {
        return markup + &tags.concat();
    }
    let mut out = String::with_capacity(markup.len() + 64 * tags.len());
    let mut tags = tags.iter();
    for ch in markup.chars() {
        if ch == '_' {
            out.push_str(tags.next().map(String::as_str).unwrap_or_default());
        } else {
            out.push(ch);
        }
    }
    out
}

/// `<img …/>` 한 장과 그 수출 기록. 통특기 속성 순서 id, he, wi, file, img-format.
#[allow(clippy::too_many_arguments)]
fn image_markup(
    document: &InspectDocument,
    image: &ImageOccurrence,
    image_stem: &str,
    image_format: &str,
    explicit_format: bool,
    import_convention: bool,
    ordinal: usize,
) -> (String, KipoExportImage) {
    let format = image_format_of(document, image.object_code, image_format, explicit_format);
    let file = format!("{image_stem}{ordinal:05}.{format}");
    let to_mm = if import_convention {
        display_units_to_mm_import
    } else {
        display_units_to_mm
    };
    let width_mm = to_mm(image.display_width).max(1);
    let height_mm = to_mm(image.display_height).max(1);
    let tag = format!(
        "<img id=\"i{ordinal:04}\" he=\"{height_mm}\" wi=\"{width_mm}\" file=\"{file}\" img-format=\"{format}\"/>"
    );
    (
        tag,
        KipoExportImage {
            image_id: image.id.clone(),
            file,
            width_mm,
            height_mm,
        },
    )
}

fn image_format_of(
    document: &InspectDocument,
    object_code: u32,
    default: &str,
    explicit: bool,
) -> String {
    if explicit {
        return default.to_string();
    }
    match document.image_payloads.get(&object_code) {
        Some(payload) if payload.pixels.is_empty() => "tif".to_string(),
        Some(_) => "jpg".to_string(),
        None => default.to_string(),
    }
}

/// 표제를 (요소, 상위 요소)로 옮긴다. 상위가 있으면 그 요소는 상위 안에 중첩된다.
/// ⛔ KIPO 스키마는 `tech-problem`, `tech-solution`, `advantageous-effects` 를
/// `description` 의 직계 자식으로 받지 않는다. 셋 다 `summary-of-invention` 안이다
/// `【발명의 내용】` 은 자기 요소 없이 그 상위 요소를 여는 표제다.
fn container_element(marker: &str) -> Option<(&'static str, Option<&'static str>)> {
    const SUMMARY: Option<&'static str> = Some("summary-of-invention");
    const CITATIONS: Option<&'static str> = Some("citation-list");
    let title = marker.trim_matches(['【', '】']);
    Some(match title {
        "기술분야" => ("technical-field", None),
        // 실물 표제는 「발명의 배경이 되는 기술」이다. 「배경기술」만 받으면 그 절의 문단이
        // 앞 절(technical-field)에 조용히 딸려 들어간다.
        "배경기술" | "발명의 배경이 되는 기술" => ("background-art", None),
        "발명의 내용" => ("", SUMMARY),
        "해결하고자 하는 과제" => ("tech-problem", SUMMARY),
        "과제의 해결 수단" => ("tech-solution", SUMMARY),
        "발명의 효과" => ("advantageous-effects", SUMMARY),
        "도면의 간단한 설명" => ("description-of-drawings", None),
        "발명을 실시하기 위한 구체적인 내용" => ("description-of-embodiments", None),
        "부호의 설명" => ("reference-signs-list", None),
        "선행기술문헌" => ("", CITATIONS),
        "특허문헌" => ("patent-literature", CITATIONS),
        "비특허문헌" => ("non-patent-literature", CITATIONS),
        _ => return None,
    })
}

/// 절 안의 문단을 한 번호 문단으로 합치고 `<br/>` 로 잇는 절인가. 정본이 그렇게 쓴다.
fn joins_lines(element: &str) -> bool {
    matches!(element, "description-of-drawings" | "reference-signs-list")
}

fn is_marker(text: &str) -> bool {
    text.starts_with('【') && text.ends_with('】')
}

fn claim_number(marker: &str) -> String {
    marker.chars().filter(char::is_ascii_digit).collect()
}

/// 도 · 식 번호: 첫 숫자부터 이어지는 영숫자. 통특기는 가지 번호 글자를 그대로 둔다 — 【도 10a】 → `num="10a"`
/// 숫자만 남기면 10a · 10b · 10c 가 모두 10 이 되어 보정서 생성이 E-243(식별항목명 중복)으로 거절한다.
fn figure_number(text: &str) -> String {
    text.chars()
        .skip_while(|ch| !ch.is_ascii_digit())
        .take_while(char::is_ascii_alphanumeric)
        .collect()
}

fn push_paragraph(sections: &mut [Section], markup: String) -> Result<(), CoreError> {
    let section = sections
        .last_mut()
        .ok_or_else(|| export_error("no_section"))?;
    section.body.push_str(&markup);
    Ok(())
}

/// 모아 둔 줄을 한 번호 문단으로 합쳐 그 절에 넣는다.
/// 도면의 간단한 설명과 부호의 설명이 이 규칙을 쓴다.
fn flush_joined_lines(
    sections: &mut [Section],
    counter: &mut u32,
    lines: &mut Vec<String>,
    element: &str,
) {
    if lines.is_empty() || element.is_empty() {
        lines.clear();
        return;
    }
    *counter += 1;
    let joined = lines.join("<br/>\n");
    lines.clear();
    if let Some(section) = sections
        .iter_mut()
        .rev()
        .find(|section| section.element == element && section.body.is_empty())
    {
        section
            .body
            .push_str(&format!("<p num=\"{:04}\">{joined}</p>\n", *counter));
    }
}

fn flush_claim(
    claims: &mut String,
    list: &mut Vec<(String, String)>,
    open: &mut Option<String>,
    parts: &mut Vec<String>,
) {
    let Some(number) = open.take() else {
        parts.clear();
        return;
    };
    let text = parts.join("<br/>");
    parts.clear();
    list.push((number.clone(), text.clone()));
    claims.push_str(&format!(
        "<claim num=\"{number}\"><claim-text>{text}</claim-text></claim>\n"
    ));
}

fn table_markup(
    number: u64,
    title: String,
    cells: &[&CellRecord],
    paragraph_text: &str,
    cell_spans: &BTreeMap<&str, &CellFormatRecord>,
    cell_images: &BTreeMap<&str, Vec<String>>,
    cell_aligns: &BTreeMap<&str, &'static str>,
) -> Result<String, CoreError> {
    // 표 본문은 [셀 텍스트 ] * N +  가 행마다 반복되고, 마지막  뒤는 표 꼬리 문단이다.
    let body = paragraph_text
        .rsplit_once(ROW_END)
        .map(|(head, _)| head)
        .unwrap_or(paragraph_text);
    let mut row_texts: Vec<Vec<String>> = Vec::new();
    for row in body.split(ROW_END) {
        if row.trim().is_empty() {
            continue;
        }
        let mut parts: Vec<String> = row
            .split(CELL_END)
            .map(|cell| cell.trim().to_string())
            .collect();
        parts.pop(); // 각 셀이  로 끝나므로 마지막 조각은 항상 빈 꼬리다
        row_texts.push(parts);
    }

    // 열은 모든 행의 x 시작점 종류로 센다(첫 행에 병합 칸이 있으면 첫 행만으로는 모자란다 ·
    // pct_a 표 1: 첫 행 3칸, 실제 5열). 통특기 저장본은 x 가 1 단위로 흔들리므로 가까운 것은 합친다.
    let mut starts: Vec<u32> = Vec::new();
    let mut sorted: Vec<u32> = cells.iter().map(|cell| cell.start).collect();
    sorted.sort_unstable();
    for x in sorted {
        if starts.last().is_none_or(|last| x.abs_diff(*last) > 2) {
            starts.push(x);
        }
    }
    if starts.is_empty() {
        return Err(export_error("table_columns"));
    }
    let col_of = |x: u32| -> usize {
        starts
            .iter()
            .enumerate()
            .min_by_key(|(_, value)| value.abs_diff(x))
            .map(|(index, _)| index)
            .unwrap_or(0)
    };
    // 열 폭·정렬 = 그 열에서 시작하는 colspan 1 칸 중 첫 것(통특기 colspec 과 같다).
    let mut columns: Vec<u32> = vec![0; starts.len()];
    let mut column_cells: Vec<Option<&CellRecord>> = vec![None; starts.len()];
    for cell in cells {
        let column = col_of(cell.start);
        if cell.colspan <= 1 && column_cells[column].is_none() {
            columns[column] = cell.width;
            column_cells[column] = Some(cell);
        }
    }

    let mut out = format!(
        "<tables num=\"{number}\">
<table>
"
    );
    if !title.is_empty() {
        out.push_str(&format!(
            "<title>{title}</title>
"
        ));
    }
    // 표는 OASIS CALS 교환 모델이다. tgroup 에 그 네임스페이스를 직접 선언해야
    // KIPO 스키마(Epasl/INCLUDE/Schema/PatentCAFDOC.xsd)를 통과한다. 정본 .fin 과 같은 모양.
    out.push_str(&format!(
        "<tgroup xmlns='{OASIS_TABLE_NS}' cols=\"{}\">
",
        columns.len()
    ));
    for (index, width) in columns.iter().enumerate() {
        let align = column_cells[index]
            .and_then(|cell| cell_aligns.get(cell.id.as_str()))
            .copied()
            .unwrap_or("justify");
        out.push_str(&format!(
            "<colspec colnum=\"{0}\" align=\"{2}\" colname=\"col{0}\" colwidth=\"{1}\"/>
",
            index + 1,
            crate::cell_width_to_colwidth(*width),
            align
        ));
    }
    out.push_str(
        "<tbody>
",
    );
    // 세로 병합에 덮인 칸은 셀 목록에 없지만 행 글자에는 (빈) 자리로 남아 있다(pct_a 표 1 둘째 행).
    // 덮인 열을 세어 글자 자리를 건너뛰지 않으면 뒤 칸 글자가 한 칸씩 밀리고 서식 스팬이 어긋난다.
    let mut covered: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for cell in cells {
        if cell.rowspan > 1 {
            let column = col_of(cell.start);
            for row in cell.row + 1..cell.row + cell.rowspan {
                covered.entry(row).or_default().push(column);
            }
        }
    }
    let mut current: Option<u32> = None;
    let mut row_index = 0_usize;
    let mut text_index = 0_usize;
    let mut skipped: Vec<usize> = Vec::new();
    for cell in cells {
        if current != Some(cell.row) {
            if current.is_some() {
                out.push_str(
                    "</row>
",
                );
                row_index += 1;
            }
            current = Some(cell.row);
            text_index = 0;
            skipped.clear();
            out.push_str(
                "<row>
",
            );
        }
        let this_column = col_of(cell.start);
        if let Some(columns) = covered.get(&cell.row) {
            for column in columns {
                if *column < this_column && !skipped.contains(column) {
                    skipped.push(*column);
                    text_index += 1;
                }
            }
        }
        let text = row_texts
            .get(row_index)
            .and_then(|row| row.get(text_index))
            .cloned()
            .unwrap_or_default();
        text_index += 1;
        let column = col_of(cell.start) as u32 + 1;
        let span = if cell.colspan > 1 {
            format!(
                " namest=\"col{column}\" nameend=\"col{}\"",
                column + cell.colspan - 1
            )
        } else {
            format!(" colname=\"col{column}\"")
        };
        let morerows = if cell.rowspan > 1 {
            format!(" morerows=\"{}\"", cell.rowspan - 1)
        } else {
            String::new()
        };
        let mut inner = cell_markup(&text, cell_spans.get(cell.id.as_str()).copied());
        if let Some(tags) = cell_images.get(cell.id.as_str()) {
            inner = place_images(inner, tags);
        }
        let align = cell_aligns
            .get(cell.id.as_str())
            .copied()
            .unwrap_or("justify");
        // 세로 맞춤 1 → valign="middle"(0 이면 안 쓴다). 2 등은 확인 전이라 안 쓴다.
        let valign = if cell.vertical_align == 1 {
            " valign=\"middle\""
        } else {
            ""
        };
        out.push_str(&format!(
            "<entry align=\"{align}\"{valign}{span}{morerows}>{inner}</entry>
"
        ));
    }
    if current.is_some() {
        out.push_str(
            "</row>
",
        );
    }
    out.push_str(
        "</tbody>
</tgroup>
</table>
</tables>
",
    );
    Ok(out)
}

fn inline_markup(
    paragraph: &ParagraphRecord,
    spans: Option<&Vec<crate::CharacterFormatSpan>>,
) -> String {
    // 통특기는 문단 글자를 그대로 쓴다(뒤 공백 `있다. </p>` · 공백 한 칸 문단 `<p> </p>`).
    let text: Vec<char> = paragraph.text.trim_matches(PAGE_BREAK).chars().collect();
    let Some(spans) = spans.filter(|spans| !spans.is_empty()) else {
        return escape(&text.iter().collect::<String>());
    };
    let offset = paragraph
        .text
        .chars()
        .position(|value| value != PAGE_BREAK)
        .unwrap_or(0);
    apply_spans(&text, spans, offset)
}

/// 표 뒤 꼬리 문단. 표 문단은 `inline_markup` 을 거치지 않아 꼬리가 서식을 통째로 잃고 있었다.
/// ⛔ 서식 스팬 좌표는 **칸 종결자와 행 종결자를 뺀** 글자열 기준이다(네이티브 PCT
/// 표 문단에서 꼬리 스팬이 종결자 24개만큼 어긋난다). 그래서 지나온 종결자 수를 빼야 한다.
fn table_tail_markup(
    paragraph: &ParagraphRecord,
    spans: Option<&Vec<crate::CharacterFormatSpan>>,
    tail: &str,
) -> String {
    let text: Vec<char> = tail.chars().collect();
    let Some(spans) = spans.filter(|spans| !spans.is_empty()) else {
        return escape(tail);
    };
    let chars: Vec<char> = paragraph.text.chars().collect();
    let body_len = chars
        .iter()
        .rposition(|value| *value == ROW_END)
        .map_or(0, |index| index + 1);
    let terminators = chars[..body_len]
        .iter()
        .filter(|value| **value == CELL_END || **value == ROW_END)
        .count();
    let lead = chars[body_len..]
        .iter()
        .position(|value| *value != PAGE_BREAK && !value.is_whitespace())
        .unwrap_or(0);
    apply_spans(&text, spans, body_len + lead - terminators)
}

/// 스팬을 글자에 입힌다. `offset` 은 이 글자열의 첫 글자가 서식 좌표계에서 갖는 위치다.
fn apply_spans(text: &[char], spans: &[crate::CharacterFormatSpan], offset: usize) -> String {
    let mut out = String::new();
    let mut cursor = 0_usize;
    for span in spans {
        let start = (span.start as usize).saturating_sub(offset);
        // 판독기가 문단 글자의 꼬리 공백을 잘라내므로 스팬이 한 글자 넘칠 수 있다.
        // 통째로 버리지 말고 글자 길이로 자른다.
        let end = (span.end as usize).saturating_sub(offset).min(text.len());
        if start >= text.len() || start >= end || start < cursor {
            continue;
        }
        out.push_str(&escape(&text[cursor..start].iter().collect::<String>()));
        let inner = escape(&text[start..end].iter().collect::<String>());
        out.push_str(&wrap_formats(&span.formats, &inner));
        cursor = end;
    }
    out.push_str(&escape(&text[cursor..].iter().collect::<String>()));
    out
}

/// 표 칸 글자에 문자 서식을 입힌다. 판독기가 낸 칸 글자(`record.text`)와 여기서 쓰는
/// 칸 글자(본문 문단을 셀 구분자로 쪼갠 것)는 앞뒤 공백만 다르므로, 그 어긋난 만큼만
/// 스팬 오프셋을 당긴다. 두 글자열이 그것으로도 안 맞으면 서식을 포기하고 원문을 그대로 쓴다.
fn cell_markup(text: &str, record: Option<&CellFormatRecord>) -> String {
    let Some(record) = record else {
        return escape(text);
    };
    let Some(offset) = record.text.find(text) else {
        return escape(text);
    };
    let offset = record.text[..offset].chars().count();
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut cursor = 0_usize;
    for span in &record.spans {
        let start = (span.start as usize).saturating_sub(offset);
        let end = (span.end as usize).saturating_sub(offset);
        if start >= chars.len() || end > chars.len() || start >= end || start < cursor {
            continue;
        }
        out.push_str(&escape(&chars[cursor..start].iter().collect::<String>()));
        let inner = escape(&chars[start..end].iter().collect::<String>());
        out.push_str(&wrap_formats(&span.formats, &inner));
        cursor = end;
    }
    out.push_str(&escape(&chars[cursor..].iter().collect::<String>()));
    out
}

/// 탭은 서식 태그 밖으로 낸다(통특기 제출 XML: `<sub>2</sub>\t` — 탭 글자에 아래첨자가 걸려 있어도).
fn wrap_formats(formats: &[CharacterFormat], inner: &str) -> String {
    inner
        .split('\t')
        .map(|piece| {
            if piece.is_empty() {
                String::new()
            } else {
                wrap_run(formats, piece)
            }
        })
        .collect::<Vec<_>>()
        .join("\t")
}

fn wrap_run(formats: &[CharacterFormat], inner: &str) -> String {
    // 중첩 순서는 통특기 실물대로 안쪽부터 sub/sup, u, i, b (pct_a_nk.fin: <b><sup>1</sup></b>).
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
            CharacterFormat::Bold => "b",
            CharacterFormat::Italic => "i",
            CharacterFormat::Underline => "u",
            CharacterFormat::Superscript => "sup",
            CharacterFormat::Subscript => "sub",
        };
        out = format!("<{tag}>{out}</{tag}>");
    }
    out
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for value in text.chars() {
        match value {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            CELL_END | ROW_END | PAGE_BREAK => {}
            _ => out.push(value),
        }
    }
    out
}

fn export_error(reason: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::StructureInvalid, "export_kipo", false);
    error.details.insert("reason".into(), reason.into());
    error
}

/// HLT 경로 하나로 판독, 서식 수집, XML 작성까지 한 번에 한다.
pub fn export_kipo_xml_path(
    path: &Path,
    options: &KipoExportOptions,
) -> Result<KipoExport, CoreError> {
    // 그림 형식(jpg/tif)은 그림마다 픽셀을 풀 수 있는지로 갈린다 — 픽셀 없이 읽으면 국내 문서는 전부 tif 가 된다
    // (화학식 그림: 통특기 제출 변환은 pat00001.jpg · fin 경로와 같은 판정).
    let document = inspect_path(
        path,
        InspectOptions {
            include_image_identities: false,
            include_image_payloads: true,
            ..InspectOptions::default()
        },
    )?;
    // 의견서(문서 종류 7)는 뿌리부터 다른 방언(`OpinionDOC` · 대문자 태그 · 쪽 표시)이다.
    if crate::export_opinion::is_opinion(&document) {
        return crate::export_opinion::export_opinion_xml_path(
            path,
            &with_computed_page_count(path, options)?,
        );
    }
    let (formats, cell_formats) = inspect_format_inventories(path)?;
    export_kipo_xml(
        &document,
        &formats,
        &cell_formats,
        &with_computed_page_count(path, options)?,
    )
}

/// 통특기는 pageCount 를 늘 쓴다 = 변환 때 제 배치로 센 쪽 수 — 저장된 꼬리 #14 가 아니다(opinion3: 헤드리스 DLL 저장본이라
/// #14 = 1 인데 통특기 XML 은 9 · 나머지 6건은 둘이 같다). 우리 쪽 매김(P3 `page_count`, 인쇄 정답 6/6)으로 센다. 옵션이 비면 채운다.
pub(crate) fn with_computed_page_count(
    path: &Path,
    options: &KipoExportOptions,
) -> Result<KipoExportOptions, CoreError> {
    let mut options = options.clone();
    if options.page_count.is_none() {
        let (_, inflated) = crate::inflate_hlt(path)?;
        let document = inspect_path(
            path,
            InspectOptions {
                include_image_payloads: true,
                ..InspectOptions::default()
            },
        )?;
        options.page_count = Some(crate::page_count(&document, &inflated) as u32);
    }
    Ok(options)
}

#[cfg(test)]
mod figure_number_tests {
    #[test]
    fn keeps_branch_letters() {
        assert_eq!(super::figure_number("【도 10a】"), "10a");
        assert_eq!(super::figure_number("【도 12】"), "12");
        assert_eq!(super::figure_number("도 3b"), "3b");
        assert_eq!(super::figure_number("【수학식 7】"), "7");
    }
}
