//! 폴더 일괄 처리: HLT 만들기(있으면 건너뜀 · 실패는 까닭) · 제출 전 점검 표 · 멈추기.

use hlt_core::{GenerationDialect, batch_generate_hlt, batch_submission_checks};

#[test]
fn makes_hlts_skips_existing_reports_failures_then_checks_them() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path();
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/generate_pct.hml");
    std::fs::copy(&fixture, folder.join("a.hml")).unwrap();
    std::fs::copy(&fixture, folder.join("c.HML")).unwrap();
    std::fs::write(folder.join("c.HLT"), b"not an hlt").unwrap();
    std::fs::write(folder.join("b.xml"), b"<broken").unwrap();
    std::fs::write(folder.join("notes.txt"), b"skip me").unwrap();

    let mut seen = Vec::new();
    let rows = batch_generate_hlt(folder, GenerationDialect::PctPatent, |at, total, file| {
        seen.push(format!("{}/{total} {file}", at + 1));
        true
    });
    assert_eq!(seen, ["1/3 a.hml", "2/3 b.xml", "3/3 c.HML"]);
    let summary: Vec<(&str, &str)> = rows
        .iter()
        .map(|row| (row.file.as_str(), row.status.as_str()))
        .collect();
    assert_eq!(
        summary,
        [("a.hml", "만듦"), ("b.xml", "실패"), ("c.HML", "있음")]
    );
    assert_eq!(rows[0].detail, "a.HLT");
    assert!(!rows[1].detail.is_empty(), "실패 까닭");
    assert_eq!(
        std::fs::read(folder.join("c.HLT")).unwrap(),
        b"not an hlt",
        "있는 파일은 덮지 않는다"
    );
    assert!(hlt_core::EditorSession::open(&folder.join("a.HLT")).is_ok());

    let checks = batch_submission_checks(folder, |_, _, _| true);
    let files: Vec<&str> = checks.iter().map(|row| row.file.as_str()).collect();
    assert_eq!(files, ["a.HLT", "c.HLT"]);
    assert_eq!(checks[0].error, None);
    assert!(checks[1].error.is_some(), "열 수 없는 파일은 까닭을 적는다");

    let none = batch_generate_hlt(folder, GenerationDialect::PctPatent, |_, _, _| false);
    assert!(none.iter().all(|row| row.status == "멈춤"), "{none:?}");

    // 멈추기: 둘째부터는 「멈춤」.
    let stopped = batch_submission_checks(folder, |at, _, _| at == 0);
    assert_eq!(stopped[0].error, None);
    assert_eq!(stopped[1].error.as_deref(), Some("멈춤"));
}
