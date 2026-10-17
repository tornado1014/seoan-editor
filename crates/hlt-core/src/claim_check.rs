//! 청구항 오류점검. 통특기 v6.3 은 로컬 `ChkClaim.exe`(2013 · 입력에 머리만 덧씌우고 끝난다)를 부르지 않고
//! 청구항 XML 을 `C:\KIPONET\APPDOM\KORCLAIM\<시각>.xml` 에 쓴 뒤 특허로 `PetitionClaimCheck.do` 를 브라우저로 연다.
//! 그 페이지는 서버가 XML 을 (번호, 글자 내용) 목록으로 바꾸고 **판정은 전부 페이지 JS(`claim_check_kr.js`)** 가 한다.
//!
//! 그래서 판정기 = KIPO JS 그대로(jsdom · `assets/claim_check_runner.js`). P4-1 의 XSD 와 같은 방식 — KIPO 자산(JS·페이지)과
//! jsdom 은 저장소 밖 점검기 폴더(`checker_dir`)에 두고 경로로 받는다. 문서는 네트워크로 나가지 않는다.
//! 통특기 KORCLAIM XML 의 청구항 글자 = 우리 제출 XML 의 글자 · jsdom 판정 = 실제 Chromium 판정.

use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

use serde::{Deserialize, Serialize};

use crate::{CoreError, ErrorCode};

const RUNNER: &str = include_str!("../assets/claim_check_runner.js");

/// 특허로 점검 페이지의 오류 유형(`chkPattern` 순서 그대로).
pub const CLAIM_ERROR_KINDS: [&str; 6] = [
    "청구항 오참조",
    "중복 다중참조",
    "불명확한 표현",
    "종결어 불일치",
    "택일어 미사용",
    "상기 인용 없음",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimVerdict {
    pub no: String,
    /// 걸린 오류 유형(`CLAIM_ERROR_KINDS` 의 글).
    pub patterns: Vec<String>,
    /// 이 항이 인용하는 항 번호.
    pub parents: Vec<String>,
    /// 「상기」 다음 낱말 → 페이지 안내문(조사 제거·띄어쓰기 경고). 비면 안내 없음.
    pub above: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClaimCheckReport {
    pub claims: Vec<ClaimVerdict>,
    /// 빈 번호에 채워 넣은 「삭제」 항(보정 명세서는 삭제한 항이 HLT 에서 빠진다).
    pub filled: Vec<String>,
}

impl ClaimCheckReport {
    pub fn error_count(&self) -> usize {
        self.claims.iter().map(|claim| claim.patterns.len()).sum()
    }
}

#[derive(Deserialize)]
struct RunnerOutput {
    error: Option<String>,
    alerts: Vec<String>,
    logs: Vec<String>,
    claims: Vec<ClaimVerdict>,
}

/// KIPO XML 의 청구항 → 특허로 서버가 만드는 목록: (번호, `claim-text` 의 글자 내용). 태그는 지우고(`<br/>` 는 구분자 없이)
/// 엔터티는 풀고 공백은 그대로 둔다.
pub fn claim_list_from_xml(xml: &str) -> Vec<(String, String)> {
    let mut claims = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find("<claim num=\"") {
        rest = &rest[at + "<claim num=\"".len()..];
        let Some(quote) = rest.find('"') else { break };
        let number = rest[..quote].to_string();
        let end = rest.find("</claim>").unwrap_or(rest.len());
        let body = &rest[..end];
        let text = body
            .find("<claim-text>")
            .map(|start| {
                let inner = &body[start + "<claim-text>".len()..];
                text_content(&inner[..inner.rfind("</claim-text>").unwrap_or(inner.len())])
            })
            .unwrap_or_default();
        claims.push((number, text));
        rest = &rest[end..];
    }
    claims
}

fn text_content(markup: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for ch in markup.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// 청구항 목록을 특허로 점검 JS 로 판정한다. `checker_dir` = web/(page.html · kipo-common.js · claim_common.js ·
/// claim_check_kr.js) + node_modules/jsdom.
pub fn check_claims(
    claims: &[(String, String)],
    checker_dir: &Path,
) -> Result<ClaimCheckReport, CoreError> {
    if claims.is_empty() {
        return Err(check_error(ErrorCode::UnsupportedFormat, "no_claims"));
    }
    let (claims, filled) = fill_deleted(claims)?;
    if !checker_dir.join("web/claim_check_kr.js").is_file()
        || !checker_dir.join("node_modules/jsdom").is_dir()
    {
        return Err(check_error(ErrorCode::IoError, "checker_missing"));
    }
    let input: Vec<serde_json::Value> = claims
        .iter()
        .map(|(no, text)| serde_json::json!({ "claimNum": no, "claimText": text }))
        .collect();
    let mut child = Command::new("node")
        .arg("-e")
        .arg(RUNNER)
        .current_dir(checker_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| check_error(ErrorCode::IoError, "node_missing"))?;
    child
        .stdin
        .take()
        .ok_or_else(|| check_error(ErrorCode::IoError, "node_stdin"))?
        .write_all(serde_json::Value::Array(input).to_string().as_bytes())
        .map_err(|_| check_error(ErrorCode::IoError, "node_stdin"))?;
    let output = child
        .wait_with_output()
        .map_err(|_| check_error(ErrorCode::IoError, "node_wait"))?;
    let run: RunnerOutput = serde_json::from_slice(&output.stdout)
        .map_err(|_| check_error(ErrorCode::VerificationFailed, "runner_output"))?;
    // 페이지가 경고창을 띄우거나 스크립트가 넘어지면 판정이 반쪽이다 — 결과를 믿지 않는다.
    let broken = run.error.is_some()
        || !run.alerts.is_empty()
        || !run.logs.is_empty()
        || run.claims.len() != claims.len()
        || run
            .claims
            .iter()
            .zip(&claims)
            .any(|(verdict, (no, _))| verdict.no != *no)
        || run
            .claims
            .iter()
            .flat_map(|claim| &claim.patterns)
            .any(|kind| !CLAIM_ERROR_KINDS.contains(&kind.as_str()));
    if broken {
        let mut error = check_error(ErrorCode::VerificationFailed, "runner_inconsistent");
        let detail = run
            .error
            .into_iter()
            .chain(run.alerts)
            .chain(run.logs)
            .collect::<Vec<_>>()
            .join("\n");
        error.details.insert("runner".into(), detail);
        return Err(error);
    }
    Ok(ClaimCheckReport {
        claims: run.claims,
        filled,
    })
}

/// 페이지 JS 는 「목록 위치 = 청구항 번호」를 전제한다(빈 번호가 있으면 뒤 항의 인용이 한 칸씩 밀린다).
/// 보정 명세서는 삭제한 항이 통째로 빠진다(after: 7·17) — 빈 번호에 보정서 꼴 「삭제」 항을 채운다.
/// 번호가 겹치거나 줄면 거절한다(예: 47·48 이 두 번 — 페이지도 스택이 넘친다 · 통특기도 변환하지 않는다).
fn fill_deleted(
    claims: &[(String, String)],
) -> Result<(Vec<(String, String)>, Vec<String>), CoreError> {
    let (mut out, mut filled) = (Vec::new(), Vec::new());
    for (no, text) in claims {
        let number: usize = no.parse().unwrap_or(0);
        if number <= out.len() {
            let mut error = check_error(ErrorCode::StructureInvalid, "claim_numbering");
            error.details.insert("number".into(), no.clone());
            return Err(error);
        }
        while out.len() + 1 < number {
            let gap = (out.len() + 1).to_string();
            out.push((gap.clone(), DELETED.to_string()));
            filled.push(gap);
        }
        out.push((no.clone(), text.clone()));
    }
    Ok((out, filled))
}

const DELETED: &str = "삭제";

fn check_error(code: ErrorCode, reason: &str) -> CoreError {
    let mut error = CoreError::new(code, "claim_check", false);
    error.details.insert("reason".into(), reason.into());
    error
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_deleted_gaps_and_refuses_repeats() {
        let claims = |numbers: &[&str]| -> Vec<(String, String)> {
            numbers
                .iter()
                .map(|no| (no.to_string(), "가.".to_string()))
                .collect()
        };
        let (filled_claims, filled) = fill_deleted(&claims(&["1", "2", "4", "7"])).unwrap();
        assert_eq!(filled, ["3", "5", "6"]);
        let numbers: Vec<&str> = filled_claims.iter().map(|(no, _)| no.as_str()).collect();
        assert_eq!(numbers, ["1", "2", "3", "4", "5", "6", "7"]);
        assert_eq!(filled_claims[2].1, "삭제");
        for bad in [&["1", "2", "2"][..], &["1", "3", "2"], &["0"], &["x"]] {
            let error = fill_deleted(&claims(bad)).unwrap_err();
            assert_eq!(error.details["reason"], "claim_numbering", "{bad:?}");
        }
    }

    #[test]
    fn claim_list_keeps_text_content_like_the_kipo_server() {
        // 특허로 서버: 태그 제거 · `<br/>` 는 구분자 없이 · 엔터티 풀기 · 공백 보존.
        let xml = "<claims><claim num=\"1\"><claim-text>가를 포함하고, <br/>나를 포함하는 장치.</claim-text></claim>\
                   <claim num=\"2\"><claim-text><b>굵은</b> H<sub>2</sub>O 및 A &amp; B &lt; C 인  장치. </claim-text></claim></claims>";
        assert_eq!(
            claim_list_from_xml(xml),
            vec![
                ("1".into(), "가를 포함하고, 나를 포함하는 장치.".into()),
                ("2".into(), "굵은 H2O 및 A & B < C 인  장치. ".into()),
            ]
        );
    }
}
