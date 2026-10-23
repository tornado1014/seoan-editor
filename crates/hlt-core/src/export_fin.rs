//! 제출 세트 작성: `.hlz`(명세서)와 `.fin`(제출파일), 그리고 보정서 `.dta`.
//!
//! `.fin` 구조:
//!
//! ```text
//! .fin (zip)
//! ├── 1020240000001.hlz (zip)      ← KIPO XML + 도면(그림마다 jpg 또는 tif)
//! ├── amend1.dta (zip → 보정서 XML)
//! ├── amend2.dta
//! └── xresult.inf                  ← [APPLICATION], [AMENDMENT] 색인
//! ```
//!
//! `xresult.inf`:
//!
//! ```ini
//! [APPLICATION]
//! APPNAME=1020240000001.hlz,2024-01-02,1
//! APPTIME=240102120
//! [AMENDMENT]
//! AMDCNT=2
//! AMD001=amend1.dta,2024-06-03,1
//! AMD002=amend2.dta,2025-03-04,1
//! ```
//!
//! 보정서 XML: `<KIPO keapsVersion dtaVersion editorKind><AmendDOC docType documentID>`
//! 아래에 `<AmendBody elementName="claim|p|invention-title|summary" status="A|D|I" …>` 가 문서 순서로 늘어서고,
//! 보정(`A`)·넣기(`I`)는 그 안에 새 요소를 통째로 담고 삭제(`D`)는 비운다(`AMENDABLE`).

use crate::{
    CoreError, ErrorCode, HltImagePayload, InspectOptions, KipoExport, KipoExportOptions,
    export_kipo_xml, inspect_format_inventories, inspect_path,
};
use encoding_rs::EUC_KR;
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    path::Path,
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

/// 통특기 JPEG 양자화표(자연 순서). 통특기가 낸 JPEG 의 DQT 를 그대로 읽은 값 — pct_b 614장 · APP2025 · 화학식 모두 이 한 벌이다.
/// 4:2:0 · 96dpi 와 함께 쓰면 크기가 통특기의 0.998~0.999배(pct_b 57.6MB · 예전 품질 92 4:4:4 는 1.9배 = fin 74MB vs 37MB).
/// 바이트는 같지 않다(DCT·색 변환 반올림이 다르다 · 스캔 데이터 일치 0/614).
const NK_LUMA_TABLE: [u16; 64] = [
    14, 10, 9, 14, 22, 37, 47, 56, 11, 11, 13, 17, 24, 54, 55, 51, 13, 12, 14, 22, 37, 53, 64, 52,
    13, 15, 20, 27, 47, 81, 74, 57, 16, 20, 34, 52, 63, 101, 96, 71, 22, 32, 51, 59, 75, 97, 105,
    85, 45, 59, 72, 81, 96, 112, 111, 94, 67, 85, 88, 91, 104, 93, 96, 92,
];
const NK_CHROMA_TABLE: [u16; 64] = [
    15, 16, 22, 43, 61, 92, 92, 92, 16, 19, 24, 61, 92, 92, 92, 92, 22, 24, 52, 92, 92, 92, 92, 92,
    43, 61, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92,
    92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinAmendment {
    /// `.fin` 안에 들어갈 이름. 예: `pct_c_amend1.dta`
    pub file_name: String,
    /// `xresult.inf` 에 적히는 날짜(YYYY-MM-DD).
    pub date: String,
    /// zip 으로 포장된 보정서 `.dta` 바이트.
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinExportOptions {
    /// `.fin` 안에 들어갈 명세서 이름. 통특기는 출원번호를 쓴다 (예: `1020230000001.hlz`).
    pub hlz_name: String,
    /// `xresult.inf` 의 APPNAME 날짜(YYYY-MM-DD).
    pub app_date: String,
    /// `xresult.inf` 의 APPTIME 값.
    pub app_time: String,
    /// 다시 구운 1비트 도면 TIFF 의 DateTime(306) — `YYYY:MM:DD HH:MM:SS`. 통특기는 변환 시각을 쓴다.
    /// 비우면 원본 값을 그대로 둔다.
    pub image_time: String,
    pub amendments: Vec<FinAmendment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmendmentEntry {
    /// 보정 대상 요소 이름(`AMENDABLE`).
    pub element_name: String,
    /// `A` = 살아 있는 요소를 갈기, `D` = 지우기, `I` = 살아 있지 않은 번호에 넣기(지운 항 되살리기 · 새 항).
    pub status: String,
    /// `num`(문단·청구항) · 비면 속성 없이(명칭·요약).
    pub attribute_name: String,
    pub attribute_value: String,
    /// 보정 본문. 삭제면 비운다.
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmendmentOptions {
    pub keaps_version: String,
    pub dta_version: String,
    pub editor_kind: String,
    pub doc_type: String,
    pub document_id: String,
    pub invention_ko_title: String,
    pub invention_en_title: String,
    /// 1비트 그림을 G4 로 다시 구울 때 TIFF `DateTime`(`YYYY:MM:DD HH:MM:SS`). 비우면 HLT 그림의 것을 둔다.
    /// 통특기는 굽는 시각을 쓴다(그 날짜를 주면 바이트 일치).
    pub image_time: String,
}

impl Default for AmendmentOptions {
    fn default() -> Self {
        Self {
            keaps_version: "5.6".into(),
            dta_version: "003".into(),
            editor_kind: "K".into(),
            doc_type: "PCTPatentCAFDOC".into(),
            document_id: "0".into(),
            invention_ko_title: String::new(),
            invention_en_title: String::new(),
            image_time: String::new(),
        }
    }
}

/// 명세서 `.hlz` = zip(KIPO XML + 도면 JPG).
pub fn build_hlz(
    xml_name: &str,
    export: &KipoExport,
    payloads: &BTreeMap<u32, HltImagePayload>,
    image_codes: &[u32],
    image_time: &str,
) -> Result<Vec<u8>, CoreError> {
    if image_codes.len() != export.images.len() {
        return Err(fin_error("image_code_count"));
    }
    let mut buffer = Vec::new();
    // 한글 XML 이름도 CP949 로 — UTF-8 깃발 이름이면 통특기(DynaZip)가 `.fin` 을 열다
    // 「Error creating output file」 로 멈춘다(이름만 CP949 로 바꾼 판은 열린다,
    // zip 속성을 DOS 로 바꾼 것만으로는 안 열린다).
    let mut names = EntryNames::new();
    {
        let mut zip = ZipWriter::new(Cursor::new(&mut buffer));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file(names.name(xml_name)?, options)
            .map_err(|_| fin_error("zip_start"))?;
        zip.write_all(export.xml.as_bytes())
            .map_err(|_| fin_error("zip_write"))?;
        for (image, code) in export.images.iter().zip(image_codes) {
            let payload = payloads
                .get(code)
                .ok_or_else(|| fin_error("missing_payload"))?;
            // 판독기가 픽셀을 못 낸 도면(1비트 그림)은 TIFF 그대로 싣되 CCITT G4 로 다시 굽는다(통특기와 같게 —
            // 태그는 원본 그대로, 압축·띠 길이·DateTime 만 바뀐다).
            // 이름의 확장자도 `export_kipo` 가 같은 판단으로 정한다.
            let bytes = encode_image(payload, image_time)?;
            zip.start_file(names.name(&image.file)?, options)
                .map_err(|_| fin_error("zip_start"))?;
            zip.write_all(&bytes).map_err(|_| fin_error("zip_write"))?;
        }
        zip.finish().map_err(|_| fin_error("zip_finish"))?;
    }
    names.patch(&mut buffer)?;
    Ok(buffer)
}

/// 제출파일 `.fin` = zip(명세서 hlz + 보정서 dta들 + xresult.inf).
pub fn build_fin(hlz: &[u8], options: &FinExportOptions) -> Result<Vec<u8>, CoreError> {
    let mut buffer = Vec::new();
    let mut names = EntryNames::new();
    {
        let mut zip = ZipWriter::new(Cursor::new(&mut buffer));
        let zip_options =
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file(names.name(&options.hlz_name)?, zip_options)
            .map_err(|_| fin_error("zip_start"))?;
        zip.write_all(hlz).map_err(|_| fin_error("zip_write"))?;
        for amendment in &options.amendments {
            zip.start_file(names.name(&amendment.file_name)?, zip_options)
                .map_err(|_| fin_error("zip_start"))?;
            zip.write_all(&amendment.bytes)
                .map_err(|_| fin_error("zip_write"))?;
        }
        zip.start_file("xresult.inf", zip_options)
            .map_err(|_| fin_error("zip_start"))?;
        zip.write_all(&build_xresult(options)?)
            .map_err(|_| fin_error("zip_write"))?;
        zip.finish().map_err(|_| fin_error("zip_finish"))?;
    }
    names.patch(&mut buffer)?;
    Ok(buffer)
}

/// `xresult.inf` 본문. 파일명에 한글이 들어가므로 CP949 로 쓴다(정본과 같은 인코딩).
pub fn build_xresult(options: &FinExportOptions) -> Result<Vec<u8>, CoreError> {
    let mut text = String::from("[APPLICATION]\r\n");
    text.push_str(&format!(
        "APPNAME={},{},1\r\n",
        options.hlz_name, options.app_date
    ));
    text.push_str(&format!("APPTIME={}\r\n", options.app_time));
    if !options.amendments.is_empty() {
        text.push_str("[AMENDMENT]\r\n");
        text.push_str(&format!("AMDCNT={}\r\n", options.amendments.len()));
        for (index, amendment) in options.amendments.iter().enumerate() {
            text.push_str(&format!(
                "AMD{:03}={},{},1\r\n",
                index + 1,
                amendment.file_name,
                amendment.date
            ));
        }
    }
    let (bytes, had_errors) = cp949(&text);
    if had_errors {
        return Err(fin_error("xresult_encoding"));
    }
    Ok(bytes)
}

/// 보정서 XML.
pub fn build_amendment_xml(entries: &[AmendmentEntry], options: &AmendmentOptions) -> String {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str(&format!(
        "<KIPO keapsVersion=\"{}\" dtaVersion=\"{}\" editorKind=\"{}\">\n",
        options.keaps_version, options.dta_version, options.editor_kind
    ));
    // docflag 는 명세서 뿌리와 같은 규칙 — PCT 가 아니면 붙는다(PCT dta 엔 없음).
    let doc_flag = if options.doc_type.starts_with("PCT") {
        ""
    } else {
        " docflag=\"1.0\""
    };
    xml.push_str(&format!(
        "<AmendDOC docType=\"{}\" documentID=\"{}\"{doc_flag}>\n",
        options.doc_type, options.document_id
    ));
    xml.push_str(&format!(
        "<InventionKOTitleName>{}</InventionKOTitleName>\n",
        options.invention_ko_title
    ));
    xml.push_str(&format!(
        "<InventionENTitleName>{}</InventionENTitleName>\n",
        options.invention_en_title
    ));
    xml.push_str("<Amendment>\n");
    for entry in entries {
        let attribute = if entry.attribute_name.is_empty() {
            String::new()
        } else {
            format!(
                " attributeName=\"{}\" attributeValue=\"{}\"",
                entry.attribute_name, entry.attribute_value
            )
        };
        xml.push_str(&format!(
            "<AmendBody elementName=\"{}\" status=\"{}\"{attribute}>\n",
            entry.element_name, entry.status
        ));
        // 통특기: 본문 바로 뒤에 닫는다(`</claim></AmendBody>`) · 파일 끝 줄바꿈 없음.
        xml.push_str(&entry.body);
        xml.push_str("</AmendBody>\n");
    }
    xml.push_str("</Amendment>\n</AmendDOC>\n</KIPO>");
    xml
}

/// 보정서 `.dta` = zip(보정서 XML). 안쪽 이름은 `.dta` 이름에서 확장자만 바꾼 것이다.
pub fn build_amendment_dta(dta_name: &str, xml: &str) -> Result<Vec<u8>, CoreError> {
    build_amendment_dta_with_images(dta_name, xml, &[])
}

/// 그림이 든 보정서: XML 뒤에 본문이 부르는 그림 파일을 처음 나온 차례로 싣는다
/// (예: dta = xml + pat00011~17.jpg · 빠지면 통특기가 불러올 때 그림 자리가 ☒).
pub fn build_amendment_dta_with_images(
    dta_name: &str,
    xml: &str,
    images: &[(String, Vec<u8>)],
) -> Result<Vec<u8>, CoreError> {
    let inner = dta_name
        .strip_suffix(".dta")
        .map(|stem| format!("{stem}.xml"))
        .unwrap_or_else(|| format!("{dta_name}.xml"));
    let mut buffer = Vec::new();
    let mut names = EntryNames::new();
    {
        let mut zip = ZipWriter::new(Cursor::new(&mut buffer));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file(names.name(&inner)?, options)
            .map_err(|_| fin_error("zip_start"))?;
        zip.write_all(xml.as_bytes())
            .map_err(|_| fin_error("zip_write"))?;
        for (name, bytes) in images {
            zip.start_file(names.name(name)?, options)
                .map_err(|_| fin_error("zip_start"))?;
            zip.write_all(bytes).map_err(|_| fin_error("zip_write"))?;
        }
        zip.finish().map_err(|_| fin_error("zip_finish"))?;
    }
    names.patch(&mut buffer)?;
    Ok(buffer)
}

/// 보정 후 HLT → KIPO XML(번호 = 식별번호) 과 그림 묶음(이름 → 그림 레코드 픽셀).
fn export_after(
    path: &Path,
    kipo: &KipoExportOptions,
) -> Result<(KipoExport, BTreeMap<String, HltImagePayload>), CoreError> {
    let mut document = inspect_path(
        path,
        InspectOptions {
            include_image_payloads: true,
            ..InspectOptions::default()
        },
    )?;
    // 서안이 구운 HLT 는 식별번호가 없다 — 편집 세션이 든 번호(같은 문단 차례)를 얹는다.
    // 다시 읽은 HLT 는 끝에 빈 문단 하나가 더 있다(모델에는 없다).
    let numbers = &kipo.paragraph_numbers;
    if !numbers.is_empty()
        && (numbers.len()..=numbers.len() + 1).contains(&document.paragraphs.len())
        && document
            .paragraphs
            .iter()
            .all(|paragraph| paragraph.wipo_id.is_none())
    {
        // 새 문단(`+`) = 통특기 식별번호 0xFFFFFFFF — 보정서에서 앞 번호 문단에 `<br/>` 로 잇는다.
        for (paragraph, number) in document.paragraphs.iter_mut().zip(numbers) {
            paragraph.wipo_id = match number.as_deref() {
                Some(crate::editor::NEW_PARAGRAPH) => Some(u32::MAX),
                Some(number) => number.parse().ok(),
                None => None,
            };
        }
    }
    let (formats, cell_formats) = inspect_format_inventories(path)?;
    let kipo = KipoExportOptions {
        number_by_id: true,
        ..kipo.clone()
    };
    let export = export_kipo_xml(&document, &formats, &cell_formats, &kipo)?;
    let codes: BTreeMap<&str, u32> = document
        .images
        .iter()
        .map(|image| (image.id.as_str(), image.object_code))
        .collect();
    let mut payloads = BTreeMap::new();
    for image in &export.images {
        let payload = codes
            .get(image.image_id.as_str())
            .and_then(|code| document.image_payloads.get(code))
            .ok_or_else(|| fin_error("missing_payload"))?;
        payloads.insert(image.file.clone(), payload.clone());
    }
    Ok((export, payloads))
}

/// 그림 하나를 다시 굽는다: 픽셀이 있으면 JPEG · 없으면(1비트) HLT 의 무압축 TIFF 를 CCITT G4 로
/// (통특기 보정서와 같은 날짜면 1비트 그림이 바이트 일치).
fn encode_image(payload: &HltImagePayload, image_time: &str) -> Result<Vec<u8>, CoreError> {
    if payload.pixels.is_empty() {
        tiff_to_g4(&payload.tiff, image_time)
    } else {
        encode_jpeg(payload)
    }
}

/// 마크업이 부르는 그림 파일 이름(차례대로).
fn image_files(markup: &str) -> Vec<&str> {
    markup
        .split(" file=\"")
        .skip(1)
        .filter_map(|part| part.split('"').next())
        .collect()
}

/// 마크업이 부르는 그림 크기(he, wi · mm · 차례대로).
fn image_sizes(markup: &str) -> Vec<(u32, u32)> {
    markup
        .split("<img ")
        .skip(1)
        .map(|tag| {
            let value = |key: &str| {
                tag.split_once(&format!(" {key}=\""))
                    .and_then(|(_, rest)| rest.split('"').next()?.parse().ok())
                    .unwrap_or(0)
            };
            (value("he"), value("wi"))
        })
        .collect()
}

/// 최종본에 실린 그림과 보정 후 HLT 그림이 같은 그림인가. JPEG 는 다시 구우면 바이트가 달라(통특기 인코더) 픽셀로 본다 —
/// 크기가 같고 평균 차가 8/255 이하(같은 그림을 다시 구운 차는 0.12 · ponytail: 아주 비슷한 다른 그림은 같다고 본다).
/// 1비트는 원본의 `DateTime` 으로 G4 를 다시 구워 바이트로 본다.
fn same_image(stored: &[u8], payload: &HltImagePayload) -> bool {
    if payload.pixels.is_empty() {
        let Some(time) = stored
            .windows(19)
            .find(|w| {
                w.iter().enumerate().all(|(i, b)| match i {
                    4 | 7 | 13 | 16 => *b == b':',
                    10 => *b == b' ',
                    _ => b.is_ascii_digit(),
                })
            })
            .and_then(|w| std::str::from_utf8(w).ok())
        else {
            return false;
        };
        return tiff_to_g4(&payload.tiff, time).is_ok_and(|ours| ours == stored);
    }
    let Ok(image) = image::load_from_memory(stored) else {
        return false;
    };
    if (image.width(), image.height()) != (payload.width, payload.height) {
        return false;
    }
    let theirs = if payload.samples == 1 {
        image.to_luma8().into_raw()
    } else {
        image.to_rgb8().into_raw()
    };
    theirs.len() == payload.pixels.len()
        && theirs
            .iter()
            .zip(&payload.pixels)
            .map(|(a, b)| u64::from(a.abs_diff(*b)))
            .sum::<u64>()
            <= 8 * theirs.len() as u64
}

/// 보정 본문이 부르는 그림을 처음 나온 차례로 다시 굽는다(통특기는 그대로인 그림도 다시 구워 싣는다).
/// `source` = 보정서 그림 이름 → 우리 XML 의 이름(없으면 같은 이름).
fn amendment_images(
    entries: &[AmendmentEntry],
    payloads: &BTreeMap<String, HltImagePayload>,
    source: &BTreeMap<String, String>,
    image_time: &str,
) -> Result<Vec<(String, Vec<u8>)>, CoreError> {
    let mut images: Vec<(String, Vec<u8>)> = Vec::new();
    for entry in entries {
        for name in image_files(&entry.body) {
            if images.iter().any(|(seen, _)| seen == name) {
                continue;
            }
            let ours = source.get(name).map_or(name, String::as_str);
            let payload = payloads
                .get(ours)
                .ok_or_else(|| fin_error("amendment_image_missing"))?;
            images.push((name.to_string(), encode_image(payload, image_time)?));
        }
    }
    Ok(images)
}

/// 보정서가 다루는 요소(문서 순서). 실물 보정서 327건(맥 로컬 · 저장소 밖)에서 이 넷이 321건, 도면(`figure`)이 2건 —
/// 인용 문헌 · 청구범위 통째(`claims`) · 서열목록은 4건뿐이라 다루지 않는다(앞선 보정서에 있으면 거절).
/// 도면 보정 = 청구항 뒤 `<figure num><img … file="pat000NN.jpg"/></figure>` · 그림은 dta 에(최종본 그림 다음 번호).
pub const AMENDABLE: [&str; 5] = ["invention-title", "p", "claim", "summary", "figure"];

/// 요소 자리 = (`AMENDABLE` 순서, 번호 길이, 번호) — 정렬하면 문서 순서다(청구항은 수 크기 · 문단은 0 채운 네 자리).
pub type ElementKey = (usize, usize, String);

/// KIPO XML 에서 보정 대상 요소의 마크업(여는 태그부터 닫는 태그까지)을 뽑는다. 문단은 `<description>` 안의 것만
/// (요약서의 `<p num="0001a">` 는 `summary` 통째로 다룬다).
pub fn amendable_elements(xml: &str) -> BTreeMap<ElementKey, String> {
    let mut out = BTreeMap::new();
    let mut take = |rank: usize, text: &str, open: &str, close: &str, numbered: bool| {
        let mut rest = text;
        while let Some(at) = rest.find(open) {
            let Some(end) = rest[at..].find(close) else {
                break;
            };
            let markup = &rest[at..at + end + close.len()];
            let number = if numbered {
                markup[open.len()..].split('"').next().unwrap_or_default()
            } else {
                ""
            };
            out.insert((rank, number.len(), number.to_string()), markup.to_string());
            rest = &rest[at + end + close.len()..];
            if !numbered {
                break;
            }
        }
    };
    take(0, xml, "<invention-title>", "</invention-title>", false);
    let description = xml
        .split_once("<description>")
        .and_then(|(_, rest)| rest.split_once("</description>"))
        .map_or("", |(body, _)| body);
    take(1, description, "<p num=\"", "</p>", true);
    take(2, xml, "<claim num=\"", "</claim>", true);
    take(3, xml, "<summary>", "</summary>", false);
    let drawings = xml
        .split_once("<drawings>")
        .and_then(|(_, rest)| rest.split_once("</drawings>"))
        .map_or("", |(body, _)| body);
    take(4, drawings, "<figure num=\"", "</figure>", true);
    out
}

/// 보정서 하나를 상태(자리 → 마크업 · `None` = 지운 자리)에 적용한다. `A` 는 살아 있는 요소만, `D` 도 살아 있는 요소만,
/// `I` 는 살아 있지 않은 자리만(실물 327건: A 941/943 · D 1111/1112 · I 34/34 — 어긋난 셋은 `claims` 통째 보정 뒤).
pub fn apply_amendment(
    state: &mut BTreeMap<ElementKey, Option<String>>,
    dta_xml: &str,
) -> Result<(), CoreError> {
    for body in dta_xml.split("<AmendBody ").skip(1) {
        let (head, rest) = body.split_once('>').unwrap_or((body, ""));
        let attr = |key: &str| {
            head.split_once(&format!("{key}=\""))
                .and_then(|(_, rest)| rest.split_once('"'))
                .map_or(String::new(), |(value, _)| value.to_string())
        };
        let kind = attr("elementName");
        let Some(rank) = AMENDABLE.iter().position(|name| *name == kind) else {
            let mut error = fin_error("previous_dta_element");
            error.details.insert("element".into(), kind);
            return Err(error);
        };
        let number = attr("attributeValue");
        let markup = rest.split("</AmendBody>").next().unwrap_or_default().trim();
        let key = (rank, number.len(), number);
        let live = matches!(state.get(&key), Some(Some(_)));
        let value = match (attr("status").as_str(), live) {
            ("A", true) | ("I", false) if !markup.is_empty() => Some(markup.to_string()),
            ("D", true) => None,
            _ => return Err(fin_error("previous_dta_entry")),
        };
        state.insert(key, value);
    }
    Ok(())
}

/// dtaVersion = 이 보정서의 순번 + 1 — 앞선 보정서가 `previous` 개면 `previous + 2`(첫 002 · 둘째 003 · 셋째 004 ·
/// 넷째 005 — 실물 통특기 보정서 320건 전부).
pub fn dta_version(previous: usize) -> String {
    format!("{:03}", previous + 2)
}

/// 보정서 머리의 국문·영문 명칭 = 보정 전 상태의 `invention-title`「국문{영문}」(앞선 보정서가 바꿨으면 바뀐 것).
/// 영문은 마지막 `}` 만 뺀다 — 뒤 공백은 남는다(실물 `…EXAM} ` → `…EXAM `).
pub fn invention_titles(state: &BTreeMap<ElementKey, Option<String>>) -> (String, String) {
    let title = state
        .get(&(0, 0, String::new()))
        .and_then(Option::as_deref)
        .and_then(|markup| markup.strip_prefix("<invention-title>"))
        .and_then(|markup| markup.strip_suffix("</invention-title>"))
        .unwrap_or_default();
    match title.split_once('{') {
        Some((ko, en)) => {
            let en = en
                .rsplit_once('}')
                .map_or(en.to_string(), |(head, tail)| format!("{head}{tail}"));
            (ko.to_string(), en)
        }
        // 중괄호가 없으면(PCT 국내진입 — 명칭이 국문 하나) 통째로 영문 칸에 간다(실물 PCT 4건 · pct_c 도 같다).
        None => (String::new(), title.to_string()),
    }
}

/// 바뀌었는지 = 태그 이름과 글자의 차례(속성·줄바꿈·`<maths>` 감싸기는 뺀다). 최종본 명세서 XML 은 출원 때 통특기가 낸 것이라
/// 우리 XML 과 줄바꿈(CRLF)·수식 감싸기·그림 형식(jpg↔tif 속성)·표 줄 배치가 다르다 — 통특기는 amend_a 에서 이것들을
/// 보정으로 치지 않았다(청구항만 8). 반대로 끝에 `<br/>` 만 는 청구항 · 열 수가 바뀐 표는 보정으로 쳤다(실물 3건).
/// ⛔ 그림 내용·속성만 바뀐 요소는 보정으로 잡히지 않는다.
/// 통특기 보정서는 표를 다시 직렬화한다: 태그 사이 줄바꿈은 `<tgroup …>` · `<colspec …/>` · `<row>` · `</entry>` 뒤에만 남고(`</tables>` 뒤도 없다)
/// `xmlns` 는 큰따옴표(제출 XML 은 줄마다 바꾸고 작은따옴표).
fn amendment_tables(markup: &str) -> String {
    let mut out = String::with_capacity(markup.len());
    let mut rest = markup;
    while let Some(start) = rest.find("<tables") {
        let end = rest[start..]
            .find("</tables>")
            .map_or(rest.len(), |end| start + end + "</tables>".len());
        out.push_str(&rest[..start]);
        let table = rest[start..end].replace(
            "xmlns='http://www.oasis-open.org/tables/exchange/1.0'",
            "xmlns=\"http://www.oasis-open.org/tables/exchange/1.0\"",
        );
        for (index, line) in table.split('\n').enumerate() {
            if index > 0 {
                let previous = out.rsplit_once('<').map_or("", |(_, tag)| tag);
                let keep = (previous.starts_with("tgroup ") && previous.ends_with('>'))
                    || (previous.starts_with("colspec ") && previous.ends_with("/>"))
                    || previous == "row>"
                    || previous == "/entry>";
                if keep {
                    out.push('\n');
                }
            }
            out.push_str(line);
        }
        // `</tables></p>` 도 붙는다.
        rest = rest[end..].strip_prefix('\n').unwrap_or(&rest[end..]);
    }
    out.push_str(rest);
    out
}

fn comparable(markup: &str) -> String {
    let mut out = String::new();
    let mut rest = markup;
    while let Some(at) = rest.find('<') {
        out.extend(rest[..at].chars().filter(|ch| !matches!(ch, '\r' | '\n')));
        let Some(end) = rest[at..].find('>') else {
            break;
        };
        let tag = &rest[at + 1..at + end];
        let name = tag
            .trim_start_matches('/')
            .split([' ', '/'])
            .next()
            .unwrap_or_default();
        if name != "maths" {
            let close = if tag.starts_with('/') { "/" } else { "" };
            // 같은 서식이 붙어 이어지면(`<u>가 </u><u>나</u>`) 한 구간이다 — 구간을 나눈 자리만 다른 것은 같은 글.
            let joint = format!("</{name}>");
            let format = matches!(name, "b" | "i" | "u" | "sup" | "sub" | "B" | "I" | "U");
            if format && close.is_empty() && out.ends_with(&joint) {
                out.truncate(out.len() - joint.len());
            } else {
                out.push_str(&format!("<{close}{name}>"));
            }
        }
        rest = &rest[at + end + 1..];
    }
    out.extend(rest.chars().filter(|ch| !matches!(ch, '\r' | '\n')));
    // 문헌 글 앞 공백은 보지 않는다 — 통특기 줄은 `<text> 홍길동` 의 앞 공백을 지운다(열고 다시 내도 같은 문헌).
    while out.contains("<text> ") {
        out = out.replace("<text> ", "<text>");
    }
    out
}

/// 보정 전 상태와 보정 후 요소를 대조해 보정 항목을 만든다(문서 순서). 바뀐 요소는 `A`(본문 통째) · 없어진 요소는
/// `D`(빈 본문) · 살아 있지 않던 자리에 생긴 요소는 `I`. 그대로인 것은 싣지 않는다.
pub fn diff_elements(
    before: &BTreeMap<ElementKey, Option<String>>,
    after: &BTreeMap<ElementKey, String>,
) -> Vec<AmendmentEntry> {
    diff_elements_by(before, after, |old, new| comparable(old) == comparable(new))
}

/// 그림 이름이 정체인 상태끼리(최종본 + 보정서 재생 — 그대로인 그림은 이름도 그대로) 대조한다: 글자·태그에 더해
/// `<img …/>` 가 통째로(이름 · 크기) 같아야 같은 요소다 — 그림만 바꾼 도면 · 크기만 바꾼 도면도 `figure A`(실물 2건).
pub fn diff_elements_named(
    before: &BTreeMap<ElementKey, Option<String>>,
    after: &BTreeMap<ElementKey, String>,
) -> Vec<AmendmentEntry> {
    let images = |markup: &str| -> Vec<String> {
        markup
            .split("<img ")
            .skip(1)
            .map(|rest| rest.split("/>").next().unwrap_or_default().to_string())
            .collect()
    };
    diff_elements_by(before, after, |old, new| {
        comparable(old) == comparable(new) && images(old) == images(new)
    })
}

/// `diff_elements` 에 「같은 요소인가」 판정을 넘긴다(그림 정체까지 보는 최종본 경로).
fn diff_elements_by(
    before: &BTreeMap<ElementKey, Option<String>>,
    after: &BTreeMap<ElementKey, String>,
    same: impl Fn(&str, &str) -> bool,
) -> Vec<AmendmentEntry> {
    let keys: std::collections::BTreeSet<&ElementKey> = before.keys().chain(after.keys()).collect();
    let mut entries = Vec::new();
    for key in keys {
        let old = before.get(key).and_then(Option::as_ref);
        let new = after.get(key);
        let status = match (old, new) {
            (Some(old), Some(new)) if !same(old, new) => "A",
            (Some(_), None) => "D",
            (None, Some(_)) => "I",
            _ => continue,
        };
        let kind = AMENDABLE[key.0];
        let numbered = !key.2.is_empty();
        // 통특기: 문단 · 도면만 본문 뒤에 줄을 바꾼다(`</p>\n</AmendBody>` · 청구항·명칭·요약은 `…></AmendBody>`).
        let body = new.map_or(String::new(), |markup| {
            let markup = amendment_tables(markup);
            if kind == "p" || kind == "figure" {
                format!("{markup}\n")
            } else {
                markup
            }
        });
        entries.push(AmendmentEntry {
            element_name: kind.into(),
            status: status.into(),
            attribute_name: if numbered {
                "num".into()
            } else {
                String::new()
            },
            attribute_value: key.2.clone(),
            body,
        });
    }
    entries
}

/// 통특기처럼 굽는다: 통특기 양자화표 · 4:2:0 · 96dpi · 표준 허프만표(최적화 허프만은 `image` 가 못 읽는 JPEG 를 냈다).
fn encode_jpeg(payload: &HltImagePayload) -> Result<Vec<u8>, CoreError> {
    use jpeg_encoder::{Encoder, PixelDensity, QuantizationTableType, SamplingFactor};
    let color = match (payload.samples, payload.bits) {
        (1, 8) => jpeg_encoder::ColorType::Luma,
        (3, 8) => jpeg_encoder::ColorType::Rgb,
        _ => return Err(fin_error("pixel_format")),
    };
    let size = |value: u32| u16::try_from(value).map_err(|_| fin_error("jpeg_size"));
    let mut out = Vec::new();
    let mut encoder = Encoder::new(&mut out, 100);
    encoder.set_sampling_factor(SamplingFactor::R_4_2_0);
    encoder.set_quantization_tables(
        QuantizationTableType::Custom(Box::new(NK_LUMA_TABLE)),
        QuantizationTableType::Custom(Box::new(NK_CHROMA_TABLE)),
    );
    encoder.set_density(PixelDensity::dpi(96));
    encoder
        .encode(
            &payload.pixels,
            size(payload.width)?,
            size(payload.height)?,
            color,
        )
        .map_err(|_| fin_error("jpeg_encode"))?;
    Ok(out)
}

/// 무압축 1비트 TIFF(II · 띠 하나 · 띠가 파일 끝)를 CCITT G4(T.6)로 다시 굽는다. 다른 꼴은 거절한다.
fn tiff_to_g4(tiff: &[u8], date_time: &str) -> Result<Vec<u8>, CoreError> {
    let bad = || fin_error("tiff_layout");
    let u16_at = |at: usize| -> Result<u32, CoreError> {
        Ok(u32::from(u16::from_le_bytes(
            tiff.get(at..at + 2).ok_or_else(bad)?.try_into().unwrap(),
        )))
    };
    let u32_at = |at: usize| -> Result<u32, CoreError> {
        Ok(u32::from_le_bytes(
            tiff.get(at..at + 4).ok_or_else(bad)?.try_into().unwrap(),
        ))
    };
    if tiff.get(..4) != Some(b"II*\0") {
        return Err(bad());
    }
    let ifd = u32_at(4)? as usize;
    let mut tags = BTreeMap::new();
    for entry in 0..u16_at(ifd)? as usize {
        let at = ifd + 2 + 12 * entry;
        let (kind, count) = (u16_at(at + 2)?, u32_at(at + 4)?);
        // (값 자리, 값) — 짧은 형(SHORT)은 값 칸 앞 2바이트.
        let value = if kind == 3 && count == 1 {
            u16_at(at + 8)?
        } else {
            u32_at(at + 8)?
        };
        tags.insert(u16_at(at)?, (at + 8, kind, count, value));
    }
    let tag = |id: u32| tags.get(&id).copied().ok_or_else(bad);
    let (width, height) = (tag(256)?.3 as usize, tag(257)?.3 as usize);
    let (strip_at, strip_len) = (tag(273)?, tag(279)?);
    if tag(258)?.3 != 1
        || tag(259)?.3 != 1
        || strip_at.2 != 1
        || strip_len.2 != 1
        || strip_at.3 as usize + strip_len.3 as usize != tiff.len()
        || tags.get(&266).is_some_and(|fill| fill.3 != 1)
        || width > usize::from(u16::MAX)
    {
        return Err(bad());
    }
    let row = width.div_ceil(8);
    let pixels = tiff.get(strip_at.3 as usize..).ok_or_else(bad)?;
    if pixels.len() < row * height {
        return Err(bad());
    }
    // 측광 0(WhiteIsZero) 이면 비트 0 = 흰색.
    let white_bit = u8::from(tag(262)?.3 != 0);
    let mut encoder = fax::encoder::Encoder::new(fax::VecWriter::new());
    for line in pixels[..row * height].chunks_exact(row) {
        let pels = (0..width).map(|x| {
            if (line[x / 8] >> (7 - x % 8)) & 1 == white_bit {
                fax::Color::White
            } else {
                fax::Color::Black
            }
        });
        let Ok(()) = encoder.encode_line(pels, width as u16);
    }
    let Ok(writer) = encoder.finish();
    let g4 = writer.finish();
    let mut out = tiff[..strip_at.3 as usize].to_vec();
    out[tag(259)?.0..tag(259)?.0 + 2].copy_from_slice(&4_u16.to_le_bytes());
    out[strip_len.0..strip_len.0 + 4].copy_from_slice(&(g4.len() as u32).to_le_bytes());
    if !date_time.is_empty() {
        let (_, kind, count, offset) = tag(306)?;
        let text = format!("{date_time}\0");
        if kind != 2 || count as usize != text.len() {
            return Err(fin_error("tiff_date_time"));
        }
        out[offset as usize..offset as usize + text.len()].copy_from_slice(text.as_bytes());
    }
    out.extend_from_slice(&g4);
    Ok(out)
}

/// 이름을 CP949 로. 맥 파일 이름은 한글이 **풀어쓴 자모(NFD)** 로 온다(저장 대화상자 「제출」 = U+110C U+1166 …) —
/// 그 자모는 CP949 에 없어 모아쓴 음절(NFC)로 먼저 합친다(2026-10-14 사용자 실패 `entry_name_encoding`).
fn cp949(text: &str) -> (Vec<u8>, bool) {
    let composed = compose_hangul(text);
    let (bytes, _, had_errors) = EUC_KR.encode(&composed);
    (bytes.into_owned(), had_errors)
}

/// 한글 자모 모아쓰기(유니코드 한글 음절 산식: 초성 19 × 중성 21 × 종성 28). 한글 밖 글자는 그대로.
pub(crate) fn compose_hangul(text: &str) -> String {
    let mut out: Vec<char> = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let code = ch as u32;
        if let Some(&last) = out.last() {
            let previous = last as u32;
            // 초성 + 중성 → 음절
            if (0x1100..0x1113).contains(&previous) && (0x1161..0x1176).contains(&code) {
                let syllable = 0xAC00 + ((previous - 0x1100) * 21 + (code - 0x1161)) * 28;
                *out.last_mut().unwrap() = char::from_u32(syllable).unwrap();
                continue;
            }
            // 받침 없는 음절 + 종성 → 받침 음절
            if (0xAC00..=0xD7A3).contains(&previous)
                && (previous - 0xAC00) % 28 == 0
                && (0x11A8..0x11C3).contains(&code)
            {
                *out.last_mut().unwrap() = char::from_u32(previous + code - 0x11A7).unwrap();
                continue;
            }
        }
        out.push(ch);
    }
    out.into_iter().collect()
}

/// zip 항목 이름을 통특기처럼 CP949 바이트로(UTF-8 깃발 없이) 쓰기 위한 자리표시.
/// 통특기 `.fin` 의 한글 항목 이름은 CP949 원 바이트다(`amend_a_보정.dta` · 깃발 0). `zip` 크레이트는 한글 이름을
/// UTF-8 + 깃발로 쓰므로, 같은 바이트 길이의 ASCII 자리표시로 쓴 뒤 `patch_entry_names` 가 바이트를 바꾼다(길이 같음 → 자리 불변).
struct EntryNames {
    patches: Vec<(Vec<u8>, Vec<u8>)>,
}

impl EntryNames {
    fn new() -> Self {
        Self {
            patches: Vec::new(),
        }
    }

    /// zip 에 넘길 이름. ASCII 면 그대로.
    fn name(&mut self, name: &str) -> Result<String, CoreError> {
        if name.is_ascii() {
            return Ok(name.to_string());
        }
        let (cp949, had_errors) = cp949(name);
        if had_errors {
            return Err(fin_error("entry_name_encoding"));
        }
        self.raw(&cp949)
    }

    /// 원 바이트 이름(다른 `.fin` 에서 그대로 옮기는 항목).
    fn raw(&mut self, cp949: &[u8]) -> Result<String, CoreError> {
        if cp949.is_ascii() {
            return Ok(String::from_utf8_lossy(cp949).into_owned());
        }
        let mut placeholder = format!("~{}~", self.patches.len());
        if placeholder.len() > cp949.len() {
            return Err(fin_error("entry_name_short"));
        }
        while placeholder.len() < cp949.len() {
            placeholder.push('#');
        }
        self.patches
            .push((placeholder.clone().into_bytes(), cp949.to_vec()));
        Ok(placeholder)
    }

    /// 자리표시마다 정확히 두 번(지역 머리 · 중앙 목록) 나와야 한다.
    fn patch(self, zip: &mut [u8]) -> Result<(), CoreError> {
        for (placeholder, cp949) in self.patches {
            let hits: Vec<usize> = zip
                .windows(placeholder.len())
                .enumerate()
                .filter(|(_, window)| *window == placeholder.as_slice())
                .map(|(at, _)| at)
                .collect();
            if hits.len() != 2 {
                return Err(fin_error("entry_name_patch"));
            }
            for at in hits {
                zip[at..at + cp949.len()].copy_from_slice(&cp949);
            }
        }
        Ok(())
    }
}

fn fin_error(reason: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::StructureInvalid, "export_fin", false);
    error.details.insert("reason".into(), reason.into());
    error
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinExport {
    pub fin: Vec<u8>,
    /// `.fin` 에 든 `.hlz` 그대로 — 통특기처럼 `.hlz` 를 먼저 따로 쓴다.
    pub hlz: Vec<u8>,
    pub hlz_bytes: usize,
    pub xml_bytes: usize,
    pub image_count: usize,
    pub paragraph_count: u32,
    pub claim_count: u32,
    pub figure_count: u32,
}

/// HLT 하나로 제출파일 `.fin` 을 만든다: 판독 → KIPO XML → 도면 JPEG → `.hlz` → `.fin`.
pub fn export_fin_path(
    path: &Path,
    kipo: &KipoExportOptions,
    fin: &FinExportOptions,
) -> Result<FinExport, CoreError> {
    let document = inspect_path(
        path,
        InspectOptions {
            include_image_payloads: true,
            ..InspectOptions::default()
        },
    )?;
    let kipo = crate::export_kipo::with_computed_page_count(path, kipo)?;
    // 의견서(종류 7)는 `OpinionDOC` 방언 — 명세서 경로로 가면 절이 없어 `no_section` 으로 멈췄다(2026-10-16).
    let export = if crate::export_opinion::is_opinion(&document) {
        crate::export_opinion::export_opinion_xml_path(path, &kipo)?
    } else {
        let (formats, cell_formats) = inspect_format_inventories(path)?;
        export_kipo_xml(&document, &formats, &cell_formats, &kipo)?
    };
    let codes: BTreeMap<&str, u32> = document
        .images
        .iter()
        .map(|image| (image.id.as_str(), image.object_code))
        .collect();
    let image_codes = export
        .images
        .iter()
        .map(|image| {
            codes
                .get(image.image_id.as_str())
                .copied()
                .ok_or_else(|| fin_error("image_object_code"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let xml_name = fin
        .hlz_name
        .strip_suffix(".hlz")
        .map(|stem| format!("{stem}.xml"))
        .unwrap_or_else(|| "document.xml".to_string());
    let hlz = build_hlz(
        &xml_name,
        &export,
        &document.image_payloads,
        &image_codes,
        &fin.image_time,
    )?;
    let bytes = build_fin(&hlz, fin)?;
    Ok(FinExport {
        fin: bytes,
        hlz_bytes: hlz.len(),
        hlz,
        xml_bytes: export.xml.len(),
        image_count: export.images.len(),
        paragraph_count: export.paragraph_count,
        claim_count: export.claim_count,
        figure_count: export.figure_count,
    })
}

/// 바이트열의 SHA-256 을 대문자 16진수로 준다 (CLI 보고용).
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:X}", Sha256::digest(bytes))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmendmentExport {
    pub xml: String,
    pub dta: Vec<u8>,
    pub amended: usize,
    pub deleted: usize,
}

/// 보정 전 HLT 와 보정 후 HLT 를 요소(명칭·문단·청구항·요약) 단위로 대조해 보정서 `.dta` 를 만든다.
pub fn export_amendment_path(
    before: &Path,
    after: &Path,
    dta_name: &str,
    kipo: &KipoExportOptions,
    options: &AmendmentOptions,
) -> Result<AmendmentExport, CoreError> {
    let before = amendable_elements(
        &crate::export_kipo_xml_path(
            before,
            &KipoExportOptions {
                number_by_id: true,
                ..kipo.clone()
            },
        )?
        .xml,
    )
    .into_iter()
    .map(|(key, markup)| (key, Some(markup)))
    .collect();
    let (after_export, payloads) = export_after(after, kipo)?;
    let after = amendable_elements(&after_export.xml);
    let entries = diff_elements(&before, &after);
    let amended = entries.iter().filter(|entry| entry.status == "A").count();
    let deleted = entries.len() - amended;
    let xml = build_amendment_xml(&entries, options);
    let images = amendment_images(&entries, &payloads, &BTreeMap::new(), &options.image_time)?;
    let dta = build_amendment_dta_with_images(dta_name, &xml, &images)?;
    Ok(AmendmentExport {
        xml,
        dta,
        amended,
        deleted,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinAmendmentExport {
    /// 보정서를 더한 새 `.fin`.
    pub fin: Vec<u8>,
    pub xml: String,
    /// `.fin` 밖에 따로 두는 `.dta`(통특기도 같은 폴더에 낸다).
    pub dta: Vec<u8>,
    pub amended: usize,
    pub deleted: usize,
}

/// 최종본 `.fin` 읽기: 항목 이름 · `xresult.inf` · 명세서 hlz 의 XML · 그림, 그리고 지금까지의 보정서를
/// xresult 의 AMD 차례로 적용한 요소 상태(보정서생성 · 「fin 열기」가 같이 쓴다).
struct FinalFin {
    names: Vec<String>,
    xresult: String,
    amendments: Vec<String>,
    before_xml: String,
    before: BTreeMap<ElementKey, Option<String>>,
    stored: BTreeMap<String, Vec<u8>>,
    /// zip 밖(fin 옆)에서 읽은 보정서 — 다음 최종본에 싣는다(옆 파일 없이도 열리게).
    outside: Vec<(String, Vec<u8>)>,
}

/// fin 옆에서 보정서를 찾는다: ① xresult 가 부르는데 zip 에 없는 dta(같은 이름) · ② 「…첨부.fin」 옆
/// 「…(첨부를 뺀 이름).dta」(통특기처럼 마지막 보정으로 겹친다 — 사용자 2026-10-23).
fn beside(fin: Option<&Path>, name: &str) -> Option<Vec<u8>> {
    std::fs::read(fin?.parent()?.join(name)).ok()
}

/// 파일 수정일(YYYY-MM-DD · UTC) — 옆 dta 를 xresult 에 올릴 때.
fn modified_date(path: &Path) -> String {
    let days = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_secs() / 86_400) as i64;
    // 날짜 ← 1970-01-01 부터 날 수(Hinnant civil_from_days).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn read_final_fin<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    fin: Option<&Path>,
) -> Result<FinalFin, CoreError> {
    use std::io::Read;
    let mut names = Vec::new();
    let mut xresult = String::new();
    let mut hlz = Vec::new();
    let mut previous: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|_| fin_error("fin_zip"))?;
        let name = entry.name_raw().to_vec();
        let (name, _, _) = EUC_KR.decode(&name);
        let name = name.into_owned();
        if name == "xresult.inf" {
            let mut raw = Vec::new();
            entry
                .read_to_end(&mut raw)
                .map_err(|_| fin_error("fin_read"))?;
            xresult = EUC_KR.decode(&raw).0.into_owned();
        } else if name.ends_with(".hlz") {
            entry
                .read_to_end(&mut hlz)
                .map_err(|_| fin_error("fin_read"))?;
        } else if name.ends_with(".dta") {
            let mut raw = Vec::new();
            entry
                .read_to_end(&mut raw)
                .map_err(|_| fin_error("fin_read"))?;
            previous.insert(name.clone(), raw);
        }
        names.push(name);
    }
    let mut amendments: Vec<String> = xresult
        .lines()
        .filter(|line| line.starts_with("AMD") && !line.starts_with("AMDCNT="))
        .filter_map(|line| line.split_once('=').map(|(_, value)| value.to_string()))
        .collect();
    let mut outside = Vec::new();
    // ① xresult 가 부르는데 zip 에 없는 보정서 → fin 옆 같은 이름.
    for value in &amendments {
        let file = value.split(',').next().unwrap_or_default();
        if !previous.contains_key(file)
            && let Some(bytes) = beside(fin, file)
        {
            previous.insert(file.to_string(), bytes.clone());
            outside.push((file.to_string(), bytes));
        }
    }
    // ② 「…첨부.fin」 옆 「…(첨부를 뺀 이름).dta」 → 마지막 보정.
    let attached = fin
        .and_then(|path| path.file_stem())
        .and_then(|stem| stem.to_str())
        .and_then(|stem| stem.strip_suffix("첨부"))
        .map(|stem| format!("{stem}.dta"));
    if let Some(name) = attached
        && !amendments
            .iter()
            .any(|value| value.split(',').next() == Some(name.as_str()))
        && let Some(bytes) = beside(fin, &name)
    {
        let date = modified_date(
            &fin.and_then(Path::parent)
                .unwrap_or(Path::new("."))
                .join(&name),
        );
        amendments.push(format!("{name},{date},1"));
        previous.insert(name.clone(), bytes.clone());
        outside.push((name, bytes));
    }
    let mut hlz_archive =
        zip::ZipArchive::new(Cursor::new(hlz)).map_err(|_| fin_error("hlz_zip"))?;
    let mut before_xml = String::new();
    // 최종본에 실린 그림(명세서 hlz + 앞선 보정서) — 보정 후 그림이 그대로인지 대조하고 새 그림 번호를 이어 매긴다.
    let mut stored: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for index in 0..hlz_archive.len() {
        let mut entry = hlz_archive
            .by_index(index)
            .map_err(|_| fin_error("hlz_zip"))?;
        if entry.name().ends_with(".xml") {
            entry
                .read_to_string(&mut before_xml)
                .map_err(|_| fin_error("hlz_read"))?;
        } else {
            let name = entry.name().to_string();
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .map_err(|_| fin_error("hlz_read"))?;
            stored.insert(name, bytes);
        }
    }
    let mut before: BTreeMap<ElementKey, Option<String>> = amendable_elements(&before_xml)
        .into_iter()
        .map(|(key, markup)| (key, Some(markup)))
        .collect();
    // 최종본 = 명세서 + 지금까지의 보정서(xresult 의 AMD 순서 — zip 항목 순서와 다를 수 있다).
    for value in &amendments {
        let file = value.split(',').next().unwrap_or_default();
        let dta = previous
            .get(file)
            .ok_or_else(|| fin_error("previous_dta"))?;
        // 보정서 XML 이 첫 항목이라는 법은 없다(옛 통특기 도면 보정 = 그림 여섯 장 뒤에 XML).
        let mut inner = zip::ZipArchive::new(Cursor::new(dta)).map_err(|_| fin_error("dta_zip"))?;
        let mut xml = String::new();
        for index in 0..inner.len() {
            let mut entry = inner.by_index(index).map_err(|_| fin_error("dta_zip"))?;
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .map_err(|_| fin_error("dta_read"))?;
            if entry.name().to_ascii_lowercase().ends_with(".xml") {
                xml = String::from_utf8(bytes).map_err(|_| fin_error("dta_read"))?;
            } else {
                stored.insert(entry.name().to_string(), bytes);
            }
        }
        apply_amendment(&mut before, &xml)?;
    }
    Ok(FinalFin {
        names,
        xresult,
        amendments,
        before_xml,
        before,
        stored,
        outside,
    })
}

/// 최종본 `.fin` → 최종본 KIPO XML(명세서 + 보정서를 차례로 적용) · 그림 파일(명세서 hlz + 보정서).
/// 바뀐 요소는 그 자리에서 바꾸고(A) · 지운 요소는 빼고(D) · 새 요소(I)는 문서 차례로 바로 앞 요소 뒤에 넣는다.
pub fn final_version_from_fin(
    path: &Path,
) -> Result<(String, BTreeMap<String, Vec<u8>>), CoreError> {
    let file = std::fs::File::open(path).map_err(|_| fin_error("fin_open"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| fin_error("fin_zip"))?;
    let FinalFin {
        before_xml,
        before: state,
        stored,
        ..
    } = read_final_fin(&mut archive, Some(path))?;
    if before_xml.is_empty() {
        return Err(fin_error("fin_no_specification"));
    }
    Ok((rebuild_xml(&before_xml, &state)?, stored))
}

/// 최종본 `.fin` 의 원출원(보정서를 적용하기 전 명세서 hlz XML) · 그림 — 「원출원에 견준 보정 표시」.
pub fn original_version_from_fin(
    path: &Path,
) -> Result<(String, BTreeMap<String, Vec<u8>>), CoreError> {
    let file = std::fs::File::open(path).map_err(|_| fin_error("fin_open"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| fin_error("fin_zip"))?;
    let FinalFin {
        before_xml, stored, ..
    } = read_final_fin(&mut archive, Some(path))?;
    if before_xml.is_empty() {
        return Err(fin_error("fin_no_specification"));
    }
    Ok((before_xml, stored))
}

fn rebuild_xml(
    original: &str,
    state: &BTreeMap<ElementKey, Option<String>>,
) -> Result<String, CoreError> {
    let mut xml = original.to_string();
    let originals = amendable_elements(original);
    // 지금 XML 에 있는 각 자리의 마크업(바꾸면 따라 바뀐다) — 새 요소를 넣을 앞 자리를 찾는다.
    let mut present: BTreeMap<ElementKey, String> = originals.clone();
    for (key, value) in state {
        match (originals.get(key), value) {
            (Some(old), Some(new)) if old != new => {
                xml = xml.replacen(old.as_str(), new, 1);
                present.insert(key.clone(), new.clone());
            }
            (Some(old), None) => {
                xml = xml.replacen(old.as_str(), "", 1);
                present.remove(key);
            }
            (None, Some(new)) => {
                let (_, anchor) = present
                    .range(..key.clone())
                    .rev()
                    .find(|(other, _)| other.0 == key.0)
                    .ok_or_else(|| fin_error("final_insert_anchor"))?;
                let at = xml
                    .find(anchor.as_str())
                    .ok_or_else(|| fin_error("final_insert_anchor"))?
                    + anchor.len();
                xml.insert_str(at, &format!("\n{new}"));
                present.insert(key.clone(), new.clone());
            }
            _ => {}
        }
    }
    Ok(xml)
}

#[cfg(test)]
mod final_version_tests {
    use super::*;

    #[test]
    fn rebuilds_amended_deleted_and_inserted_elements() {
        let original = "<KIPO><X><description><p num=\"0001\">가</p>\n<p num=\"0002\">나</p>\n<p num=\"0003\">다</p></description>\
<claims><claim num=\"1\"><claim-text>청1</claim-text></claim></claims></X></KIPO>";
        let mut state: BTreeMap<ElementKey, Option<String>> = amendable_elements(original)
            .into_iter()
            .map(|(key, value)| (key, Some(value)))
            .collect();
        let dta = "<AmendBody elementName=\"p\" attributeValue=\"0002\" status=\"A\"><p num=\"0002\">나나</p></AmendBody>\
<AmendBody elementName=\"p\" attributeValue=\"0003\" status=\"D\"></AmendBody>\
<AmendBody elementName=\"claim\" attributeValue=\"2\" status=\"I\"><claim num=\"2\"><claim-text>청2</claim-text></claim></AmendBody>";
        apply_amendment(&mut state, dta).unwrap();
        let rebuilt = rebuild_xml(original, &state).unwrap();
        assert!(rebuilt.contains("<p num=\"0002\">나나</p>"));
        assert!(!rebuilt.contains("0003"));
        let one = rebuilt.find("<claim num=\"1\">").unwrap();
        let two = rebuilt.find("<claim num=\"2\">").unwrap();
        assert!(one < two && rebuilt[two..].starts_with("<claim num=\"2\"><claim-text>청2"));
    }
}

/// 보정서생성(통특기와 같은 흐름): 최종본 `.fin` 의 명세서 XML 청구항(보정 전)과 보정한 HLT(보정 후)를 대조해
/// `.dta` 를 만들고 그 `.fin` 에 더한다:
/// - documentID = 그 `.fin` 의 `APPTIME` (2/2) · 발명의 명칭 = 명세서 `invention-title` 의 「국문{영문}」.
/// - docType = 명세서 뿌리 요소 · dtaVersion = 보정서 순번 + 1(`options.dta_version` 이 비면).
/// - 새 `.fin` = 기존 항목(바이트 그대로) + 새 `.dta`(무압축 저장) + `[AMENDMENT]` 를 고친 `xresult.inf`.
pub fn export_amendment_from_fin(
    final_fin: &Path,
    after: &Path,
    dta_name: &str,
    date: &str,
    kipo: &KipoExportOptions,
    options: &AmendmentOptions,
) -> Result<FinAmendmentExport, CoreError> {
    let file = std::fs::File::open(final_fin).map_err(|_| fin_error("fin_open"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| fin_error("fin_zip"))?;
    let FinalFin {
        names,
        xresult,
        mut amendments,
        before_xml,
        before,
        stored,
        outside,
    } = read_final_fin(&mut archive, Some(final_fin))?;
    if names.iter().any(|name| name == dta_name) {
        return Err(fin_error("dta_exists"));
    }
    let field = |key: &str| {
        xresult
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .map(str::to_string)
            .ok_or_else(|| fin_error("xresult_field"))
    };
    let app_time = field("APPTIME=")?;
    let appname = field("APPNAME=")?;
    if !amendable_elements(&before_xml).keys().any(|key| key.0 == 2) {
        return Err(fin_error("before_claims"));
    }
    // 그림을 픽셀째 읽으니 그림마다 jpg/tif 가 갈린다(`export_kipo::image_format` — 풀리면 jpg · 통특기 보정서의 jpg 와 같다).
    let (after_export, payloads) = export_after(after, kipo)?;
    let after = amendable_elements(&after_export.xml);
    // 문단 수가 바뀌어도 통특기는 받는다(문단 삭제 → `p D 0009` 하나 · 뒤 번호 그대로). 번호는 HLT 식별번호가
    // 들고 있다. 식별번호가 없는 문서(최종본 불러오기로 열지 않은 문서)는 차례로 세니 지운 자리부터 뒤가 밀린다 — 그때만 거절한다.
    let live_before = before
        .iter()
        .filter(|(key, value)| key.0 == 1 && value.is_some())
        .count();
    let after_count = after.keys().filter(|key| key.0 == 1).count();
    if live_before != after_count && !after_export.numbered_by_id {
        let mut error = fin_error("paragraph_numbers_missing");
        error
            .details
            .insert("before".into(), live_before.to_string());
        error
            .details
            .insert("after".into(), after_count.to_string());
        return Err(error);
    }
    // 그림만 바뀐 요소도 바뀐 것이다(수식 그림 교체 → `p A 0150` · 글자·태그 차례로는 같다).
    // 크기(he · wi mm)만 바꾼 그림도 — 통특기는 도면 크기만 바꾼 것을 `figure A` 로 냈다. 손대지 않은 그림도
    // 우리 값이 통특기 값과 반올림으로 1 어긋난다(실물 78건 중 29건 · 1 넘게는 0건) → 1 넘게 달라야 바뀐 것.
    let same_images = |old: &str, new: &str| {
        let (old_sizes, new_sizes) = (image_sizes(old), image_sizes(new));
        let (old, new) = (image_files(old), image_files(new));
        old.len() == new.len()
            && old_sizes
                .iter()
                .zip(&new_sizes)
                .all(|(old, new)| old.0.abs_diff(new.0) <= 1 && old.1.abs_diff(new.1) <= 1)
            && old.iter().zip(&new).all(|(old, new)| {
                matches!(
                    (stored.get(*old), payloads.get(*new)),
                    (Some(stored), Some(payload)) if same_image(stored, payload)
                )
            })
    };
    let mut entries = diff_elements_by(&before, &after, |old, new| {
        comparable(old) == comparable(new) && same_images(old, new)
    });
    if entries.is_empty() {
        return Err(fin_error("amendment_empty"));
    }
    // 보정서의 그림 이름: 최종본에 있던 그림은 그 이름 · 새 그림은 최종본 그림 다음 번호
    // (예: hlz pat00001~46 뒤 → 수식 교체도 1비트 추가도 pat00047 · `id` 는 우리 XML 그대로 문서 차례).
    let mut next = stored
        .keys()
        .filter_map(|name| {
            name.split('.')
                .next()?
                .trim_start_matches(|c: char| !c.is_ascii_digit())
                .parse::<u32>()
                .ok()
        })
        .max()
        .unwrap_or(0);
    let mut source: BTreeMap<String, String> = BTreeMap::new();
    for entry in &mut entries {
        let old = before
            .iter()
            .find(|(key, _)| {
                AMENDABLE[key.0] == entry.element_name && key.2 == entry.attribute_value
            })
            .and_then(|(_, value)| value.as_deref())
            .map(image_files)
            .unwrap_or_default();
        let mut used = vec![false; old.len()];
        let mut body = String::new();
        let mut rest = entry.body.as_str();
        while let Some(at) = rest.find(" file=\"") {
            let start = at + " file=\"".len();
            let end = start
                + rest[start..]
                    .find('"')
                    .ok_or_else(|| fin_error("img_file"))?;
            let ours = &rest[start..end];
            let payload = payloads
                .get(ours)
                .ok_or_else(|| fin_error("amendment_image_missing"))?;
            let kept = old.iter().enumerate().find(|(index, name)| {
                !used[*index]
                    && stored
                        .get(**name)
                        .is_some_and(|bytes| same_image(bytes, payload))
            });
            let name = match kept {
                Some((index, name)) => {
                    used[index] = true;
                    name.to_string()
                }
                None => {
                    next += 1;
                    let (stem, extension) =
                        ours.rsplit_once('.').ok_or_else(|| fin_error("img_file"))?;
                    let stem = stem.trim_end_matches(|c: char| c.is_ascii_digit());
                    format!("{stem}{next:05}.{extension}")
                }
            };
            body.push_str(&rest[..start]);
            body.push_str(&name);
            source.insert(name, ours.to_string());
            rest = &rest[end..];
        }
        body.push_str(rest);
        entry.body = body;
    }
    let amended = entries.iter().filter(|entry| entry.status == "A").count();
    let deleted = entries.len() - amended;

    let doc_type = before_xml
        .split("<KIPO")
        .nth(1)
        .and_then(|rest| rest.split_once('>'))
        .and_then(|(_, rest)| rest.trim_start().strip_prefix('<'))
        .and_then(|rest| rest.split([' ', '>']).next())
        .ok_or_else(|| fin_error("before_root"))?
        .to_string();
    let (ko, en) = invention_titles(&before);
    let mut options = options.clone();
    options.doc_type = doc_type;
    options.document_id = app_time;
    if options.invention_ko_title.is_empty() {
        options.invention_ko_title = ko;
    }
    if options.invention_en_title.is_empty() {
        options.invention_en_title = en;
    }
    if options.dta_version.is_empty() {
        options.dta_version = dta_version(amendments.len());
    }
    let xml = build_amendment_xml(&entries, &options);
    let images = amendment_images(&entries, &payloads, &source, &options.image_time)?;
    let dta = build_amendment_dta_with_images(dta_name, &xml, &images)?;

    // 새 xresult: [APPLICATION] 그대로 + [AMENDMENT] 다시 셈.
    amendments.push(format!("{dta_name},{date},1"));
    let mut text = xresult
        .split("[AMENDMENT]")
        .next()
        .unwrap_or_default()
        .to_string();
    if !text.contains("APPNAME=") || !appname.contains(".hlz") {
        return Err(fin_error("xresult_application"));
    }
    text.push_str(&format!("[AMENDMENT]\r\nAMDCNT={}\r\n", amendments.len()));
    for (index, value) in amendments.iter().enumerate() {
        text.push_str(&format!("AMD{:03}={value}\r\n", index + 1));
    }
    let (xresult_bytes, _, had_errors) = EUC_KR.encode(&text);
    if had_errors {
        return Err(fin_error("xresult_encoding"));
    }

    let mut buffer = Vec::new();
    let mut entry_names = EntryNames::new();
    {
        let mut zip = ZipWriter::new(Cursor::new(&mut buffer));
        for index in 0..archive.len() {
            let entry = archive
                .by_index_raw(index)
                .map_err(|_| fin_error("fin_zip"))?;
            if entry.name() == "xresult.inf" {
                continue;
            }
            let name = entry_names.raw(&entry.name_raw().to_vec())?;
            zip.raw_copy_file_rename(entry, name)
                .map_err(|_| fin_error("zip_copy"))?;
        }
        // 옆에서 읽은 보정서도 싣는다(새 최종본은 옆 파일 없이 열린다).
        for (name, bytes) in &outside {
            zip.start_file(
                entry_names.name(name)?,
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .map_err(|_| fin_error("zip_start"))?;
            zip.write_all(bytes).map_err(|_| fin_error("zip_write"))?;
        }
        zip.start_file(
            entry_names.name(dta_name)?,
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )
        .map_err(|_| fin_error("zip_start"))?;
        zip.write_all(&dta).map_err(|_| fin_error("zip_write"))?;
        zip.start_file(
            "xresult.inf",
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .map_err(|_| fin_error("zip_start"))?;
        zip.write_all(&xresult_bytes)
            .map_err(|_| fin_error("zip_write"))?;
        zip.finish().map_err(|_| fin_error("zip_finish"))?;
    }
    entry_names.patch(&mut buffer)?;
    Ok(FinAmendmentExport {
        fin: buffer,
        xml,
        dta,
        amended,
        deleted,
    })
}

#[cfg(test)]
mod tests {
    use super::amendment_tables;

    #[test]
    fn tables_take_the_amendment_serialization() {
        // 제출 XML 꼴 → 보정서 꼴. 두 번 걸어도 같다(쌓인 보정서 본문은 이미 보정서 꼴).
        let loose = "<p num=\"0001\"><tables num=\"1\">\n<table>\n<title>가</title>\n<tgroup xmlns='http://www.oasis-open.org/tables/exchange/1.0' cols=\"1\">\n<colspec colnum=\"1\" colname=\"col1\"/>\n<tbody>\n<row>\n<entry colname=\"col1\">나</entry>\n</row>\n</tbody>\n</tgroup>\n</table>\n</tables>\n</p>";
        let compact = "<p num=\"0001\"><tables num=\"1\"><table><title>가</title><tgroup xmlns=\"http://www.oasis-open.org/tables/exchange/1.0\" cols=\"1\">\n<colspec colnum=\"1\" colname=\"col1\"/>\n<tbody><row>\n<entry colname=\"col1\">나</entry>\n</row></tbody></tgroup></table></tables></p>";
        assert_eq!(amendment_tables(loose), compact);
        assert_eq!(amendment_tables(compact), compact);
        assert_eq!(amendment_tables("<p>표 없음\n</p>"), "<p>표 없음\n</p>");
    }
}

#[cfg(test)]
mod compose_tests {
    #[test]
    fn composes_mac_decomposed_hangul() {
        // 「제출_보정서.hlz」 NFD(맥 저장 대화상자) → NFC.
        let nfd = "\u{110c}\u{1166}\u{110e}\u{116e}\u{11af}_\u{1107}\u{1169}\u{110c}\u{1165}\u{11bc}\u{1109}\u{1165}.hlz";
        assert_eq!(super::compose_hangul(nfd), "제출_보정서.hlz");
        assert!(!super::cp949(nfd).1);
        assert_eq!(super::compose_hangul("이미 NFC · abc"), "이미 NFC · abc");
    }
}
