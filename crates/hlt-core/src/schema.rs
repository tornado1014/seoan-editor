//! KIPO XML 스키마 검증 — 통특기가 제출 파일을 만들기 전에 거는 문(「XML 파일 검증에 실패했습니다」).
//!
//! 엔진 = libxml2 `xmllint`(macOS 기본). 스키마 = KIPO 공식 `Epasl/INCLUDE/Schema/kipo.xsd`(KIPO 자산이라 저장소 밖 — 경로로 받는다).
//! 통특기가 받는 XML 은 통과 · 통특기가 거절하는 XML(빈 `<OpinionDOC>`)은 거절한다.
//! 보정서 XML(dta)은 이름공간이 없어 이 스키마의 대상이 아니다(통특기 dta 도 안 지난다 — 통특기는 dta 를 스키마로 거르지 않는다).

use std::{
    io::{Cursor, Read, Write},
    path::Path,
    process::{Command, Stdio},
};

use crate::{CoreError, ErrorCode};

/// 오류 줄을 이만큼까지만 싣는다.
const MAX_ERRORS: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaReport {
    pub valid: bool,
    /// xmllint 의 오류 줄(앞의 `-:줄:` 은 `줄 N:` 으로).
    pub errors: Vec<String>,
}

/// `xml` 을 `schema_dir/kipo.xsd` 로 검증한다. KIPO 이름공간이 아니면 `UnsupportedFormat`(보정서 dta 등).
pub fn validate_kipo_xml(xml: &[u8], schema_dir: &Path) -> Result<SchemaReport, CoreError> {
    let head = String::from_utf8_lossy(&xml[..xml.len().min(512)]);
    if !head.contains("xmlns=\"http://www.kipo.go.kr\"") {
        return Err(schema_error(
            ErrorCode::UnsupportedFormat,
            "not_kipo_namespace",
        ));
    }
    let schema = schema_dir.join("kipo.xsd");
    if !schema.is_file() {
        return Err(schema_error(ErrorCode::IoError, "schema_missing"));
    }
    let mut child = Command::new("xmllint")
        .arg("--noout")
        .arg("--schema")
        .arg(&schema)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| schema_error(ErrorCode::IoError, "xmllint_missing"))?;
    child
        .stdin
        .take()
        .ok_or_else(|| schema_error(ErrorCode::IoError, "xmllint_stdin"))?
        .write_all(xml)
        .map_err(|_| schema_error(ErrorCode::IoError, "xmllint_stdin"))?;
    let output = child
        .wait_with_output()
        .map_err(|_| schema_error(ErrorCode::IoError, "xmllint_wait"))?;
    let errors: Vec<String> = String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter(|line| line.contains("error"))
        .take(MAX_ERRORS)
        .map(|line| {
            match line
                .strip_prefix("-:")
                .and_then(|rest| rest.split_once(':'))
            {
                Some((number, rest)) => format!("줄 {number}:{rest}"),
                None => line.to_string(),
            }
        })
        .collect();
    // xmllint: 0 = 통과 · 3/4 = 스키마 오류 · 1 = 구문 오류. 통과인데 오류 줄이 있거나 그 반대면 판정을 믿지 않는다.
    let valid = output.status.success();
    if valid != errors.is_empty() {
        return Err(schema_error(
            ErrorCode::VerificationFailed,
            "xmllint_inconsistent",
        ));
    }
    Ok(SchemaReport { valid, errors })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageReport {
    /// (항목 이름, 판정). `.fin` 이면 그 안 `.hlz` 의 XML.
    pub documents: Vec<(String, SchemaReport)>,
    /// 스키마 대상이 아닌 항목(보정서 `.dta` — 이름공간 없음).
    pub skipped: Vec<String>,
}

impl PackageReport {
    pub fn valid(&self) -> bool {
        !self.documents.is_empty() && self.documents.iter().all(|(_, report)| report.valid)
    }
}

/// `.xml` · `.hlz` · `.fin` 을 받아 안의 KIPO XML 을 모두 검증한다(`.fin` → `.hlz` → `.xml`).
pub fn validate_kipo_package(
    name: &str,
    bytes: &[u8],
    schema_dir: &Path,
) -> Result<PackageReport, CoreError> {
    let mut report = PackageReport {
        documents: Vec::new(),
        skipped: Vec::new(),
    };
    collect(name, bytes, schema_dir, &mut report)?;
    if report.documents.is_empty() {
        return Err(schema_error(ErrorCode::UnsupportedFormat, "no_kipo_xml"));
    }
    Ok(report)
}

fn collect(
    name: &str,
    bytes: &[u8],
    schema_dir: &Path,
    report: &mut PackageReport,
) -> Result<(), CoreError> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".xml") {
        report
            .documents
            .push((name.to_string(), validate_kipo_xml(bytes, schema_dir)?));
        return Ok(());
    }
    if lower.ends_with(".dta") {
        report.skipped.push(name.to_string());
        return Ok(());
    }
    if !(lower.ends_with(".hlz") || lower.ends_with(".fin")) {
        return Err(schema_error(ErrorCode::UnsupportedFormat, "package_kind"));
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| schema_error(ErrorCode::StructureInvalid, "package_zip"))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| schema_error(ErrorCode::StructureInvalid, "package_zip"))?;
        // 통특기 zip 의 한글 이름은 CP949 원 바이트다(UTF-8 이 아니면 CP949 로 읽는다).
        let raw = entry.name_raw().to_vec();
        let entry_name = match std::str::from_utf8(&raw) {
            Ok(name) => name.to_string(),
            Err(_) => encoding_rs::EUC_KR.decode(&raw).0.into_owned(),
        };
        let lower = entry_name.to_ascii_lowercase();
        if !(lower.ends_with(".xml") || lower.ends_with(".hlz") || lower.ends_with(".dta")) {
            continue;
        }
        let mut data = Vec::new();
        entry
            .read_to_end(&mut data)
            .map_err(|_| schema_error(ErrorCode::StructureInvalid, "package_read"))?;
        collect(&entry_name, &data, schema_dir, report)?;
    }
    Ok(())
}

fn schema_error(code: ErrorCode, reason: &str) -> CoreError {
    let mut error = CoreError::new(code, "kipo_schema", false);
    error.details.insert("reason".into(), reason.into());
    error
}
