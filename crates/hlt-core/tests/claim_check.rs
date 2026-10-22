//! 청구항 오류점검 — 특허로 점검 페이지의 판정 JS 를 맥에서(jsdom).
//! 실행: `NK_CLAIM_CHECKER=<점검기 폴더> cargo test -p hlt-core --test claim_check -- --ignored`

use std::path::PathBuf;

use hlt_core::{ErrorCode, check_claims};

fn dir(var: &str) -> PathBuf {
    PathBuf::from(std::env::var(var).unwrap_or_else(|_| panic!("{var} 가 필요하다")))
}

#[test]
#[ignore = "점검기 폴더(NK_CLAIM_CHECKER — KIPO JS · jsdom)가 필요하다"]
fn synthetic_cases_match_the_browser_engine() {
    let checker = dir("NK_CLAIM_CHECKER");
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/claim_check_cases.json")).unwrap();
    let cases = fixture["cases"].as_object().unwrap();
    for (name, case) in cases {
        let claims: Vec<(String, String)> = case["claims"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, text)| ((i + 1).to_string(), text.as_str().unwrap().to_string()))
            .collect();
        let report = check_claims(&claims, &checker).unwrap();
        let ours = serde_json::to_value(&report.claims).unwrap();
        assert_eq!(ours, case["expect"], "{name}");
    }
    println!("합성 {} 건 = 브라우저 판정", cases.len());
}

#[test]
#[ignore = "점검기 폴더(NK_CLAIM_CHECKER)가 필요하다"]
fn refuses_when_the_page_script_breaks() {
    // 판정 JS 가 넘어지면(페이지에선 화면이 비거나 반쪽) 판정을 믿지 않는다 — 빈 결과를 「오류 없음」으로 읽지 마라.
    let checker = dir("NK_CLAIM_CHECKER");
    let broken = std::env::temp_dir().join(format!("claim-checker-broken-{}", std::process::id()));
    std::fs::create_dir_all(broken.join("web")).unwrap();
    for name in ["page.html", "kipo-common.js", "claim_common.js"] {
        std::fs::copy(
            checker.join("web").join(name),
            broken.join("web").join(name),
        )
        .unwrap();
    }
    std::fs::write(
        broken.join("web/claim_check_kr.js"),
        "throw new Error('broken');",
    )
    .unwrap();
    std::os::unix::fs::symlink(checker.join("node_modules"), broken.join("node_modules")).unwrap();
    let error = check_claims(&[("1".into(), "가를 포함하는 장치.".into())], &broken).unwrap_err();
    std::fs::remove_dir_all(&broken).unwrap();
    assert_eq!(error.details["reason"], "runner_inconsistent");
    assert!(
        error.details["runner"].contains("broken"),
        "{:?}",
        error.details
    );
}

#[test]
fn refuses_without_claims_or_checker() {
    let missing = PathBuf::from("/nonexistent-claim-checker");
    let error = check_claims(&[], &missing).unwrap_err();
    assert_eq!(error.code, ErrorCode::UnsupportedFormat);
    let error = check_claims(&[("1".into(), "가.".into())], &missing).unwrap_err();
    assert_eq!(error.code, ErrorCode::IoError);
    // 번호가 겹치면 점검기를 찾기 전에 거절한다.
    let error = check_claims(
        &[("1".into(), "가.".into()), ("1".into(), "나.".into())],
        &missing,
    )
    .unwrap_err();
    assert_eq!(error.details["reason"], "claim_numbering");
}

/// 앱 안 인용 검사(`claim_citation_issues` · node 없이)가 점검 페이지와 같은 판정을 내는가 — 실물 최종본 청구항 전부.
/// 실행: `NK_CLAIM_CHECKER=… NK_AMEND_CORPUS=<최종본 .fin 폴더> cargo test -p hlt-core --test claim_check -- --ignored corpus`
#[test]
#[ignore = "점검기 폴더(NK_CLAIM_CHECKER) · 실물 최종본 폴더(NK_AMEND_CORPUS)가 필요하다"]
fn citation_rules_match_the_page_on_the_corpus() {
    use hlt_core::{
        CitationKind, claim_citation_issues, claim_list_from_xml, final_version_from_fin,
    };
    let checker = dir("NK_CLAIM_CHECKER");
    let corpus = dir("NK_AMEND_CORPUS");
    let page_kind = |kind: CitationKind| match kind {
        CitationKind::MisReference => Some("청구항 오참조"),
        CitationKind::DuplicateMultiple => Some("중복 다중참조"),
        CitationKind::NoAlternative => Some("택일어 미사용"),
        _ => None, // 앱만 보는 규칙(지운 항 · 범위 나머지 · 「및」 · 범위 택일어)
    };
    let (mut files, mut claims_seen, mut flagged) = (0, 0, 0);
    let mut mismatches = Vec::new();
    let mut by_kind = std::collections::BTreeMap::new();
    let mut app_only = std::collections::BTreeMap::new();
    let mut entries: Vec<_> = std::fs::read_dir(&corpus)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    for path in entries
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "fin"))
    {
        let Ok((xml, _)) = final_version_from_fin(path) else {
            continue;
        };
        let real = claim_list_from_xml(&xml);
        files += 1;
        // 실물 차례 그대로 + 글 차례를 뒤집은 목록(인용이 뒤 항을 가리켜 오참조가 실물 문장으로 걸린다) +
        // 끝에 다중 인용 항 둘(둘째가 첫째를 다시 다중 인용 — 실물 코퍼스엔 다중의 다중이 없다 · 본문은 마지막 실물 항).
        let texts: Vec<String> = real.iter().rev().map(|(_, text)| text.clone()).collect();
        let reversed: Vec<(String, String)> =
            real.iter().map(|(no, _)| no.clone()).zip(texts).collect();
        let mut multiple = real.clone();
        if let Some(last) = real.last().filter(|_| real.len() >= 2) {
            let next: u32 = last.0.parse().unwrap_or(0) + 1;
            multiple.push((
                next.to_string(),
                format!("제1항 또는 제2항에 있어서, {}", last.1),
            ));
            multiple.push((
                (next + 1).to_string(),
                format!("제1항 또는 제{next}항에 있어서, {}", last.1),
            ));
        }
        for (variant, claims) in [real, reversed, multiple].into_iter().enumerate() {
            let Ok(report) = check_claims(&claims, &checker) else {
                continue;
            };
            let numbered: Vec<(u32, String)> = claims
                .iter()
                .filter_map(|(no, text)| Some((no.parse().ok()?, text.clone())))
                .collect();
            let ours = claim_citation_issues(&numbered);
            for issue in ours
                .iter()
                .filter(|i| variant == 0 && page_kind(i.kind).is_none())
            {
                *app_only
                    .entry(format!("{:?}", issue.kind))
                    .or_insert(0usize) += 1;
                if std::env::var("NK_SHOW_APP_ONLY").is_ok() {
                    let text = &numbered.iter().find(|(n, _)| *n == issue.claim).unwrap().1;
                    println!(
                        "앱만 {:?} {} 청구항 {} → {:?}: {}",
                        issue.kind,
                        path.file_name().unwrap().to_string_lossy(),
                        issue.claim,
                        issue.cited,
                        text.chars().take(70).collect::<String>()
                    );
                }
            }
            for verdict in report
                .claims
                .iter()
                .filter(|v| !report.filled.contains(&v.no))
            {
                claims_seen += 1;
                let mut page: Vec<&str> = verdict
                    .patterns
                    .iter()
                    .map(String::as_str)
                    .filter(|p| ["청구항 오참조", "중복 다중참조", "택일어 미사용"].contains(p))
                    .collect();
                let mut mine: Vec<&str> = ours
                    .iter()
                    .filter(|i| i.claim.to_string() == verdict.no)
                    .filter_map(|i| page_kind(i.kind))
                    .collect();
                page.sort();
                mine.sort();
                flagged += usize::from(!page.is_empty());
                for kind in &page {
                    *by_kind.entry(kind.to_string()).or_insert(0usize) += 1;
                }
                if page != mine {
                    mismatches.push(format!(
                        "{} 청구항 {}: 페이지 {page:?} · 앱 {mine:?}",
                        path.file_name().unwrap().to_string_lossy(),
                        verdict.no
                    ));
                }
            }
        }
    }
    println!(
        "최종본 {files} 건 · 청구항 {claims_seen} · 페이지가 건 항 {flagged} {by_kind:?} · 앱만 보는 규칙(실물 차례) {app_only:?}"
    );
    for line in mismatches.iter().take(40) {
        println!("{line}");
    }
    assert!(files > 0 && claims_seen > 0, "대조한 것이 없다");
    // 세 규칙이 모두 실물 문장에서 걸려야 이 대조가 그 규칙을 잰 것이다.
    for kind in ["청구항 오참조", "중복 다중참조", "택일어 미사용"] {
        assert!(
            by_kind.get(kind).is_some_and(|n| *n > 0),
            "{kind} 가 한 번도 안 걸렸다 — 대조가 그 규칙을 못 쟀다"
        );
    }
    assert!(mismatches.is_empty(), "불일치 {}", mismatches.len());
}
