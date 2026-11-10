mod support;

use std::process::{Command, Output};

/// 문턱을 1바이트로 낮춰(`SEOAN_LARGE_THRESHOLD_BYTES`) 작은 픽스처를 「큰 문서」로 만든다.
fn run(args: &[&str], envs: &[(&str, &str)]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hlt-cli"))
        .args(args)
        .env_remove("SEOAN_ALLOW_LARGE")
        .env("SEOAN_LARGE_THRESHOLD_BYTES", "1")
        .envs(envs.iter().copied())
        .current_dir(support::repo_root())
        .output()
        .unwrap()
}

#[test]
fn a_large_document_stops_without_the_flag_and_opens_with_it() {
    let fixture = support::pct_table_fixture();
    let path = fixture.path().to_str().unwrap();

    let refused = run(&["inspect", path, "--kind", "summary"], &[]);
    assert!(!refused.status.success());
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("이 문서는 풀면"), "{stderr}");
    assert!(
        stderr.contains("계속하려면 --allow-large 를 붙이거나 SEOAN_ALLOW_LARGE=1 을 두세요."),
        "{stderr}"
    );
    let envelope: serde_json::Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(envelope["error"]["code"], "large_document");
    support::validate_error_envelope(&envelope);

    let verify = run(&["verify", path], &[]);
    assert!(!verify.status.success());

    for (args, envs) in [
        (
            vec!["--allow-large", "inspect", path, "--kind", "summary"],
            vec![],
        ),
        (
            vec!["inspect", path, "--kind", "summary", "--allow-large"],
            vec![],
        ),
        (
            vec!["inspect", path, "--kind", "summary"],
            vec![("SEOAN_ALLOW_LARGE", "1")],
        ),
    ] {
        let allowed = run(&args, &envs);
        assert!(
            allowed.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&allowed.stderr)
        );
        let envelope: serde_json::Value = serde_json::from_slice(&allowed.stdout).unwrap();
        assert_eq!(envelope["ok"], true);
    }
}
