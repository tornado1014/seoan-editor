//! 하네스용 명령: `new-opinion` → `export-fin` → `open-package` 왕복 · `export-docx`.

mod support;

use std::{path::Path, process::Command};

fn run(args: &[&str]) -> (Option<i32>, serde_json::Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_hlt-cli"))
        .args(args)
        .current_dir(support::repo_root())
        .output()
        .unwrap();
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    (
        output.status.code(),
        serde_json::from_str(stdout.trim()).unwrap(),
    )
}

fn texts(hlt: &Path) -> Vec<String> {
    let (code, value) = run(&["inspect", hlt.to_str().unwrap(), "--kind", "paragraphs"]);
    assert_eq!(code, Some(0), "{value}");
    value["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["text"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn opinion_from_text_round_trips_through_a_final_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = |name: &str| dir.path().join(name).to_str().unwrap().to_string();
    std::fs::write(path("body.txt"), "첫째 의견.\n\n둘째 의견.\n").unwrap();

    let (code, value) = run(&["new-opinion", &path("o.HLT"), "--body", &path("body.txt")]);
    assert_eq!(code, Some(0), "{value}");
    let written = texts(Path::new(&path("o.HLT")));
    assert_eq!(
        written[..4],
        ["【의견내용】", "첫째 의견.", "", "둘째 의견."]
    );

    let (code, value) = run(&[
        "export-fin",
        &path("o.HLT"),
        &path("o.fin"),
        "--hlz-name",
        "o.hlz",
        "--app-date",
        "2026-10-19",
        "--app-time",
        "20261019120000",
    ]);
    assert_eq!(code, Some(0), "{value}");
    let (code, value) = run(&["open-package", &path("o.fin"), &path("back.HLT")]);
    assert_eq!(code, Some(0), "{value}");
    assert_eq!(texts(Path::new(&path("back.HLT"))), written);

    // 있는 파일은 덮지 않는다.
    let (code, value) = run(&["new-opinion", &path("o.HLT")]);
    assert_eq!(code, Some(2));
    assert_eq!(value["error"]["code"], "output_exists");
}

#[test]
fn export_docx_reads_back_and_never_overwrites() {
    let fixture = support::pct_table_fixture();
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("out.docx");
    let args = [
        "export-docx",
        fixture.path().to_str().unwrap(),
        output.to_str().unwrap(),
    ];
    let (code, value) = run(&args);
    assert_eq!(code, Some(0), "{value}");

    let (code, value) = run(&["inspect-docx", output.to_str().unwrap()]);
    assert_eq!(code, Some(0), "{value}");
    let summary = &value["result"]["page"]["summary"];
    assert_ne!(summary["content_paragraph_count"], "0", "{summary}");
    assert_ne!(summary["table_count"], "0", "표가 남는다 {summary}");

    let written = std::fs::read(&output).unwrap();
    let (code, value) = run(&args);
    assert_eq!(code, Some(2));
    assert_eq!(value["error"]["code"], "output_exists");
    assert_eq!(std::fs::read(&output).unwrap(), written);
}

#[test]
fn harness_commands_reject_bad_arguments() {
    for args in [
        &["export-docx", "a.HLT"][..],
        &["open-package"][..],
        &["new-opinion", "o.HLT", "--body"][..],
    ] {
        let (code, value) = run(args);
        assert_eq!(code, Some(64), "{args:?}");
        assert_eq!(value["ok"], false);
    }
}

/// 덮어쓰기: 기본은 있는 출력을 거부(바이트 그대로) · `--overwrite` 면 덮는다(export-fin · export-amendment).
#[test]
fn export_commands_overwrite_only_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let path = |name: &str| dir.path().join(name).to_str().unwrap().to_string();
    std::fs::write(path("body.txt"), "의견.\n").unwrap();
    let (code, value) = run(&["new-opinion", &path("o.HLT"), "--body", &path("body.txt")]);
    assert_eq!(code, Some(0), "{value}");
    let export = |date: &str, overwrite: bool| {
        let o = path("o.HLT");
        let fin = path("o.fin");
        let mut args = vec![
            "export-fin",
            &o,
            &fin,
            "--hlz-name",
            "o.hlz",
            "--app-date",
            date,
            "--app-time",
            "20261019120000",
        ];
        if overwrite {
            args.push("--overwrite");
        }
        run(&args)
    };
    assert_eq!(export("2026-10-19", false).0, Some(0));
    let first = std::fs::read(path("o.fin")).unwrap();

    let (code, value) = export("2026-10-20", false);
    assert_eq!(code, Some(2));
    assert_eq!(value["error"]["code"], "output_exists");
    assert_eq!(
        std::fs::read(path("o.fin")).unwrap(),
        first,
        "거부하면 그대로"
    );

    let (code, value) = export("2026-10-20", true);
    assert_eq!(code, Some(0), "{value}");
    assert_ne!(std::fs::read(path("o.fin")).unwrap(), first, "덮었다");
    assert!(
        !dir.path().join("o.fin.tmp").exists(),
        "임시 파일이 남지 않는다"
    );

    // export-amendment 도 같은 문을 쓴다: 있는 출력 — 기본은 output_exists · --overwrite 면 그 검사를 지난다.
    std::fs::write(path("a.dta"), b"old").unwrap();
    let o = path("o.HLT");
    let dta = path("a.dta");
    let (_, value) = run(&["export-amendment", &o, &o, &dta]);
    assert_eq!(value["error"]["code"], "output_exists");
    let (_, value) = run(&["export-amendment", &o, &o, &dta, "--overwrite"]);
    assert_ne!(value["error"]["code"], "output_exists", "{value}");
}
