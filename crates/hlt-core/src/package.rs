//! 제출파일 열기: `.hlz`(명세서 XML + 그림) · `.fin`(최종본 = 명세서 hlz + 보정서를 차례로 적용) → 새 `.HLT`.
//! 풀어 둔 XML · 그림을 KIPO XML 입력 경로(`inspect_kipo_xml_path`)로 의미 모델로 읽고 생성기로 굽는다.

use std::{collections::BTreeMap, io::Read, path::Path};

use crate::{
    CoreError, ErrorCode, GenerationDialect, KipoXmlDialect, WriteOptions, inspect_kipo_xml_path,
    load_hml_image_payloads_from_directory, save_generated_hlt_as_new,
};

/// `.fin` · `.hlz` 를 `work`(빈 폴더)에 풀고 `output`(없는 파일)에 HLT 로 굽는다.
/// 돌려주는 값 = 가져온 문단 차례의 식별번호(`<p num>` · HLT 에는 실리지 않아 편집 세션이 따로 든다).
pub fn hlt_from_package(
    package: &Path,
    work: &Path,
    output: &Path,
) -> Result<Vec<Option<String>>, CoreError> {
    let extension = package
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let (xml, files) = match extension.as_str() {
        "fin" => crate::export_fin::final_version_from_fin(package)?,
        "hlz" => read_hlz(&std::fs::read(package).map_err(|_| package_error("open"))?)?,
        _ => return Err(package_error("extension")),
    };
    for (name, bytes) in &files {
        // 항목 이름은 파일 이름만 쓴다(zip 안 경로로 폴더 밖에 쓰지 않게).
        let Some(name) = Path::new(name).file_name() else {
            continue;
        };
        std::fs::write(work.join(name), bytes).map_err(|_| package_error("write"))?;
    }
    // 의견서(OpinionDOC)는 명세서 판독기가 모른다(`kipo_xml_dialect`) — 의견서 틀로 따로 굽는다(식별번호 없음).
    if crate::import_opinion::is_opinion_xml(&xml) {
        crate::import_opinion::hlt_from_opinion_xml(&xml, work, output)?;
        return Ok(Vec::new());
    }
    let xml_path = work.join("document.xml");
    std::fs::write(&xml_path, xml).map_err(|_| package_error("write"))?;
    let parsed = inspect_kipo_xml_path(&xml_path)?;
    let payloads = load_hml_image_payloads_from_directory(&parsed.document, work)?;
    let dialect = match parsed.dialect {
        KipoXmlDialect::PctPatentCafdoc => GenerationDialect::PctPatent,
        KipoXmlDialect::PatentCafdoc => GenerationDialect::PatentSpecificationKo,
    };
    save_generated_hlt_as_new(
        &xml_path,
        &parsed.document,
        dialect,
        &payloads,
        output,
        WriteOptions::default(),
    )?;
    Ok(parsed
        .document
        .paragraphs
        .iter()
        .map(|paragraph| paragraph.number.clone())
        .collect())
}

/// `.hlz` = zip(명세서 XML 하나 + 그림들).
fn read_hlz(bytes: &[u8]) -> Result<(String, BTreeMap<String, Vec<u8>>), CoreError> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| package_error("hlz_zip"))?;
    let mut xml = String::new();
    let mut files = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| package_error("hlz_zip"))?;
        let name = entry.name().to_string();
        if name.to_ascii_lowercase().ends_with(".xml") {
            entry
                .read_to_string(&mut xml)
                .map_err(|_| package_error("hlz_read"))?;
        } else {
            let mut raw = Vec::new();
            entry
                .read_to_end(&mut raw)
                .map_err(|_| package_error("hlz_read"))?;
            files.insert(name, raw);
        }
    }
    if xml.is_empty() {
        return Err(package_error("hlz_no_xml"));
    }
    Ok((xml, files))
}

fn package_error(reason: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::StructureInvalid, "open_package", false);
    error.details.insert("reason".into(), reason.into());
    error
}
