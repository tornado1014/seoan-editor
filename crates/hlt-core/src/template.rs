//! 통특기 서식(`Epasl/FRM/*.frm`)으로 「새 문서」를 만든다.
//!
//! 서식은 `.HLT` 템플릿이다. 통특기는 그것을 `[제목없음]` 으로 열고, 저장할 때 몇 군데만 고쳐
//! 현행 판(0x33)으로 쓴다. 템플릿의 나머지 바이트(본문·모양·스타일 표)는 그대로 둔다 —
//! 생성기(`generate`)는 2026 서식 프로필을 내장해 템플릿 자신의 표를 버리므로 이 길에 맞지 않는다.

use std::{
    collections::BTreeMap,
    io::{Read, Write},
    ops::Range,
    path::Path,
};

use encoding_rs::EUC_KR;
use flate2::read::DeflateDecoder;

use crate::{
    CoreError, ErrorCode, HLT_HEADER_LEN, HLT_MAGIC, InspectOptions,
    inspect::{cell_record_size, inspect_edit_layout, locate_tail, register_paired},
    reflow::{self, Item},
};

type Header = [u8; HLT_HEADER_LEN as usize];

/// 헤더 0x15 — TER 포맷 판. 통특기 라이터는 늘 0x33 을 쓴다.
const VERSION: usize = 0x15;
/// 헤더 0x17 — 1 이면 deflate.
const COMPRESSED: usize = 0x17;
/// 헤더 0x20..0x24 — 문서 종류. 서식은 0x8000(템플릿 표지)이나 0x8·0xc 를 싣고, 새 문서는 0 이다.
const DOCUMENT_TYPE: Range<usize> = 0x20..0x24;
/// 꼬리 #16 의 앞 두 float = 용지 폭·높이(인치). 새 문서는 A4 다(210/25.4, 297/25.4).
const A4_INCHES: [u8; 8] = [0x91, 0x48, 0x04, 0x41, 0x2c, 0x16, 0x3b, 0x41];
/// 기하 표(TER 표 #6, 68B 레코드) +4..+20 = 쪽 여백 f32 인치 넷. 통특기는 0 이면 25·20·40·20mm 를 채운다.
/// 서식 32건: 0→기본 9 · 기본→기본 23. 0 도 기본도 아닌 서식이 없어 「항상 덮어쓰기」와는 아직 못 가른다.
const DEFAULT_MARGINS: [u8; 16] = [
    0x3f, 0xe9, 0x7b, 0x3f, 0x17, 0x6c, 0x49, 0x3f, 0x9a, 0x99, 0xc9, 0x3f, 0x17, 0x6c, 0x49, 0x3f,
];
const GEOMETRY_RECORD: usize = 68;
/// 문단모양 표(줄 배열 뒤 `bf n`) 레코드 — +0 왼쪽 · +4 오른쪽 · +8 첫 줄 들여쓰기 · +40 줄 간격.
const PARAGRAPH_SHAPE_RECORD: usize = 72;
/// R16 서열목록 줄 간격.
const LINE_SPACING_SEQUENCE: u32 = 130;
const CHAR_SHAPE_RECORD: usize = 100;
/// 표의 행 끝 표지.
const ROW_END: u8 = 0x12;
/// 회색(c0c0c0) Symbol 9pt 글자모양 — 서식 87건의 1번 레코드 그대로(R12).
const GRAY_SYMBOL: [u8; CHAR_SHAPE_RECORD] = {
    let mut record = [0_u8; CHAR_SHAPE_RECORD];
    record[4] = 9;
    let name = *b"Symbol";
    let mut i = 0;
    while i < name.len() {
        record[8 + i] = name[i];
        i += 1;
    }
    record[39] = 0x10;
    record[44] = 0xc0;
    record[45] = 0xc0;
    record[46] = 0xc0;
    record[64] = 0xff;
    record[65] = 0xff;
    record[66] = 0xff;
    record[80] = 1;
    record
};
const TAG: u8 = 0xbf;
/// 절 표: 0x2c 부터 67B 레코드(제목 51 · 길이 · 번호 · 다음 · 이전), index 0xFFFFFFFF 종결 레코드, 곧바로 본문.
const SECTION_START: usize = 0x2c;
const SECTION_RECORD: usize = 67;
/// 제목은 이 길이에서 자른다(서식 TradMark1).
const SECTION_TITLE_CAP: usize = 50;
const OPEN: [u8; 2] = [0xa1, 0xbc]; // 【
const CLOSE: [u8; 2] = [0xa1, 0xbd]; // 】
/// R3 문턱 목록(NKEditor 0x521168). 이 중 하나로 시작하는 첫 제목부터 끝까지 전부 절이 된다.
/// 띄어쓰기까지 그대로다 — `change1` 의 【보정대상항목】은 `보정대상 항목` 과 안 맞아 절이 0 이다.
const SECTION_LATCH: &str = "요약서|명세서|발명의 설명|고안의 설명|도면|상품견본|입체디자인 도면|평면디자인 도면|\
글자체디자인 도면|청구의 이유|청구의 취지|청구취지|답변의 취지|의견내용|증거|진술내용|신청이유|재답변내용|답변이유|\
정관의 요약서|정관(규약)의 요약서|답변내용|보정대상 항목|DESCRIPTION";

pub struct NewDocument {
    pub header: Header,
    pub inflated: Vec<u8>,
}

/// 한 문서의 판독 지도. 다른 문서와 같은 자리끼리 대조하는 데 쓴다.
#[derive(Debug, Clone)]
pub struct DocumentRegions {
    /// 절 표(0x2c~)·종료 레코드·간격이 끝나고 본문이 시작하는 곳.
    pub body_start: usize,
    /// 본문 끝(글자 표 표지 앞).
    pub body_end: usize,
    /// 글자 표(줄마다 `개수 u32` + 글자별 객체 번호 u32) 시작.
    pub char_base: usize,
    /// 객체 표(`bf n` + 글자모양 100B·그림 레코드) 태그 위치.
    pub object_table: usize,
    pub total_lines: usize,
    /// 기하 표 태그 위치(68B 레코드).
    pub geometry: usize,
    /// 셀 표(TER 표 B) 태그 위치.
    pub cell_table: usize,
    /// 등록부 태그 위치(`bf 00 00 00 00` + 슬롯 2n-1 개).
    pub register: usize,
    /// 꼬리 #14 `bf n` + n×u32 — 렌더 쪽 수에 따라 바뀐다(레이아웃 엔진 전까지 판정 보류).
    pub tail14: Range<usize>,
    /// 꼬리 #15 `bf` + 2×u32 (9B).
    pub tail15: usize,
    /// 꼬리 #16 `bf` + 4B 값 6개(앞 둘 = 용지 폭·높이 f32 인치), 뒤에 `ba`.
    pub tail16: usize,
}

/// `.HLT`(또는 `.frm`) 파일을 헤더와 해제본으로 연다. 서식의 두 변종을 받는다:
/// - 압축(0x17 = 1): deflate 뒤 바로 꼬리(CRC·크기)를 대조한다. 꼬리 **뒤** 에 줄바꿈만 붙은 서식(`SpIdea1`, `\r\n`×4)은
///   받고, 그 밖의 바이트가 붙으면 거부한다.
/// - 무압축(0x17 = 0): 헤더 뒤 끝까지가 해제본이고 꼬리가 없다(`PctPatE`·`PctPatED`) — 무결성을 대조할 수 없다.
pub fn inflate_hlt(path: &Path) -> Result<(Header, Vec<u8>), CoreError> {
    let bytes = std::fs::read(path).map_err(|_| error(ErrorCode::IoError, "template_open"))?;
    let header: Header = bytes
        .get(..HLT_HEADER_LEN as usize)
        .and_then(|head| head.try_into().ok())
        .filter(|head: &Header| head.starts_with(HLT_MAGIC))
        .ok_or_else(|| error(ErrorCode::HeaderInvalid, "template_header"))?;
    let stream = &bytes[HLT_HEADER_LEN as usize..];
    match header[COMPRESSED] {
        0 => Ok((header, stream.to_vec())),
        1 => {
            let mut decoder = DeflateDecoder::new(stream);
            let mut inflated = Vec::new();
            decoder
                .read_to_end(&mut inflated)
                .map_err(|_| error(ErrorCode::DeflateInvalid, "template_inflate"))?;
            let rest = &stream[decoder.total_in() as usize..];
            let footer = rest
                .get(..8)
                .ok_or_else(|| error(ErrorCode::FooterSizeMismatch, "template_footer"))?;
            let crc = u32::from_le_bytes(footer[..4].try_into().unwrap());
            let size = u32::from_le_bytes(footer[4..].try_into().unwrap());
            if crc32fast::hash(&inflated) != crc {
                return Err(error(ErrorCode::FooterCrcMismatch, "template_footer"));
            }
            if size as usize != inflated.len() {
                return Err(error(ErrorCode::FooterSizeMismatch, "template_footer"));
            }
            if !rest[8..].iter().all(|byte| matches!(byte, b'\r' | b'\n')) {
                return Err(error(ErrorCode::DeflateTrailingData, "template_footer"));
            }
            Ok((header, inflated))
        }
        _ => Err(error(ErrorCode::UnsupportedFormat, "template_compression")),
    }
}

pub fn document_regions(header: &Header, inflated: &[u8]) -> Result<DocumentRegions, CoreError> {
    let mut spool = tempfile::tempfile().map_err(|_| error(ErrorCode::IoError, "regions_spool"))?;
    spool
        .write_all(inflated)
        .map_err(|_| error(ErrorCode::IoError, "regions_spool"))?;
    let size = inflated.len() as u64;
    let edit = inspect_edit_layout(&mut spool, size)?;
    let tail = locate_tail(
        &mut spool,
        size,
        edit.line_base,
        edit.total_lines,
        cell_record_size(header)?,
    )?;
    let register = tail.register as usize;
    let total_lines = edit.total_lines as usize;
    let slots_per_line = if register_paired(header) { 2 } else { 1 };
    let tail14_start = register + 1 + 4 * slots_per_line * total_lines;
    let count = tagged_count(inflated, tail14_start, "tail14")?;
    let tail15 = tail14_start + 5 + 4 * count;
    if inflated.get(tail15) != Some(&TAG) {
        return Err(error(ErrorCode::StructureInvalid, "tail15"));
    }
    let tail16 = tail15 + 9;
    if inflated.get(tail16) != Some(&TAG) || inflated.get(tail16 + 25) != Some(&0xba) {
        return Err(error(ErrorCode::StructureInvalid, "tail16"));
    }
    Ok(DocumentRegions {
        body_start: edit.body_start as usize,
        body_end: edit.body_end as usize,
        char_base: edit.char_base as usize,
        object_table: edit.object_table as usize,
        total_lines,
        geometry: tail.shape_order.end_byte as usize,
        cell_table: tail.font_table as usize,
        register,
        tail14: tail14_start..tail15,
        tail15,
        tail16,
    })
}

/// 원본 파일 자신의 틀(헤더 · 문단모양 표 · 스타일 표 · 쪽 설정 꼬리)로 생성 프로필을 만든다 — 내장 틀이 없는
/// 서식(의견서 · 서식 7)을 그 파일 틀 그대로 다시 굽는다. 의견서는 파일마다 모양 표가 다르다(실물 5건: 3·6·7개).
/// 본문 · 줄 배열 · 셀 표 · 등록부는 생성기가 모델로 새로 만든다.
pub fn source_generation_profile(path: &Path) -> Result<crate::GenerationProfile, CoreError> {
    let (mut header, inflated) = inflate_hlt(path)?;
    let builtin = crate::generation_profile(crate::GenerationDialect::PatentSpecificationKo);
    // 라이터는 늘 deflate 로 쓴다 — 무압축 원본(opinion3)의 표지를 빌리면 압축본을 해제본으로 읽는다.
    header[COMPRESSED] = 1;
    // 생성기는 현행 판(짝 등록부 · 칸 레코드 크기)으로 쓴다 — 판이 다르면 원본 헤더를 빌릴 수 없다.
    if header[VERSION] != builtin.header[VERSION] || !register_paired(&header) {
        return Err(error(
            ErrorCode::UnsupportedFormat,
            "source_profile_version",
        ));
    }
    let regions = document_regions(&header, &inflated)?;
    let mut spool = tempfile::tempfile().map_err(|_| error(ErrorCode::IoError, "regions_spool"))?;
    spool
        .write_all(&inflated)
        .map_err(|_| error(ErrorCode::IoError, "regions_spool"))?;
    let size = inflated.len() as u64;
    let edit = inspect_edit_layout(&mut spool, size)?;
    let tail = locate_tail(
        &mut spool,
        size,
        edit.line_base,
        edit.total_lines,
        cell_record_size(&header)?,
    )?;
    let slice = |start: usize,
                 end: usize,
                 what: &'static str|
     -> Result<std::borrow::Cow<'static, [u8]>, CoreError> {
        inflated
            .get(start..end)
            .map(|bytes| std::borrow::Cow::Owned(bytes.to_vec()))
            .ok_or_else(|| error(ErrorCode::StructureInvalid, what))
    };
    // 절 표 종결 레코드(67B · 번호 0xFFFFFFFF)가 본문 바로 앞에 있다.
    let gap = regions.body_start.saturating_sub(SECTION_RECORD);
    if inflated.get(gap + 55..gap + 59) != Some(&[0xff; 4][..]) {
        return Err(error(ErrorCode::StructureInvalid, "source_profile_gap"));
    }
    let counter = (edit.line_base + 12 * edit.total_lines) as usize;
    let shape_count = u32::from_le_bytes(
        inflated
            .get(counter + 1..counter + 5)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| error(ErrorCode::StructureInvalid, "source_profile_shapes"))?,
    );
    let shapes = slice(
        counter + 13,
        tail.shape_order.start_byte as usize,
        "source_profile_shapes",
    )?;
    let geometry_count = tagged_count(&inflated, regions.geometry, "source_profile_geometry")?;
    let geometry = slice(
        regions.geometry,
        regions.geometry + 5 + GEOMETRY_RECORD * geometry_count,
        "source_profile_geometry",
    )?;
    // 레코드 i(1..) = 모양 바이트 72i-8 부터: +8 들여쓰기 · +12 정렬 · +40 줄 간격.
    let field = |at: usize| u32::from_le_bytes(shapes[at..at + 4].try_into().expect("shape field"));
    let shape_map = (1..shape_count as usize)
        .filter(|index| PARAGRAPH_SHAPE_RECORD * index + 36 <= shapes.len())
        .map(|index| {
            let at = PARAGRAPH_SHAPE_RECORD * index;
            (
                field(at) as i32,
                field(at + 4),
                field(at + 32),
                index as u32,
            )
        })
        .collect();
    Ok(crate::GenerationProfile {
        header,
        pre_body_gap: slice(gap, regions.body_start, "source_profile_gap")?,
        shapes,
        shape_order: slice(
            tail.shape_order.start_byte as usize,
            tail.shape_order.end_byte as usize,
            "source_profile_shape_order",
        )?,
        geometry,
        second_shape_order: slice(
            tail.second_shape_order.start_byte as usize,
            tail.second_shape_order.end_byte as usize,
            "source_profile_second_shape_order",
        )?,
        style_table: slice(
            tail.style_table.start_byte as usize,
            tail.style_table.end_byte as usize,
            "source_profile_style_table",
        )?,
        ending: slice(
            regions.tail14.start,
            inflated.len(),
            "source_profile_ending",
        )?,
        shape_count,
        fallback_shape: 1,
        shape_map: Some(shape_map),
    })
}

/// 서식을 「새 문서」로 바꾼다.
pub fn new_document_from_template(template: &Path) -> Result<NewDocument, CoreError> {
    let (mut header, mut source) = inflate_hlt(template)?;
    // 판 순서를 먼저 본다 — 받지 않는 판에서 구조 해석이 엉뚱한 오류(cp949 등)로 떨어지지 않게.
    cell_record_size(&header)?;
    let mut regions = document_regions(&header, &source)?;
    // R14: 0x2b 서식은 등록부가 줄마다 문단 정보 번호 하나다. 줄마다 절 번호 칸(0)을 끼워 0x32 모양으로 편 뒤
    // 나머지 규칙을 그대로 건다(서식 Change: 결과가 정확히 줄 수 × 4B 크다).
    if !register_paired(&header) {
        let lines = regions.total_lines;
        let slots = regions.register + 1;
        let mut widened = source[..slots].to_vec();
        for line in 0..lines {
            widened.extend_from_slice(&source[slots + 4 * line..slots + 4 * line + 4]);
            widened.extend_from_slice(&[0; 4]);
        }
        widened.extend_from_slice(&source[slots + 4 * lines..]);
        source = widened;
        header[VERSION] = 0x32;
        regions = document_regions(&header, &source)?;
    }
    let cell_record = cell_record_size(&header)? as usize;

    // R9: 비어 있는 쪽 여백을 기본값으로. 제자리 고침 · R15 가 본문 폭을 여기서 읽는다.
    let geometry = regions.geometry;
    for record in 0..tagged_count(&source, geometry, "geometry")? {
        let margins = geometry + 5 + GEOMETRY_RECORD * record + 4;
        let field = source
            .get_mut(margins..margins + 16)
            .ok_or_else(|| error(ErrorCode::StructureInvalid, "geometry"))?;
        if field.iter().all(|byte| *byte == 0) {
            field.copy_from_slice(&DEFAULT_MARGINS);
        }
    }

    // R10·R11: 글자모양 표 정리. 기하·셀 표보다 앞이지만 길이를 안 바꾼다.
    tidy_char_shapes(&mut source, &regions)?;
    // R16: 서열목록 구간 글자 10 · 줄 간격 130. 문단모양 표가 자라므로 자리를 다시 잡는다.
    if sequence_listing(&mut source, &regions)? {
        regions = document_regions(&header, &source)?;
    }
    // R15: 문단을 저장 폭으로 다시 흘린다(R16 의 글자 크기로). 글자 표 길이가 바뀌므로 자리를 다시 잡는다.
    if let Some(rewrapped) = rewrap(&source, &regions, cell_record, None)? {
        source = rewrapped;
        regions = document_regions(&header, &source)?;
    }
    // R3·R4: 절 트리를 본문 제목에서 다시 만든다. 등록부는 셀 표 뒤라 R5 전에 원본 좌표로 쓴다.
    let sections = rebuild_sections(&source, &regions)?;
    for (line, index) in sections.line_index.iter().enumerate() {
        let slot = regions.register + 5 + 4 * (2 * line);
        source[slot..slot + 4].copy_from_slice(&index.to_le_bytes());
    }
    let mut inflated = Vec::with_capacity(source.len() + 1024);
    // R13: 셀의 줄 범위(+52·+56)를 줄 배열에서 다시 센다(R5 전 — 원본 레코드 폭으로).
    recount_cell_lines(&mut source, &regions, cell_record)?;
    // R17: 살아 있는 칸의 높이(+48)·행 병합 수(+68)를 다시 잰다(R13 의 줄 범위로).
    cell_heights(&mut source, &regions, cell_record)?;

    // R5: 셀 표 레코드를 80B 로. 0x32 의 76B 레코드는 끝 u32 가 없다.
    let cells = regions.cell_table;
    let count = tagged_count(&source, cells, "cell_table")?;
    inflated.extend_from_slice(&source[..cells + 5]);
    for record in source[cells + 5..cells + 5 + cell_record * count].chunks_exact(cell_record) {
        inflated.extend_from_slice(record);
        inflated.resize(inflated.len() + (80 - cell_record), 0);
    }
    let shift = inflated.len() - (cells + 5 + cell_record * count);
    inflated.extend_from_slice(&source[cells + 5 + cell_record * count..]);

    // R6: 꼬리 #15 = 저장 때 커서(줄, 칸). 통특기는 줄이 0 이면 칸을 0 으로 되돌린다
    // ((0,17)·(0,9)·(0,23) → 칸 0 · (3,18)·(5,29) 그대로 · 줄은 88건 모두 그대로).
    let tail15 = regions.tail15 + shift;
    if read_u32(&inflated, tail15 + 1)? == 0 {
        inflated[tail15 + 5..tail15 + 9].fill(0);
    }
    // R7: 꼬리 #16 용지 → A4.
    let tail16 = regions.tail16 + shift;
    inflated[tail16 + 1..tail16 + 9].copy_from_slice(&A4_INCHES);

    // R3: 절 표 교체. 맨 앞이라 위의 모든 자리 고침이 끝난 뒤에 끼운다.
    inflated.splice(SECTION_START..regions.body_start, sections.table);

    // R1·R2: 현행 판, 문서 종류 0. 라이터는 늘 deflate 하므로 압축 표지도 1(무압축 서식도 1).
    header[VERSION] = 0x33;
    header[COMPRESSED] = 1;
    header[DOCUMENT_TYPE].fill(0);
    // R8: 꼬리 #14 = 렌더 쪽 수(서식 101건 확정). 값은 [서식 첫 값, 0 × (쪽 수 − 1)].
    let inflated = with_page_count(&header, inflated)?;
    Ok(NewDocument { header, inflated })
}

/// 빈 글자모양 레코드(통특기가 지운 모양): 색 칸 +64 = ffffff, +80 = 1.
fn empty_char_shape() -> [u8; CHAR_SHAPE_RECORD] {
    let mut record = [0_u8; CHAR_SHAPE_RECORD];
    record[64..68].copy_from_slice(&[0xff, 0xff, 0xff, 0x00]);
    record[80] = 1;
    record
}

/// R10: 통특기 리더는 읽으면서 글자 표가 가리키는 모양 번호(와 0번)만 「사용 중」으로 표시하고
/// 나머지를 빈 레코드로 지운다.
/// R11: 플래그(+0) 0x200 모양을 가리키는 글자는 0x200 만 뺀 같은 모양으로 옮긴다(서식 5/5).
/// 0x200 모양 레코드 자체는 읽을 때 사용 중이었으므로 남는다.
fn tidy_char_shapes(source: &mut [u8], regions: &DocumentRegions) -> Result<(), CoreError> {
    let (records, _) = object_records(source, regions)?;
    let count = records.len();
    let mut codes = Vec::new();
    let mut line = regions.char_base;
    for _ in 0..regions.total_lines {
        let n = read_u32(source, line)? as usize;
        codes.extend((0..n).map(|i| line + 4 + 4 * i));
        line += 4 + 4 * n;
    }
    let mut used = vec![false; count];
    used[0] = true;
    for &slot in &codes {
        if let Some(flag) = used.get_mut(read_u32(source, slot)? as usize) {
            *flag = true;
        }
    }
    let shape = |source: &[u8], code: usize| -> Option<[u8; CHAR_SHAPE_RECORD]> {
        let (at, plain) = *records.get(code)?;
        plain.then(|| source[at..at + CHAR_SHAPE_RECORD].try_into().unwrap())
    };
    let mut remap = vec![None; count];
    for (code, target) in remap.iter_mut().enumerate() {
        let Some(mut record) = shape(source, code) else {
            continue;
        };
        if record[1] & 0x02 == 0 {
            continue;
        }
        record[1] &= !0x02;
        *target = (0..count).find(|other| shape(source, *other) == Some(record));
    }
    for &slot in &codes {
        let code = read_u32(source, slot)? as usize;
        if let Some(Some(target)) = remap.get(code) {
            source[slot..slot + 4].copy_from_slice(&(*target as u32).to_le_bytes());
        }
    }
    for (code, (at, plain)) in records.iter().enumerate() {
        if *plain && !used[code] {
            source[*at..*at + CHAR_SHAPE_RECORD].copy_from_slice(&empty_char_shape());
        }
    }
    // R12: 통특기는 줄을 그릴 때 표시 기호용 회색 Symbol 9pt 를 만든다(같은 모양이
    // 있으면 재사용, 없으면 첫 빈 자리). 위에서 지운 뒤라 대개 1번에 다시 선다
    // (Priority: 템플릿 21번 → 1번). 그릴 글자가 없는 emptydoc 에는 없다 — 그리기 조건은 레이아웃(P3) 몫이라
    // 「본문에 줄바꿈·문단 끝 표지 말고 다른 바이트가 있다」로 근사한다. (글자 표는 한 줄이 한 모양이면 번호를 하나만
    // 싣는다 — 글자 표의 개수로 재면 안 된다.)
    let has_text = source[regions.body_start..regions.body_end]
        .iter()
        .any(|byte| !matches!(byte, b'\r' | b'\n' | 0x15));
    let present = records
        .iter()
        .any(|(at, plain)| *plain && source[*at..*at + CHAR_SHAPE_RECORD] == GRAY_SYMBOL);
    if has_text && !present {
        let free = records.iter().skip(1).find(|(at, plain)| {
            *plain && source[*at..*at + CHAR_SHAPE_RECORD] == empty_char_shape()
        });
        let Some((at, _)) = free else {
            return Err(error(ErrorCode::UnsupportedFormat, "char_shape_full"));
        };
        source[*at..*at + CHAR_SHAPE_RECORD].copy_from_slice(&GRAY_SYMBOL);
    }
    Ok(())
}

/// R16: 서열목록. 통특기는 【서열목록】 책갈피가 있으면 그 다음 줄 처음부터 마지막 줄 처음까지를 골라
/// 글자 크기 10 · 줄 간격 130 을 건다(문단 간격 앞 0, 뒤 0, 줄 130).
/// 글자모양은 고른 글자만(마지막 줄은 빠진다), 문단모양은 고르기가 닿는 문단 전부(마지막 줄 포함).
/// 모양은 둘 다 「같은 것이 있으면 그 번호, 없으면 새로」 — 글자 = 첫 빈 자리 · 문단 = 표 끝에 덧붙임, 줄 순서대로.
/// 이 제목이 있는 서식 둘(PctPatH·PctPatHD) 다. 다음 줄이 서열목록 파일 첨부 문구인 갈래(빨강·굵게)는 서식에 없어 안 옮겼다.
/// ponytail: 문단 앞뒤 간격 칸의 자리를 모른다 — 서식의 해당 문단은 모두 0 이라 줄 간격(+40)만 쓴다.
fn sequence_listing(source: &mut Vec<u8>, regions: &DocumentRegions) -> Result<bool, CoreError> {
    let heading = EUC_KR.encode("【서열목록】").0;
    let lines = body_lines(
        &source[regions.body_start..regions.body_end],
        regions.total_lines,
    );
    let Some(first) = lines
        .iter()
        .position(|line| trim_trailing_spaces(line) == &heading[..])
    else {
        return Ok(false);
    };
    let last = regions.total_lines - 1;
    if first + 1 >= last {
        return Ok(false);
    }
    let (records, line_array) = object_records(source, regions)?;
    let mut slots = Vec::with_capacity(regions.total_lines);
    let mut at = regions.char_base;
    for _ in 0..regions.total_lines {
        let n = read_u32(source, at)? as usize;
        slots.push((0..n).map(|i| at + 4 + 4 * i).collect::<Vec<_>>());
        at += 4 + 4 * n;
    }
    let mut sized = BTreeMap::new();
    for slot in slots[first + 1..last].iter().flatten() {
        let code = read_u32(source, *slot)?;
        let new = match sized.get(&code) {
            Some(new) => *new,
            None => {
                let Some(&(record, true)) = records.get(code as usize) else {
                    return Err(error(ErrorCode::UnsupportedFormat, "sequence_shape"));
                };
                let mut shape: [u8; CHAR_SHAPE_RECORD] = source[record..record + CHAR_SHAPE_RECORD]
                    .try_into()
                    .unwrap();
                shape[4..8].copy_from_slice(&10_u32.to_le_bytes());
                let same = |target: &[u8; CHAR_SHAPE_RECORD]| {
                    records.iter().position(|(at, plain)| {
                        *plain && source[*at..*at + CHAR_SHAPE_RECORD] == target[..]
                    })
                };
                let new = match same(&shape) {
                    Some(found) => found,
                    None => {
                        let free = records
                            .iter()
                            .skip(1)
                            .position(|(at, plain)| {
                                *plain && source[*at..*at + CHAR_SHAPE_RECORD] == empty_char_shape()
                            })
                            .map(|index| index + 1)
                            .ok_or_else(|| {
                                error(ErrorCode::UnsupportedFormat, "char_shape_full")
                            })?;
                        let at = records[free].0;
                        source[at..at + CHAR_SHAPE_RECORD].copy_from_slice(&shape);
                        free
                    }
                } as u32;
                sized.insert(code, new);
                new
            }
        };
        source[*slot..*slot + 4].copy_from_slice(&new.to_le_bytes());
    }
    let table = line_array + 1 + 12 * regions.total_lines;
    let mut count = tagged_count(source, table, "paragraph_shapes")?;
    let record = |index: usize| table + 5 + PARAGRAPH_SHAPE_RECORD * index;
    let mut spaced = BTreeMap::new();
    for line in first + 1..=last {
        let slot = line_array + 1 + 12 * line;
        let shape = read_u32(source, slot)? as usize;
        let new = match spaced.get(&shape) {
            Some(new) => *new,
            None => {
                if shape >= count {
                    return Err(error(ErrorCode::StructureInvalid, "paragraph_shapes"));
                }
                let mut target =
                    source[record(shape)..record(shape) + PARAGRAPH_SHAPE_RECORD].to_vec();
                target[40..44].copy_from_slice(&LINE_SPACING_SEQUENCE.to_le_bytes());
                let new = match (0..count).find(|index| {
                    source[record(*index)..record(*index) + PARAGRAPH_SHAPE_RECORD] == target[..]
                }) {
                    Some(found) => found,
                    None => {
                        source.splice(record(count)..record(count), target);
                        count += 1;
                        source[table + 1..table + 5].copy_from_slice(&(count as u32).to_le_bytes());
                        count - 1
                    }
                } as u32;
                spaced.insert(shape, new);
                new
            }
        };
        source[slot..slot + 4].copy_from_slice(&new.to_le_bytes());
    }
    Ok(true)
}

/// R13: 셀 레코드 +52 = 첫 줄, +56 = 끝 줄. 서식엔 낡은 값이 남기도 해서(SpAcc1: 16번 셀부터 1씩 큼)
/// 통특기는 줄 배열(객체 표 뒤 `bf` + 줄마다 모양·셀 번호·0)에서 다시 센다.
fn recount_cell_lines(
    source: &mut [u8],
    regions: &DocumentRegions,
    cell_record: usize,
) -> Result<(), CoreError> {
    let (_, at) = object_records(source, regions)?;
    let cells = tagged_count(source, regions.cell_table, "cell_table")?;
    let mut ranges: Vec<Option<(u32, u32)>> = vec![None; cells];
    let lines = body_lines(
        &source[regions.body_start..regions.body_end],
        regions.total_lines,
    );
    for line in 0..regions.total_lines {
        let cell = read_u32(source, at + 1 + 12 * line + 4)? as usize;
        // 행 끝 표지(0x12)만 있는 줄은 줄 배열에선 행의 마지막 칸에 붙지만 칸의 범위엔 안 든다(SpAcc1 4칸).
        if cell == 0 || lines.get(line) == Some(&&[ROW_END][..]) {
            continue;
        }
        let range = ranges
            .get_mut(cell)
            .ok_or_else(|| error(ErrorCode::StructureInvalid, "line_cell"))?;
        let line = line as u32;
        *range = Some(range.map_or((line, line), |(first, _)| (first, line)));
    }
    for (cell, range) in ranges.iter().enumerate() {
        if let Some((first, last)) = range {
            let record = regions.cell_table + 5 + cell_record * cell;
            source[record + 52..record + 56].copy_from_slice(&first.to_le_bytes());
            source[record + 56..record + 60].copy_from_slice(&last.to_le_bytes());
        }
    }
    Ok(())
}

/// R17: 통특기는 본문에 줄이 있는 칸(살아 있는 칸)의 높이 +48 을 다시 잰다. 본문에 없는 칸(낡은 표 레코드)은 그대로.
/// 칸 내용 높이 = 줄 수 × 106 − 5, 표의 첫·끝 행이면 +3 (101·207·313 / 104·210·316). 행 병합 칸(+68 = 행 수 > 1)은
/// 걸친 행 높이의 합이고(첫 행은 제 내용 높이와의 최대 — SpIdea 1146 = max(207, 313)+313+313+207), 행 높이 = 그 행의
/// 병합 안 된 칸 내용 높이 최대. ⛔ 「합 + 106」 과는 자료(둘)로 못 가른다.
/// 병합에 덮인 칸(+64 비트 16)은 0. 살아 있는 칸의 +68 이 0 이면 1 로 (SpRev 28칸).
/// 행 표(셀 표 바로 앞 `bf n` + 80B, +20 = 행 높이)도 살아 있는 행은 max(117, 행 높이)로 (SpRev 101·104 칸만 있는 행 → 117).
/// ponytail: 117 은 SpRev 에서만 보인 하한이다(다른 서식 행은 모두 207 이상) — 유도식 미상.
/// 확인한 서식: SpIdea · SpRev(다시 잼) · SpAcc1 · SpIdea1(서식 값 그대로가 이 식과 같음) — 표가 산 서식 전부.
/// 글자·줄 간격과 무관하다: 통특기가 저장한 12pt · 줄 간격 280 표(`fixtures/phase2/pct_table_probe`)도 101/104 그대로.
/// ponytail: 두 줄 이상 칸은 10pt · 130 에서만 쟀다 · 한 행짜리 표(첫 = 끝 행)는 자료가 없다.
fn cell_heights(
    source: &mut [u8],
    regions: &DocumentRegions,
    cell_record: usize,
) -> Result<(), CoreError> {
    const LINE: u32 = 106;
    const BASE: u32 = 5;
    const EDGE: u32 = 3;
    const COVERED: u32 = 16;
    const ROW_MIN: u32 = 117;
    const ROW_RECORD: usize = 80;
    let (_, line_array) = object_records(source, regions)?;
    let cells = regions.cell_table + 5;
    let field =
        |source: &[u8], cell: usize, at: usize| read_u32(source, cells + cell_record * cell + at);
    // 표 = 줄 배열에서 칸 줄이 이어진 구간. 그 첫·끝 줄의 행이 첫·끝 행이다.
    let mut edges = Vec::new();
    let mut run: Option<(u32, u32)> = None;
    let mut live = std::collections::BTreeSet::new();
    for line in 0..regions.total_lines {
        let cell = read_u32(source, line_array + 1 + 12 * line + 4)? as usize;
        if cell != 0 {
            live.insert(cell);
            let row = field(source, cell, 4)?;
            run = Some(run.map_or((row, row), |(first, _)| (first, row)));
        } else if let Some(table) = run.take() {
            edges.push(table);
        }
    }
    edges.extend(run);
    let mut content = BTreeMap::new();
    let mut rows: BTreeMap<u32, u32> = BTreeMap::new();
    for &cell in &live {
        let span = cells + cell_record * cell + 68;
        if read_u32(source, span)? == 0 {
            source[span..span + 4].copy_from_slice(&1_u32.to_le_bytes());
        }
        let (row, first, last) = (
            field(source, cell, 4)?,
            field(source, cell, 52)?,
            field(source, cell, 56)?,
        );
        let edge = edges
            .iter()
            .any(|(top, bottom)| row == *top || row == *bottom);
        let height = LINE * (last - first + 1) - BASE + if edge { EDGE } else { 0 };
        content.insert(cell, height);
        if field(source, cell, 64)? & COVERED == 0 && field(source, cell, 68)? <= 1 {
            let tallest = rows.entry(row).or_default();
            *tallest = (*tallest).max(height);
        }
    }
    rows.values_mut()
        .for_each(|height| *height = (*height).max(ROW_MIN));
    // 행 표: 셀 표 태그 바로 앞에서 끝나는 `bf n` + n × 80B.
    let row_table = (0..regions.cell_table / ROW_RECORD)
        .map(|n| regions.cell_table.checked_sub(5 + ROW_RECORD * n))
        .take_while(Option::is_some)
        .flatten()
        .find(|at| {
            tagged_count(source, *at, "row_table")
                .is_ok_and(|n| at + 5 + ROW_RECORD * n == regions.cell_table)
        })
        .ok_or_else(|| error(ErrorCode::StructureInvalid, "row_table"))?;
    let row_count = tagged_count(source, row_table, "row_table")?;
    for (&row, &height) in &rows {
        if row as usize >= row_count {
            return Err(error(ErrorCode::StructureInvalid, "row_table"));
        }
        let at = row_table + 5 + ROW_RECORD * row as usize + 20;
        source[at..at + 4].copy_from_slice(&height.to_le_bytes());
    }
    for &cell in &live {
        let (row, span) = (field(source, cell, 4)?, field(source, cell, 68)?);
        let height = if field(source, cell, 64)? & COVERED != 0 {
            0
        } else if span > 1 {
            let first = rows.get(&row).copied().unwrap_or(0).max(content[&cell]);
            first
                + (row + 1..row + span)
                    .map(|row| rows.get(&row).copied().unwrap_or(0))
                    .sum::<u32>()
        } else {
            content[&cell]
        };
        let at = cells + cell_record * cell + 48;
        source[at..at + 4].copy_from_slice(&height.to_le_bytes());
    }
    Ok(())
}

struct Sections {
    /// 절 레코드들 + 종결 레코드.
    table: Vec<u8>,
    /// 줄마다 붙은 북마크 번호(없으면 0) — 등록부 짝수 슬롯(R4).
    line_index: Vec<u32>,
}

/// R3 규칙(서식 92/92).
fn rebuild_sections(source: &[u8], regions: &DocumentRegions) -> Result<Sections, CoreError> {
    let latch: Vec<Vec<u8>> = SECTION_LATCH
        .split('|')
        .map(|item| EUC_KR.encode(item).0.into_owned())
        .collect();
    let lines = body_lines(
        &source[regions.body_start..regions.body_end],
        regions.total_lines,
    );
    // 줄마다 붙은 북마크 번호(= 등록부 짝수 슬롯). 서식에 남은 옛 번호에서 출발한다.
    let mut line_index: Vec<u32> = (0..regions.total_lines)
        .map(|line| read_u32(source, regions.register + 5 + 8 * line))
        .collect::<Result<_, _>>()?;
    // 옛 【 레코드를 지운다. 통특기는 지운 번호를 「줄 번호」로 범위 검사해 번호 < 줄 수
    // 일 때만 그 번호를 단 첫 줄을 0 으로 만든다. 그래서 PCT 의견서류의
    // 옛 번호 16(9줄 문서)은 살아남아 새 제목이 물려받는다(서식 PctOpnKr·PctExpKr 16).
    let mut at = SECTION_START;
    while at + SECTION_RECORD <= regions.body_start {
        let index = read_u32(source, at + 55)?;
        if index == u32::MAX {
            break;
        }
        if (index as usize) < line_index.len()
            && let Some(slot) = line_index.iter_mut().find(|slot| **slot == index)
        {
            *slot = 0;
        }
        at += SECTION_RECORD;
    }
    let mut titles: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut on = false;
    let mut line = 0;
    while line < lines.len() {
        let text = lines[line];
        let Some(open) = find_dbcs(text, OPEN).filter(|_| text.len() > 2) else {
            line += 1;
            continue;
        };
        // 】는 같은 줄이나 두 줄 뒤까지. 여러 줄에 걸치면 제목은 【 줄의 나머지이고, 걸친 줄이 모두 같은 번호를 단다.
        let (title, span) = match find_dbcs(&text[open..], CLOSE) {
            Some(close) => (&text[open..open + close + 2], 1),
            None => match (1..=2).find(|k| {
                lines
                    .get(line + k)
                    .is_some_and(|l| find_dbcs(l, CLOSE).is_some())
            }) {
                Some(k) => (trim_trailing_spaces(&text[open..]), k + 1),
                None => {
                    line += 1;
                    continue;
                }
            },
        };
        on = on || latch.iter().any(|item| title[2..].starts_with(item));
        let title = cap_title(title);
        if on && !titles.iter().any(|(_, known)| *known == title) {
            // 줄에 번호가 남아 있으면 물려받고, 없으면 지금까지의 최대 + 1.
            let index = match line_index[line] {
                0 => titles.iter().map(|(index, _)| *index).max().unwrap_or(0) + 1,
                kept => kept,
            };
            titles.push((index, title));
            line_index[line..line + span].fill(index);
        }
        line += span;
    }
    // 절 표는 줄 차례로, 실재하는 북마크 번호를 단 줄마다 레코드 하나(여러 줄 제목은 줄 수만큼 되풀이 — TradMark).
    let records: Vec<(u32, &[u8])> = line_index
        .iter()
        .filter_map(|index| {
            titles
                .iter()
                .find(|(known, _)| known == index)
                .map(|(index, title)| (*index, title.as_slice()))
        })
        .collect();
    let mut table = Vec::with_capacity((records.len() + 1) * SECTION_RECORD);
    for (i, (index, title)) in records.iter().enumerate() {
        let mut record = [0_u8; SECTION_RECORD];
        record[..title.len()].copy_from_slice(title);
        record[51..55].copy_from_slice(&(title.len() as u32).to_le_bytes());
        record[55..59].copy_from_slice(&index.to_le_bytes());
        // 다음·이전 포인터는 통특기 힙 주소라 비결정이다. 모양만 맞춘다: 끝 레코드 다음 = 0, 첫 레코드 이전 = 0.
        let next = if i + 1 == records.len() {
            0
        } else {
            0x1DF2F60 - 0x50 * i as u32
        };
        let prev = if i == 0 {
            0
        } else {
            0x39CCD70 - 0x50 * (i as u32 - 1)
        };
        record[59..63].copy_from_slice(&next.to_le_bytes());
        record[63..67].copy_from_slice(&prev.to_le_bytes());
        table.extend_from_slice(&record);
    }
    let mut terminator = [0_u8; SECTION_RECORD];
    terminator[55..59].copy_from_slice(&u32::MAX.to_le_bytes());
    table.extend_from_slice(&terminator);
    Ok(Sections { table, line_index })
}

/// 본문 저장 줄. 줄은 `\r\n` 에서, 문단은 0x15 에서 끝난다(0x15 뒤 `\r\n` 은 다음 문단의 머리 — 줄이 아니다).
fn body_lines(body: &[u8], total_lines: usize) -> Vec<&[u8]> {
    let (mut lines, mut start, mut at) = (Vec::new(), 0, 0);
    while at < body.len() && lines.len() < total_lines {
        if body[at] == 0x15 || body[at..].starts_with(b"\r\n") {
            lines.push(&body[start..at]);
            at += if body[at] == 0x15 { 1 } else { 2 };
            if body[at - 1] == 0x15 && body[at..].starts_with(b"\r\n") {
                at += 2;
            }
            start = at;
        } else {
            at += if body[at] >= 0x81 { 2 } else { 1 };
        }
    }
    lines
}

/// 2바이트 글자 경계에서만 찾는다(「에서」 = bf a1 bc ad 의 가운데 a1 bc 를 【로 읽지 않게).
fn find_dbcs(text: &[u8], pattern: [u8; 2]) -> Option<usize> {
    let mut at = 0;
    while at + 1 < text.len() {
        if text[at..at + 2] == pattern {
            return Some(at);
        }
        at += if text[at] >= 0x81 { 2 } else { 1 };
    }
    None
}

fn trim_trailing_spaces(text: &[u8]) -> &[u8] {
    let end = text.iter().rposition(|b| *b != b' ').map_or(0, |i| i + 1);
    &text[..end]
}

/// 50바이트 상한 → 끝에서 쪼개진 2바이트 글자 제거 → [2, 길이−2) 가 공백 연속으로 끝나면 그 공백부터 자른다
/// (`…품질·명성 또` → `…품질·명성`).
fn cap_title(title: &[u8]) -> Vec<u8> {
    let mut title = title[..title.len().min(SECTION_TITLE_CAP)].to_vec();
    let mut at = 0;
    while at < title.len() {
        if title[at] >= 0x80 {
            if at + 1 >= title.len() {
                title.truncate(at);
                break;
            }
            at += 1;
        }
        at += 1;
    }
    let (mut space, mut at) = (None, 2);
    while at + 2 < title.len() {
        if title[at] >= 0x80 {
            at += 1;
            space = None;
        } else if title[at] == b' ' {
            space = space.or(Some(at));
        } else {
            space = None;
        }
        at += 1;
    }
    if let Some(space) = space {
        title.truncate(space);
    }
    title
}

/// 변환 결과를 임시 컨테이너로 읽어 쪽을 매기고(`paginate`) 꼬리 #14 를 그 쪽 수로 다시 쓴다.
fn with_page_count(header: &Header, mut inflated: Vec<u8>) -> Result<Vec<u8>, CoreError> {
    let mut draft = tempfile::NamedTempFile::new()
        .map_err(|_| error(ErrorCode::IoError, "page_count_spool"))?;
    crate::write_hlt(
        header,
        &mut std::io::Cursor::new(&inflated),
        draft.as_file_mut(),
    )?;
    let document = crate::inspect_path(
        draft.path(),
        InspectOptions {
            include_image_payloads: true,
            ..InspectOptions::default()
        },
    )?;
    let pages = crate::page_count(&document, &inflated).max(1);
    let regions = document_regions(header, &inflated)?;
    let first = read_u32(&inflated, regions.tail14.start + 5).unwrap_or(0);
    let mut table = vec![TAG];
    table.extend_from_slice(&(pages as u32).to_le_bytes());
    table.extend_from_slice(&first.to_le_bytes());
    table.resize(table.len() + 4 * (pages - 1), 0);
    inflated.splice(regions.tail14.clone(), table);
    Ok(inflated)
}

/// 객체 표 레코드마다 (자리, 글자모양인가) 와 표 뒤 줄 배열(`bf` + 줄마다 문단모양·칸 번호·0) 태그 자리.
fn object_records(
    source: &[u8],
    regions: &DocumentRegions,
) -> Result<(Vec<(usize, bool)>, usize), CoreError> {
    let count = tagged_count(source, regions.object_table, "object_table")?;
    let mut records = Vec::with_capacity(count);
    let mut at = regions.object_table + 5;
    for _ in 0..count {
        let kind = read_u32(source, at + 40)?;
        let size = if matches!(kind, 3 | 4) {
            384 + read_u32(source, at + 380)? as usize
        } else {
            CHAR_SHAPE_RECORD
        };
        records.push((at, size == CHAR_SHAPE_RECORD));
        at += size;
    }
    if source.get(at) != Some(&TAG) {
        return Err(error(ErrorCode::StructureInvalid, "line_array"));
    }
    Ok((records, at))
}

/// R15: 통특기는 서식을 열면 문단을 편집기 폭으로 처음부터 다시 흘린다(SpRev 「3천만원 ⏎이하인」 → 「3천만원 이⏎하인」 ·
/// SpIdea1 다음 줄 앞 공백 셋이 앞 줄 끝으로). 끊기는 인쇄 재흘림과 같은 규칙(`reflow::wrap` — 한글은 글자 단위 ·
/// ASCII 는 낱말 · 줄머리 금지 부호 · 공백은 앞 줄 끝에 매단다)이고 폭만 저장 폭이다: 글자 = 바이트 × 크기 × 10 twip
/// (바탕체 고정폭) · 본문 = A4 − 좌우 여백 − 문단 좌우 들여쓰기(첫 줄은 +8 들여쓰기 더) · 칸 = 셀 +20 폭.
/// 서식 101건 전부 고정점(SpRev·SpIdea1 만 바뀐다) ·
/// 인쇄 정답 domestic 264/264 · pct 563/564 · 표 칸 174/174 고정점.
/// 줄 수가 바뀌면(줄 배열·셀 범위·등록부·쪽까지 다시 짜야 한다) 거절한다 — 101건엔 없다.
/// 첨자·그림·크기 섞인 문단도 거절한다(서식엔 없다 · 화면 첨자 폭 미확정).
/// `audit` 이면 고치지 않고 문단을 센다: [저장 줄 그대로, 다름, 첨자·그림·크기 섞임이라 못 잼].
fn rewrap(
    source: &[u8],
    regions: &DocumentRegions,
    cell_record: usize,
    mut audit: Option<&mut [usize; 3]>,
) -> Result<Option<Vec<u8>>, CoreError> {
    let refuse = |phase| Err(error(ErrorCode::UnsupportedFormat, phase));
    let body = &source[regions.body_start..regions.body_end];
    let lines = body_lines(body, regions.total_lines);
    if lines.len() != regions.total_lines {
        return Err(error(ErrorCode::StructureInvalid, "rewrap_lines"));
    }
    let start = |line: &[u8]| line.as_ptr() as usize - body.as_ptr() as usize;
    // 줄마다 (내용, 끝 표지). 끝 표지 = 다음 줄 시작까지(0x15 = 문단 끝 · \r\n = 줄바꿈).
    let mut rows: Vec<(Vec<u8>, &[u8])> = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let end = start(line) + line.len();
        let next = lines.get(i + 1).map_or(body.len(), |next| start(next));
        rows.push((line.to_vec(), &body[end..next]));
    }
    // 글자 표: 줄마다 번호 하나(줄 전체) 또는 바이트마다(0x15 끝 표지 몫 하나 더).
    let mut codes = Vec::with_capacity(lines.len());
    let mut at = regions.char_base;
    for (text, end) in &rows {
        let n = read_u32(source, at)? as usize;
        let list = (0..n)
            .map(|i| read_u32(source, at + 4 + 4 * i))
            .collect::<Result<Vec<_>, _>>()?;
        let bytes = text.len() + usize::from(end.first() == Some(&0x15));
        if n != 1 && n != bytes {
            return Err(error(ErrorCode::StructureInvalid, "rewrap_codes"));
        }
        codes.push(list);
        at += 4 + 4 * n;
    }
    let table_end = at;
    let (records, line_array) = object_records(source, regions)?;
    let shapes_at = line_array + 1 + 12 * regions.total_lines;
    let shape_count = tagged_count(source, shapes_at, "paragraph_shapes")?;
    let signed = |at| read_u32(source, at).map(|value| f64::from(value as i32));
    let body_width = {
        let margins = regions.geometry + 5 + 4;
        let inches = |at| read_u32(source, at).map(|bits| f64::from(f32::from_bits(bits)));
        (210.0 / 25.4 - inches(margins)? - inches(margins + 4)?) * 1440.0
    };

    let mut changed = false;
    let mut first = 0;
    while first < rows.len() {
        let cell = read_u32(source, line_array + 1 + 12 * first + 4)? as usize;
        let mut last = first;
        while !(rows[last].1.first() == Some(&0x15)
            || rows[last]
                .0
                .iter()
                .any(|b| matches!(*b, 0x13 | ROW_END | 0x0c))
            || last + 1 == rows.len()
            || read_u32(source, line_array + 1 + 12 * (last + 1) + 4)? as usize != cell)
        {
            last += 1;
        }
        let paragraph = first..last + 1;
        first = last + 1;

        let mut text = Vec::new();
        let mut byte_codes = Vec::new();
        let mut stored = Vec::new();
        for line in paragraph.clone() {
            let content = &rows[line].0;
            let list = &codes[line];
            text.extend_from_slice(content);
            byte_codes.extend(
                (0..content.len()).map(|i| if list.len() == 1 { list[0] } else { list[i] }),
            );
            stored.push(text.len());
        }
        let mut items = Vec::new();
        let mut ends = Vec::new();
        let mut size = None;
        let mut unsupported = false;
        let mut i = 0;
        while i < text.len() {
            let wide = text[i] >= 0x81 && i + 1 < text.len();
            let byte = text[i];
            if byte >= 0x20 || byte == b'\t' {
                let Some(&(record, true)) = records.get(byte_codes[i] as usize) else {
                    unsupported = true;
                    break;
                };
                let points = read_u32(source, record + 4)?;
                if read_u32(source, record)? & (16 | 32) != 0
                    || *size.get_or_insert(points) != points
                {
                    unsupported = true;
                    break;
                }
            }
            items.push(if wide {
                Item::Wide(false)
            } else {
                Item::Ascii(byte, false)
            });
            i += if wide { 2 } else { 1 };
            ends.push(i);
        }
        if unsupported {
            match audit.as_deref_mut() {
                Some(count) => {
                    count[2] += 1;
                    continue;
                }
                None => return refuse("rewrap_shape"),
            }
        }
        let Some(points) = size.filter(|points| *points > 0) else {
            continue;
        };
        let shape = read_u32(source, line_array + 1 + 12 * paragraph.start)?;
        if shape as usize >= shape_count {
            return Err(error(ErrorCode::StructureInvalid, "paragraph_shapes"));
        }
        let shape = shapes_at + 5 + 72 * shape as usize;
        let width = if cell == 0 {
            body_width
        } else {
            // ponytail: 칸 안쪽 여백은 데이터가 (−14, 133] twip 밖으로 못 가른다(결과 불변) — 0 으로 둔다.
            f64::from(read_u32(
                source,
                regions.cell_table + 5 + cell_record * cell + 20,
            )?)
        } - signed(shape)?
            - signed(shape + 4)?;
        // 칸 폭 단위: 한글 100 = 크기 × 20 twip → twip × 5 / 크기.
        let scale = 5.0 / f64::from(points);
        let wrapped: Vec<usize> = reflow::wrap(&items, signed(shape + 8)? * scale, width * scale)
            .into_iter()
            .map(|end| if end == 0 { 0 } else { ends[end - 1] })
            .collect();
        if let Some(count) = audit.as_deref_mut() {
            count[usize::from(wrapped != stored)] += 1;
            continue;
        }
        if wrapped == stored {
            continue;
        }
        if wrapped.len() != stored.len() {
            return refuse("rewrap_line_count");
        }
        changed = true;
        let mut from = 0;
        for (line, to) in paragraph.zip(wrapped) {
            // 번호 하나로 실린 줄은 그대로 하나로(통특기가 줄마다 고른 표현을 지킨다 · SpIdea1).
            let single = codes[line].len() == 1;
            let terminator =
                (rows[line].1.first() == Some(&0x15)).then(|| *codes[line].last().unwrap());
            let mut list: Vec<u32> = byte_codes[from..to]
                .iter()
                .copied()
                .chain(terminator)
                .collect();
            if single && list.iter().all(|code| *code == codes[line][0]) {
                list.truncate(1);
            }
            rows[line].0 = text[from..to].to_vec();
            codes[line] = list;
            from = to;
        }
    }
    if !changed {
        return Ok(None);
    }
    let mut out = source[..regions.body_start].to_vec();
    for (text, end) in &rows {
        out.extend_from_slice(text);
        out.extend_from_slice(end);
    }
    out.extend_from_slice(&source[regions.body_end..regions.char_base]);
    for list in &codes {
        out.extend_from_slice(&(list.len() as u32).to_le_bytes());
        list.iter()
            .for_each(|code| out.extend_from_slice(&code.to_le_bytes()));
    }
    out.extend_from_slice(&source[table_end..]);
    Ok(Some(out))
}

/// P3-2 판정용: 통특기가 저장한 문서의 문단이 R15 줄나눔의 고정점인가 — [그대로, 다름, 못 잼(첨자·그림·크기 섞임)].
#[doc(hidden)]
pub fn stored_wrap_audit(path: &Path) -> Result<[usize; 3], CoreError> {
    let (header, source) = inflate_hlt(path)?;
    let regions = document_regions(&header, &source)?;
    let mut count = [0; 3];
    rewrap(
        &source,
        &regions,
        cell_record_size(&header)? as usize,
        Some(&mut count),
    )?;
    Ok(count)
}

fn read_u32(bytes: &[u8], at: usize) -> Result<u32, CoreError> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| error(ErrorCode::StructureInvalid, "sections"))
}

fn tagged_count(bytes: &[u8], at: usize, phase: &'static str) -> Result<usize, CoreError> {
    match bytes.get(at..at + 5) {
        Some([TAG, a, b, c, d]) => Ok(u32::from_le_bytes([*a, *b, *c, *d]) as usize),
        _ => Err(error(ErrorCode::StructureInvalid, phase)),
    }
}

fn error(code: ErrorCode, phase: &'static str) -> CoreError {
    CoreError::new(code, phase, false)
}
