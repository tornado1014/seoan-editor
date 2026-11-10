//! 큰 문서 허락은 문서(경로) 단위다 — 한 문서를 허락해도 다른 문서 · 일괄 처리는 그대로 묻는다.
mod support;

use std::{path::Path, path::PathBuf, sync::Mutex};

use hlt_core::{
    EditorSession, ErrorCode, InspectOptions, VerifyOptions, allow_large_path,
    batch_submission_checks, inspect_path, large_threshold_default, verify_path,
};

/// 문턱 환경변수는 프로세스 전역이다 — 줄 세운다.
static SERIAL: Mutex<()> = Mutex::new(());

/// 문턱을 1바이트로 낮춘다(작은 픽스처가 「큰 문서」).
struct Small;
impl Small {
    fn on() -> Self {
        // SAFETY: SERIAL 아래에서만 부른다.
        unsafe { std::env::set_var("SEOAN_LARGE_THRESHOLD_BYTES", "1") };
        Small
    }
}
impl Drop for Small {
    fn drop(&mut self) {
        unsafe { std::env::remove_var("SEOAN_LARGE_THRESHOLD_BYTES") };
    }
}

fn copy_of_fixture(dir: &Path, name: &str) -> PathBuf {
    let copy = dir.join(name);
    std::fs::copy(support::pct_table_fixture().path(), &copy).unwrap();
    copy
}

#[test]
fn allowing_one_document_does_not_allow_another() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let _small = Small::on();
    let dir = tempfile::tempdir().unwrap();
    let a = copy_of_fixture(dir.path(), "a.HLT");
    let b = copy_of_fixture(dir.path(), "b.HLT");
    allow_large_path(&a);

    assert!(inspect_path(&a, InspectOptions::default()).is_ok());
    let error = inspect_path(&b, InspectOptions::default()).err().unwrap();
    assert_eq!(error.code, ErrorCode::LargeDocument);
    assert_eq!(
        verify_path(&b, VerifyOptions::default()).unwrap_err().code,
        ErrorCode::LargeDocument
    );
}

#[test]
fn a_vanished_path_never_panics() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    allow_large_path(Path::new("/no/such/dir/ghost.HLT"));
}

#[test]
fn a_batch_skips_large_files_one_by_one_and_goes_on() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let _small = Small::on();
    let folder = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    copy_of_fixture(folder.path(), "c.HLT");
    copy_of_fixture(folder.path(), "d.HLT");
    // 다른 곳 문서를 허락해 둬도 일괄 처리에는 새지 않는다.
    allow_large_path(&copy_of_fixture(elsewhere.path(), "x.HLT"));

    let rows = batch_submission_checks(folder.path(), |_, _, _| true);

    assert_eq!(rows.len(), 2);
    for row in rows {
        let reason = row.error.expect("건너뜀");
        assert!(reason.contains("GB — 건너뜀"), "{reason}");
        assert!(reason.starts_with("풀면 "), "{reason}");
    }
}

#[test]
fn an_allowed_session_rereads_and_saves_under_a_new_name() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let _small = Small::on();
    let dir = tempfile::tempdir().unwrap();
    let a = copy_of_fixture(dir.path(), "a.HLT");
    let b = copy_of_fixture(dir.path(), "b.HLT");
    assert_eq!(
        EditorSession::open(&b).err().map(|e| e.code),
        Some(ErrorCode::LargeDocument)
    );
    allow_large_path(&a);

    let mut session = EditorSession::open(&a).unwrap();
    let _ = session.paragraphs(); // 세션 사본을 다시 읽는 길(저장이 모델 읽기를 기다린다)
    let target = dir.path().join("renamed.HLT");
    session.save_over(&target).unwrap(); // 새 이름 저장 + 되읽기 검증
    assert!(target.exists());
}

#[test]
fn threshold_env_ignores_zero_and_garbage() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    for bad in ["0", "abc", "", "-5"] {
        unsafe { std::env::set_var("SEOAN_LARGE_THRESHOLD_BYTES", bad) };
        assert_eq!(
            large_threshold_default(),
            hlt_core::LARGE_THRESHOLD_BYTES,
            "{bad:?}"
        );
    }
    unsafe { std::env::set_var("SEOAN_LARGE_THRESHOLD_BYTES", "5") };
    assert_eq!(large_threshold_default(), 5);
    unsafe { std::env::remove_var("SEOAN_LARGE_THRESHOLD_BYTES") };
}

#[test]
fn an_impossible_declared_size_is_garbage_not_a_large_document() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let _small = Small::on();
    let mut container = support::valid_dynamic_container();
    container[0x17] = 1;
    container.extend_from_slice(b"\r\nX"); // 꼬리가 어긋나 선언 크기가 쓰레기
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), &container).unwrap();
    let error = verify_path(file.path(), VerifyOptions::default()).unwrap_err();
    assert_ne!(error.code, ErrorCode::LargeDocument);
}
