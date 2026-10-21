use std::{fs, io::Write, path::PathBuf, process::Command};

#[test]
fn generate_hlt_emits_one_json_line_and_creates_a_verified_output() {
    let input = long_fixture();
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("generated.HLT");
    let result = run(&[
        "generate-hlt",
        input.path().to_str().unwrap(),
        output.to_str().unwrap(),
        "--dialect",
        "patent-specification-ko",
    ]);

    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(result.stderr.is_empty());
    let stdout = String::from_utf8(result.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(value["command"], "generate-hlt");
    assert_eq!(value["ok"], true);
    // 통특기 실물 국내 출원서 44건이 쓰는 값 `0x8000 + 140` (전수).
    assert_eq!(value["result"]["summary"]["document_type_raw"], 32_908);
    assert!(
        value["result"]["summary"]["line_count"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            > 200
    );
    assert_eq!(value["result"]["verification"]["footer_crc_ok"], true);
    assert_eq!(value["result"]["verification"]["window_ok"], true);
    assert!(output.is_file());
}

fn long_fixture() -> tempfile::NamedTempFile {
    let source = fs::read_to_string(fixture()).unwrap();
    let source = source.replace("plain <b>bold</b> text.", &"A".repeat(20_000));
    let mut file = tempfile::Builder::new().suffix(".hml").tempfile().unwrap();
    file.write_all(source.as_bytes()).unwrap();
    file.flush().unwrap();
    file
}

#[test]
fn generate_hlt_rejects_unknown_dialect_and_existing_output() {
    let directory = tempfile::tempdir().unwrap();
    let invalid = directory.path().join("invalid.HLT");
    let usage = run(&[
        "generate-hlt",
        fixture().to_str().unwrap(),
        invalid.to_str().unwrap(),
        "--dialect",
        "guessed",
    ]);
    assert_eq!(usage.status.code(), Some(64));
    assert!(!invalid.exists());
    let usage_json: serde_json::Value = serde_json::from_slice(&usage.stdout).unwrap();
    assert_eq!(usage_json["ok"], false);

    let existing = directory.path().join("existing.HLT");
    fs::write(&existing, b"protected").unwrap();
    let failure = run(&[
        "generate-hlt",
        fixture().to_str().unwrap(),
        existing.to_str().unwrap(),
        "--dialect",
        "patent-specification-ko",
    ]);
    assert_eq!(failure.status.code(), Some(2));
    let failure_json: serde_json::Value = serde_json::from_slice(&failure.stdout).unwrap();
    assert_eq!(failure_json["error"]["code"], "output_exists");
    assert_eq!(fs::read(&existing).unwrap(), b"protected");
}

#[test]
fn generate_hlt_accepts_bare_file_names_relative_to_the_working_directory() {
    // `Path::new("a.hml").parent()` 는 `Some("")` 이라 canonicalize 가 NotFound 로 죽었다
    // (파일명만 넘긴 경우). 입력·출력 둘 다 잡는다.
    let directory = tempfile::tempdir().unwrap();
    fs::copy(fixture(), directory.path().join("in.hml")).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_hlt-cli"))
        .current_dir(directory.path())
        .args([
            "generate-hlt",
            "in.hml",
            "out.HLT",
            "--dialect",
            "patent-specification-ko",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(directory.path().join("out.HLT").is_file());
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_hlt-cli"))
        .args(args)
        .output()
        .unwrap()
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../hlt-core/tests/fixtures/generate_counterpart.hml")
        .canonicalize()
        .unwrap()
}

/// CP949 밖 글자: 대응 없는 글자(⁵)는 그냥 굽기를 거부하고, `--substitutions` 로 고른 대응을 주면 굽는다.
#[test]
fn generate_hlt_takes_chosen_cp949_substitutions() {
    let source = fs::read_to_string(fixture())
        .unwrap()
        .replace("plain <b>bold</b> text.", "10\u{2075} \u{2212} 3");
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.hml");
    fs::write(&input, source).unwrap();
    let output = directory.path().join("out.HLT");
    let args = [
        "generate-hlt",
        input.to_str().unwrap(),
        output.to_str().unwrap(),
        "--dialect",
        "patent-specification-ko",
    ];
    let refused: serde_json::Value = serde_json::from_slice(&run(&args).stdout).unwrap();
    assert_eq!(refused["error"]["code"], "cp949_invalid", "{refused}");
    assert!(
        refused["error"]["details"]["chars"]
            .as_str()
            .unwrap()
            .contains("U+2075")
    );

    let map = directory.path().join("map.json");
    fs::write(&map, r#"{"⁵": "^5"}"#).unwrap();
    let mut chosen = args.to_vec();
    chosen.extend(["--substitutions", map.to_str().unwrap()]);
    let done: serde_json::Value = serde_json::from_slice(&run(&chosen).stdout).unwrap();
    assert_eq!(done["ok"], true, "{done}");
    let pairs: Vec<(String, String)> = done["result"]["cp949_substitutions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["from"].as_str().unwrap().into(),
                item["to"].as_str().unwrap().into(),
            )
        })
        .collect();
    assert_eq!(
        pairs,
        [
            ("\u{2075}".into(), "^5".into()),
            ("\u{2212}".into(), "-".into())
        ]
    );
}
