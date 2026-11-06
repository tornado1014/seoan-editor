mod support;

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(support::repo_root())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn version() -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_hlt-cli"))
        .arg("version")
        .current_dir(support::repo_root())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, b"");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    serde_json::from_str(stdout.trim_end()).unwrap()
}

#[test]
fn version_is_one_success_envelope_with_the_build_source() {
    let value = version();
    assert_eq!(value["ok"], true);
    assert_eq!(value["command"], "version");
    let result = &value["result"];
    assert!(result["package_version"].is_string());
    assert!(result["git_tag"].is_string());
    assert!(result["dirty"].is_boolean());
    let commit = result["git_commit"].as_str().unwrap();
    assert!(
        commit == "unknown"
            || (commit.len() == 40 && commit.chars().all(|c| c.is_ascii_hexdigit())),
        "{commit}"
    );
}

/// A harness decides whether to trust the binary from these fields, so they
/// must describe the source this binary was built from, not a guess.
#[test]
fn version_matches_the_checkout_it_was_built_from() {
    let Some(head) = git(&["rev-parse", "HEAD"]) else {
        return;
    };
    let result = version()["result"].clone();
    assert_eq!(result["git_commit"], head.as_str());
    let changed = git(&[
        "status",
        "--porcelain",
        "--",
        "crates",
        "Cargo.toml",
        "Cargo.lock",
    ])
    .unwrap();
    assert_eq!(result["dirty"], !changed.is_empty(), "{changed}");
    let tag = git(&["describe", "--tags", "--exact-match", "HEAD"]).unwrap_or_default();
    assert_eq!(result["git_tag"], tag.as_str());
}
