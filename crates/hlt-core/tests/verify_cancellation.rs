mod support;

use std::{
    io::Cursor,
    sync::{Arc, Mutex},
};

use hlt_core::{
    CancellationToken, ErrorCode, ProgressEvent, VerifyOptions, verify_path, verify_reader,
};

#[test]
fn cancellation_is_reported_from_an_injected_read_loop() {
    let token = CancellationToken::new();
    token.cancel();
    let mut cursor = Cursor::new(support::valid_stored_container(b"test"));

    let error = verify_reader(
        &mut cursor,
        "cancel.HLT",
        VerifyOptions {
            cancellation: token,
            ..Default::default()
        },
    )
    .unwrap_err();

    assert_eq!(error.code, ErrorCode::Cancelled);
}

#[test]
fn progress_callback_can_cancel_an_active_verification() {
    let token = CancellationToken::new();
    let callback_token = token.clone();
    let events = Arc::new(Mutex::new(Vec::<ProgressEvent>::new()));
    let callback_events = Arc::clone(&events);
    let progress = Arc::new(move |event: ProgressEvent| {
        if event.phase == "inflate" {
            callback_token.cancel();
        }
        callback_events.lock().unwrap().push(event);
    });
    let mut cursor = Cursor::new(support::valid_dynamic_container());

    let error = verify_reader(
        &mut cursor,
        "active-cancel.HLT",
        VerifyOptions {
            request_id: "req-cancel".to_string(),
            cancellation: token,
            progress: Some(progress),
            ..Default::default()
        },
    )
    .unwrap_err();

    assert_eq!(error.code, ErrorCode::Cancelled);
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .any(|event| event.phase == "inflate" && event.request_id == "req-cancel")
    );
}

#[test]
fn verification_does_not_change_input_or_create_a_sibling() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.HLT");
    let (body, stream) = support::far_backreference_stream();
    std::fs::write(&input, support::container_from_raw(&body, &stream)).unwrap();
    let before = std::fs::read(&input).unwrap();

    let error = verify_path(&input, VerifyOptions::default()).unwrap_err();

    assert_eq!(error.code, ErrorCode::WindowNotProven);
    assert_eq!(std::fs::read(&input).unwrap(), before);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
