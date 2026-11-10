//! 디스크 여유 확인(스풀에 쓰는 동안)과 큰 문서 확인(해제 전).
mod support;

use std::{path::Path, sync::Mutex};

use hlt_core::{
    ErrorCode, InspectOptions, VerifyOptions, inspect_path, set_free_space_probe, verify_path,
};

/// 여유 조회 훅은 프로세스 전역이다 — 줄 세운다.
static SERIAL: Mutex<()> = Mutex::new(());

const GIB: u64 = 1024 * 1024 * 1024;

fn one_gib(_: &Path) -> Option<u64> {
    Some(GIB)
}
fn ten_gib(_: &Path) -> Option<u64> {
    Some(10 * GIB)
}
fn unknown(_: &Path) -> Option<u64> {
    None
}

fn spool_files(base: &Path) -> usize {
    fn count(dir: &Path) -> usize {
        std::fs::read_dir(dir).map_or(0, |entries| {
            entries
                .flatten()
                .map(|entry| {
                    if entry.path().is_dir() {
                        count(&entry.path())
                    } else {
                        1
                    }
                })
                .sum()
        })
    }
    count(&base.join("seoan-spool"))
}

#[test]
fn low_free_space_stops_the_open_and_leaves_nothing_in_the_spool_folder() {
    let _guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let base = tempfile::tempdir().unwrap();
    // SAFETY: 이 파일의 테스트는 SERIAL 아래에서만 환경을 건드린다.
    unsafe { std::env::set_var("SEOAN_SPOOL_DIR", base.path()) };
    let fixture = support::pct_table_fixture();

    set_free_space_probe(Some(one_gib));
    let error = inspect_path(fixture.path(), InspectOptions::default())
        .err()
        .unwrap();
    set_free_space_probe(None);

    assert_eq!(error.code, ErrorCode::DiskFull);
    assert!(
        error.message.contains("디스크 여유가 부족"),
        "{}",
        error.message
    );
    assert!(error.message.contains("2GB 이상 필요"), "{}", error.message);
    assert_eq!(spool_files(base.path()), 0);
}

#[test]
fn enough_or_unknown_free_space_reads_the_same_as_before() {
    let _guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let fixture = support::pct_table_fixture();
    set_free_space_probe(None);
    let baseline = inspect_path(fixture.path(), InspectOptions::default()).unwrap();
    for probe in [ten_gib as fn(&Path) -> Option<u64>, unknown] {
        set_free_space_probe(Some(probe));
        let again = inspect_path(fixture.path(), InspectOptions::default()).unwrap();
        assert_eq!(again.paragraphs.len(), baseline.paragraphs.len());
        assert_eq!(
            again.summary.paragraph_count,
            baseline.summary.paragraph_count
        );
    }
    set_free_space_probe(None);
}

fn small_threshold_inspect(allow: bool) -> InspectOptions {
    InspectOptions {
        large_threshold_bytes: 1,
        allow_large: allow,
        ..InspectOptions::default()
    }
}

#[test]
fn a_large_document_is_refused_before_anything_is_inflated() {
    let _guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let base = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("SEOAN_SPOOL_DIR", base.path()) };
    let fixture = support::pct_table_fixture();

    let error = inspect_path(fixture.path(), small_threshold_inspect(false))
        .err()
        .unwrap();

    assert_eq!(error.code, ErrorCode::LargeDocument);
    assert!(
        error.message.contains("이 문서는 풀면"),
        "{}",
        error.message
    );
    assert!(error.message.contains("GB 입니다"), "{}", error.message);
    assert!(error.details.contains_key("inflated_bytes"));
    assert!(
        !base.path().join("seoan-spool").exists(),
        "스풀 폴더조차 만들지 않는다"
    );

    let verify = VerifyOptions {
        large_threshold_bytes: 1,
        allow_large: false,
        ..VerifyOptions::default()
    };
    let error = verify_path(fixture.path(), verify).unwrap_err();
    assert_eq!(error.code, ErrorCode::LargeDocument);
}

#[test]
fn allowing_the_large_document_opens_it_with_the_same_result() {
    let _guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let fixture = support::pct_table_fixture();
    let baseline = inspect_path(fixture.path(), InspectOptions::default()).unwrap();

    let allowed = inspect_path(fixture.path(), small_threshold_inspect(true)).unwrap();

    assert_eq!(allowed.paragraphs.len(), baseline.paragraphs.len());
    assert_eq!(
        allowed.summary.paragraph_count,
        baseline.summary.paragraph_count
    );
}

#[test]
fn the_process_wide_allowance_lets_default_options_through() {
    let _guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let fixture = support::pct_table_fixture();
    hlt_core::set_allow_large(true);
    let options = InspectOptions {
        large_threshold_bytes: 1,
        ..InspectOptions::default()
    };
    let result = inspect_path(fixture.path(), options);
    hlt_core::set_allow_large(false);
    assert!(result.is_ok());
}
