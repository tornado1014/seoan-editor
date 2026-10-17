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
