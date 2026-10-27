use crate::{
    CoreError, ErrorCode, GeneratedImagePayload, GeneratedLineEnding, GenerationDialect,
    GenerationProfile, HLT_HEADER_LEN, HmlDocument, generate_cell_table, generate_font_table,
    generate_line_array, generate_section_register, generation_profile, plan_hlt_body,
    plan_hlt_characters, plan_hlt_images, plan_table_tail,
};
use encoding_rs::EUC_KR;
use std::collections::BTreeMap;

const CHARACTER_MARKER: &[u8] = b"~`!@#$%^&*()-+|=-TeCfMt\r\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedHltDocument {
    pub header: [u8; HLT_HEADER_LEN as usize],
    pub inflated: Vec<u8>,
    pub section_count: u32,
    pub line_count: u32,
    pub span_count: u32,
    pub table_count: u32,
    pub image_count: u32,
}

pub fn generate_hlt_document(
    document: &HmlDocument,
    dialect: GenerationDialect,
    payloads: &[GeneratedImagePayload],
) -> Result<GeneratedHltDocument, CoreError> {
    generate_hlt_document_with_profile(document, dialect, &generation_profile(dialect), payloads)
}

/// 프로필을 직접 준다 — 원본 틀 프로필(`source_generation_profile`)이면 그 파일의 헤더·모양 표·쪽 설정으로 굽는다.
/// 본문 규칙(줄나눔 · 표 꼬리 상수)은 `dialect` 를 따른다.
pub fn generate_hlt_document_with_profile(
    document: &HmlDocument,
    dialect: GenerationDialect,
    profile: &GenerationProfile,
    payloads: &[GeneratedImagePayload],
) -> Result<GeneratedHltDocument, CoreError> {
    let mut body = plan_hlt_body(document, dialect)?;
    let mut profile = std::borrow::Cow::Borrowed(profile);
    if profile.shape_map.is_some() {
        add_alignment_shapes(profile.to_mut(), document);
    }
    let profile = profile.as_ref();
    if let Some(map) = &profile.shape_map {
        remap_shapes(&mut body, document, map);
    }
    if body.lines.is_empty() {
        return Err(generation_error("empty_document"));
    }
    let line_count =
        u32::try_from(body.lines.len()).map_err(|_| generation_error("line_count_overflow"))?;
    let characters = plan_hlt_characters(&body)?;
    let images = plan_hlt_images(document, &body, &characters, payloads)?;
    let table_tail = plan_table_tail(document, dialect, &body.cell_ranges)?;
    let cell_table = generate_cell_table(
        &table_tail.rows,
        &table_tail.terminal_rows,
        dialect == GenerationDialect::PctPatent,
    )?;
    let font_table = generate_font_table(
        &table_tail.cells,
        &table_tail.terminal_cells,
        dialect == GenerationDialect::PctPatent,
    )?;
    let line_array = generate_line_array(document, &body)?;
    let title_indices = section_title_indices(document, &body)?;
    // Built-in new-document profiles do not contain the separate paragraph-information
    // objects that nonzero WIPO IDs reference. Emitting dangling IDs makes NKEditor report
    // "문단 정보가 없습니다", so only verified section slots are serialized here.
    // 보정 명세서(종류 111)만은 식별번호를 싣는다 — 보정서의 문단 번호가 이것이다.
    let document_type = u32::from_le_bytes(profile.header[0x20..0x24].try_into().expect("type"));
    let wipo_ids = if document_type & 0x7fff == crate::AMENDMENT_DOCUMENT_TYPE {
        amendment_wipo_ids(document, &body)
    } else {
        vec![None; body.lines.len()]
    };
    let register = generate_section_register(&title_indices, &wipo_ids, true)?;
    let geometry = patched_geometry(&profile.geometry, line_count)?;

    let mut header = profile.header;
    patch_header_title(&mut header, document)?;
    let mut inflated = vec![0; 0x2c];
    inflated.extend_from_slice(&body.section_table);
    inflated.extend_from_slice(&profile.pre_body_gap);
    inflated.extend_from_slice(&body.body);
    inflated.extend_from_slice(CHARACTER_MARKER);
    inflated.extend_from_slice(&images.spans);
    inflated.extend_from_slice(&images.object_table);
    inflated.push(0xbf);
    inflated.extend_from_slice(&line_array);
    inflated.push(0xbf);
    inflated.extend_from_slice(&profile.shape_count.to_le_bytes());
    inflated.extend_from_slice(&[0_u8; 8]);
    inflated.extend_from_slice(&profile.shapes);
    inflated.extend_from_slice(&profile.shape_order);
    inflated.extend_from_slice(&geometry);
    inflated.extend_from_slice(&cell_table);
    inflated.extend_from_slice(&font_table);
    inflated.extend_from_slice(&profile.second_shape_order);
    inflated.extend_from_slice(&profile.style_table);
    inflated.extend_from_slice(&register);
    inflated.extend_from_slice(&profile.ending);

    Ok(GeneratedHltDocument {
        header,
        inflated,
        section_count: u32::try_from(document.sections.len())
            .map_err(|_| generation_error("section_count_overflow"))?,
        line_count,
        span_count: line_count,
        table_count: u32::try_from(document.tables.len())
            .map_err(|_| generation_error("table_count_overflow"))?,
        image_count: u32::try_from(document.images.len())
            .map_err(|_| generation_error("image_count_overflow"))?,
    })
}

fn section_title_indices(
    document: &HmlDocument,
    body: &crate::HltBodyPlan,
) -> Result<Vec<u32>, CoreError> {
    // ⛔ 줄지도 슬롯은 **절 테이블 레코드와 1:1** 이다(실물 71건 전수 71/71). 슬롯 값은 `section_table` 이 쓰는
    // 레코드 **자리**(i+1)다 — 원본 idx 가 아니다. 통특기는 나중에 넣은 절(【서열목록】)에 max+1 idx 를 주고 표 가운데에
    // 끼우므로 원본 idx 를 그대로 찍으면 그 뒤 절이 하나씩 밀리고 마지막 절은 앵커를 잃었다.
    // 앵커 = 그 절의 첫 문단(`generate_body` 의 앵커 규칙과 같다). 표제 문단이 아닐 수 있다 — 【서열목록】 절은
    // 표제 다음 줄(서열목록 전자파일 첨부)에 걸린다.
    // 표제를 달았는데 레코드가 없는 문단(정본 `domestic_출원서` 의 `【도 12】`)은 어느 절의 첫 문단도 아니라 슬롯이 없다.
    // 한 문단이 레코드 여럿의 첫 문단이면 그 문단의 줄에 차례로 하나씩 싣는다.
    // ⚠️ `amend_a_의견서` 처럼 산문 한 문단의 줄마다 레코드가 붙은 문서는 우리 의미 모델로 재현하지 못한다(그대로 둔다).
    let mut anchors: BTreeMap<&str, std::collections::VecDeque<u32>> = BTreeMap::new();
    for (position, section) in document.sections.iter().enumerate() {
        if let Some(first) = section.paragraph_ids.first() {
            let slot = u32::try_from(position + 1)
                .map_err(|_| generation_error("section_count_overflow"))?;
            anchors.entry(first.as_str()).or_default().push_back(slot);
        }
    }
    Ok(body
        .lines
        .iter()
        .map(|line| {
            if line.ending == GeneratedLineEnding::PageBreak {
                return 0;
            }
            anchors
                .get_mut(line.paragraph_id.as_str())
                .and_then(|slots| slots.pop_front())
                .unwrap_or(0)
        })
        .collect())
}

fn patch_header_title(
    header: &mut [u8; HLT_HEADER_LEN as usize],
    document: &HmlDocument,
) -> Result<(), CoreError> {
    let title = if document.title.trim().is_empty() {
        document
            .sections
            .first()
            .map(|section| section.title.as_str())
            .unwrap_or("")
    } else {
        document.title.as_str()
    };
    let (encoded, _, bad) = EUC_KR.encode(title);
    if bad || encoded.len() > 40 {
        return Err(CoreError::new(
            ErrorCode::Cp949Invalid,
            "generate_header",
            false,
        ));
    }
    header[0x24..0x24 + 40].fill(0);
    header[0x24..0x24 + encoded.len()].copy_from_slice(&encoded);
    Ok(())
}

/// 원본 틀로 굽을 때 줄마다 모양 번호를 원본 모양 표에서 고른다 — 그 줄 문단(표 칸이면 칸)의 (들여쓰기, 정렬, 줄 간격) 그대로.
/// 같은 것이 없으면 같은 정렬에서 줄 간격 · 들여쓰기가 가장 가까운 것, 그것도 없으면 아무 정렬에서 가장 가까운 것.
fn align_code(align: &str) -> u32 {
    match align {
        "center" => 1,
        "left" => 1024,
        _ => 2048,
    }
}

/// 원본 틀 모양 표에 없는 정렬은 정렬만 바꾼 모양을 표 끝에 덧붙인다. 통특기도 그렇게 한다(opinion4: 의견서 틀의
/// 양쪽 정렬 3개 뒤에 「왼쪽 + 800」). 같은 (들여쓰기, 줄 간격) 모양이 없으면 덧붙이지 않고 가까운 모양으로 둔다.
fn add_alignment_shapes(profile: &mut GenerationProfile, document: &HmlDocument) {
    let wanted = document
        .paragraphs
        .iter()
        .map(|p| (p.indent_raw, align_code(&p.align), p.lspace))
        .chain(document.cells.iter().map(|c| {
            (
                c.indent_raw,
                align_code(&c.align),
                crate::generate_body::TABLE_CELL_LSPACE,
            )
        }));
    for (indent, align, lspace) in wanted {
        let map = profile.shape_map.as_ref().expect("shape map");
        if map
            .iter()
            .any(|shape| (shape.0, shape.1 & 0xfff, shape.2) == (indent, align, lspace))
        {
            continue;
        }
        let Some(&(_, _, _, index)) = map
            .iter()
            .find(|shape| (shape.0, shape.2) == (indent, lspace))
        else {
            continue;
        };
        // 레코드 i 는 모양 바이트 72i-8 부터 72바이트(앞 8바이트는 파일에서 개수 뒤 0) · +12 = 정렬.
        let at = 72 * index as usize - 8;
        let mut record = profile.shapes[at..at + 72].to_vec();
        record[12..16].copy_from_slice(&align.to_le_bytes());
        profile.shapes.to_mut().extend_from_slice(&record);
        let added = profile.shape_count;
        profile.shape_count += 1;
        let map = profile.shape_map.as_mut().expect("shape map");
        map.push((indent, align, lspace, added));
    }
}

fn remap_shapes(
    body: &mut crate::HltBodyPlan,
    document: &HmlDocument,
    map: &[(i32, u32, u32, u32)],
) {
    let paragraphs: BTreeMap<&str, (i32, u32, u32)> = document
        .paragraphs
        .iter()
        .map(|p| {
            (
                p.id.as_str(),
                (p.indent_raw, align_code(&p.align), p.lspace),
            )
        })
        .collect();
    let cells: BTreeMap<&str, (i32, u32, u32)> = document
        .cells
        .iter()
        .map(|c| {
            (
                c.id.as_str(),
                (
                    c.indent_raw,
                    align_code(&c.align),
                    crate::generate_body::TABLE_CELL_LSPACE,
                ),
            )
        })
        .collect();
    for line in &mut body.lines {
        let wanted = match &line.cell_id {
            Some(cell) => cells.get(cell.as_str()),
            None => paragraphs.get(line.paragraph_id.as_str()),
        };
        let Some(&(indent, align, lspace)) = wanted else {
            continue;
        };
        // HLT 에서 읽은 모델의 들여쓰기는 모양 값 그대로다(799 · 800 — `shape_index` 의 0.8 환산은 다른 입력용).
        let distance =
            |shape: &&(i32, u32, u32, u32)| (shape.2.abs_diff(lspace), shape.0.abs_diff(indent));
        let chosen = map
            .iter()
            .find(|shape| (shape.0, shape.1 & 0xfff, shape.2) == (indent, align, lspace))
            .or_else(|| {
                map.iter()
                    .filter(|shape| shape.1 & 0xfff == align)
                    .min_by_key(distance)
            })
            .or_else(|| map.iter().min_by_key(distance));
        if let Some(shape) = chosen {
            line.shape_index = shape.3;
        }
    }
}

fn patched_geometry(source: &[u8], line_count: u32) -> Result<Vec<u8>, CoreError> {
    let mut geometry = source.to_vec();
    let marker = [0x3f, 0xe9, 0x7b, 0x3f];
    let offset = geometry
        .windows(marker.len())
        .position(|window| window == marker)
        .ok_or_else(|| generation_error("geometry_marker"))?;
    let field = offset
        .checked_add(28)
        .ok_or_else(|| generation_error("geometry_offset"))?;
    if field + 4 > geometry.len() {
        return Err(generation_error("geometry_range"));
    }
    geometry[field..field + 4].copy_from_slice(&(line_count - 1).to_le_bytes());
    Ok(geometry)
}

fn generation_error(reason: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::StructureInvalid, "generate_document", false);
    error.details.insert("reason".into(), reason.into());
    error
}

/// 생성 입력(`.docx` · KIPO `.xml` · `.hml` · `.HLT`/`.HBK`) → 의미 모델 + 그림 원본. 확장자로 고른다.
/// CLI `generate-hlt` 와 편집기 「불러와 HLT 만들기」가 같은 길을 쓴다.
/// 글은 CP949 로 쓸 수 있게 바꿔서 낸다(바꾼 글자 목록 · 대응 없는 글자는 거부 — [`crate::make_cp949_safe`]).
pub fn load_generation_input(
    input: &std::path::Path,
) -> Result<
    (
        HmlDocument,
        Vec<GeneratedImagePayload>,
        Vec<crate::Cp949Substitution>,
    ),
    CoreError,
> {
    let (mut document, payloads) = load_generation_source(input)?;
    let substitutions = crate::make_cp949_safe(&mut document)?;
    Ok((document, payloads, substitutions))
}

/// [`load_generation_input`] 에서 CP949 바꾸기를 뺀 원본 — 확인 표([`crate::scan_cp949`])를 먼저 보여 줄 때.
pub fn load_generation_source(
    input: &std::path::Path,
) -> Result<(HmlDocument, Vec<GeneratedImagePayload>), CoreError> {
    let extension = input
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    // `Path::new("a.xml").parent()` 는 `Some("")` 이라 canonicalize 가 죽는다(파일명만 준 입력).
    let root = match input.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => std::path::Path::new("."),
    };
    match extension.as_str() {
        "hml" => {
            let document = crate::inspect_hml_path(input)?;
            let payloads = crate::load_hml_image_payloads_from_directory(&document, root)?;
            Ok((document, payloads))
        }
        "xml" => {
            let parsed = crate::inspect_kipo_xml_path(input)?;
            let payloads = crate::load_hml_image_payloads_from_directory(&parsed.document, root)?;
            Ok((parsed.document, payloads))
        }
        "docx" => {
            let parsed = crate::inspect_docx_path(input)?;
            let payloads = crate::load_docx_image_payloads(input, &parsed)?;
            Ok((parsed.document, payloads))
        }
        // 이미 받은 HLT 안의 표를 고치는 길. 역변환기가 의미 모델과 그림 원본을 함께 낸다.
        "hlt" | "hbk" => crate::hml_document_from_hlt(input),
        _ => {
            let mut error = CoreError::new(ErrorCode::UnsupportedFormat, "generate_input", false);
            error.source = input
                .file_name()
                .and_then(|value| value.to_str())
                .map(str::to_string);
            Err(error)
        }
    }
}

/// 보정 명세서의 줄마다 식별번호(통특기 「최종본 불러오기」 HLT 와 줄마다 같다 · 실측 328/328):
/// 문단 첫 줄에만 · 번호 없는 줄(명칭 · 도면 설명 · 청구항 · 식 그림) = 0 · 새 문단(`+`) = 0xFFFFFFFF ·
/// 【표 N】·【수학식 N】·【화학식 N】 표제가 번호를 든다(들여올 때는 다음 문단에 달려 온다 — 표제로 옮긴다).
fn amendment_wipo_ids(document: &HmlDocument, body: &crate::HltBodyPlan) -> Vec<Option<u32>> {
    let parse = |number: Option<&str>| match number? {
        "+" => Some(u32::MAX),
        number => number.parse().ok(),
    };
    let mut numbers: BTreeMap<&str, Option<u32>> = BTreeMap::new();
    let paragraphs = &document.paragraphs;
    let mut moved: Option<usize> = None;
    for (index, paragraph) in paragraphs.iter().enumerate() {
        let text = paragraph.text.trim_matches('\u{c}').trim();
        let carrier = ["【표 ", "【수학식 ", "【화학식 "]
            .iter()
            .any(|head| text.starts_with(head));
        let number = if carrier && paragraph.number.is_none() {
            let next = paragraphs[index + 1..]
                .iter()
                .position(|next| next.number.is_some())
                .map(|offset| index + 1 + offset);
            moved = next;
            next.and_then(|next| parse(paragraphs[next].number.as_deref()))
        } else if moved == Some(index) {
            None
        } else {
            parse(paragraph.number.as_deref())
        };
        numbers.insert(paragraph.id.as_str(), number);
    }
    let mut previous: Option<&str> = None;
    body.lines
        .iter()
        .map(|line| {
            if line.text == "\x0c" {
                return None;
            }
            let first = line.cell_id.is_none() && previous != Some(line.paragraph_id.as_str());
            previous = Some(line.paragraph_id.as_str());
            if first {
                numbers.get(line.paragraph_id.as_str()).copied().flatten()
            } else {
                None
            }
        })
        .collect()
}
