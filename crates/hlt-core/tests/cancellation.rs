use hlt_core::{CancellationToken, VerifyOptions};

#[test]
fn cancellation_token_changes_only_after_cancel() {
    let token = CancellationToken::new();
    assert!(!token.is_cancelled());
    token.cancel();
    assert!(token.is_cancelled());
}

#[test]
fn verify_defaults_are_bounded_and_not_cancelled() {
    let options = VerifyOptions::default();

    assert_eq!(options.max_inflated_bytes, 4 * 1024 * 1024 * 1024);
    assert!(options.request_id.is_empty());
    assert!(!options.cancellation.is_cancelled());
    assert!(options.progress.is_none());
}
