use hlt_core::{ErrorCode, FirstBlockKind, inspect_first_block};

#[test]
fn decodes_dynamic_distance_field_as_count() {
    let evidence = inspect_first_block(&[0xec, 0xb7, 0xf9]).unwrap();

    assert_eq!(evidence.kind, FirstBlockKind::Dynamic);
    assert_eq!(evidence.hlit, Some(286));
    assert_eq!(evidence.hdist, Some(24));
    assert_eq!(evidence.hclen, Some(17));
    assert!(evidence.window_ok);
}

#[test]
fn rejects_dynamic_block_with_decoded_distance_count_25() {
    let evidence = inspect_first_block(&[0xec, 0xb8, 0xf9]).unwrap();

    assert_eq!(evidence.hdist, Some(25));
    assert!(!evidence.window_ok);
}

#[test]
fn stored_and_fixed_first_blocks_do_not_prove_the_window() {
    let stored = inspect_first_block(&[0x01]).unwrap();
    let fixed = inspect_first_block(&[0x03]).unwrap();

    assert_eq!(stored.kind, FirstBlockKind::Stored);
    assert_eq!(fixed.kind, FirstBlockKind::Fixed);
    assert!(!stored.window_ok);
    assert!(!fixed.window_ok);
}

#[test]
fn truncated_dynamic_header_is_invalid() {
    let error = inspect_first_block(&[0x05]).unwrap_err();

    assert_eq!(error.code, ErrorCode::DeflateInvalid);
}
