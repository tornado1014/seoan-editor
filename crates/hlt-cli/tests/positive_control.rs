mod support;

use std::process::Command;

#[test]
fn positive_control_emits_one_schema_valid_json_line_and_exit_zero() {
    let output = Command::new(env!("CARGO_BIN_EXE_hlt-cli"))
        .args(["verify", "work/pct_a/pct_a_full_v5.HLT"])
        .current_dir(support::repo_root())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, b"");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines.len(), 1);
    let value: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    support::validate_success_envelope(&value);
    assert_eq!(value["contract_version"], 1);
    assert_eq!(value["command"], "verify");
    assert_eq!(value["ok"], true);
    assert_eq!(value["result"]["file_size"], "25715558");
    assert_eq!(value["result"]["inflated"], "469761837");
    assert_eq!(value["result"]["footer_crc_ok"], true);
    assert_eq!(value["result"]["footer_size_ok"], true);
    assert_eq!(value["result"]["window_ok"], true);
    let text = lines[0].to_ascii_lowercase();
    for forbidden in ["body", "raw", "image_bytes", "base64"] {
        assert!(!text.contains(forbidden));
    }
}
