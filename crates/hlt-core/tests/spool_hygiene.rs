//! 해제본 스풀 위생: 이름 붙은 큰 파일이 남지 않고, 죽은 프로세스의 폴더는 치워진다.
#![cfg(unix)]
mod support;

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Once, OnceLock},
};

use hlt_core::{InspectOptions, inspect_path, spool_dir, sweep_stale_spools};
use tempfile::TempDir;

static ROOT: OnceLock<TempDir> = OnceLock::new();
static INIT: Once = Once::new();
/// 시스템 임시 폴더를 훑는 두 테스트가 서로의 파일을 보지 않게 줄 세운다.
static TEMP_SCAN: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 이 바이너리의 모든 테스트가 같은 스풀 뿌리를 쓴다(환경변수는 프로세스 전역이라 한 번만 정한다).
fn root() -> PathBuf {
    INIT.call_once(|| {
        let path = match std::env::var_os("SEOAN_SPOOL_DIR") {
            Some(existing) => PathBuf::from(existing), // 자식 프로세스: 부모가 정해 준 폴더
            None => {
                let dir = ROOT.get_or_init(|| tempfile::tempdir().unwrap());
                dir.path().join("seoan-spool")
            }
        };
        // SAFETY: 모든 테스트가 첫 줄에서 `root()` 를 불러 이 호출이 끝난 뒤에만 환경을 읽는다.
        unsafe { std::env::set_var("SEOAN_SPOOL_DIR", &path) };
    });
    PathBuf::from(std::env::var_os("SEOAN_SPOOL_DIR").unwrap())
}

/// 일반 파일 중 0바이트보다 큰 것들의 (경로, 크기). 심링크는 따라가지 않는다.
fn nonempty_files(dir: &Path) -> Vec<(PathBuf, u64)> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let meta = fs::symlink_metadata(entry.path()).unwrap();
        if meta.is_dir() {
            found.extend(nonempty_files(&entry.path()));
        } else if meta.is_file() && meta.len() > 0 {
            found.push((entry.path(), meta.len()));
        }
    }
    found
}

fn fixture_copy(dir: &Path) -> PathBuf {
    let source = support::pct_table_fixture();
    let copy = dir.join("fixture.HLT");
    fs::copy(source.path(), &copy).unwrap();
    copy
}

fn temp_dir_tmp_files() -> Vec<PathBuf> {
    fs::read_dir(std::env::temp_dir())
        .unwrap()
        .flatten()
        .filter(|entry| {
            // 접미사 .HLT 는 픽스처(support) — 스풀은 접미사가 없다.
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with(".tmp") && !name.ends_with(".HLT")
        })
        .filter(|entry| fs::symlink_metadata(entry.path()).is_ok_and(|meta| meta.is_file()))
        .map(|entry| entry.path())
        .collect()
}

/// 이 단언은 이 바이너리에서 이 테스트만 한다(시스템 임시 폴더의 새 `.tmp*` 파일은 다른 테스트 잡음에 약하다).
#[test]
fn open_document_leaves_no_named_spool_file() {
    let root = root();
    let _scan = TEMP_SCAN.lock().unwrap_or_else(|e| e.into_inner());
    let work = tempfile::tempdir().unwrap();
    let fixture = fixture_copy(work.path());
    let before = temp_dir_tmp_files();
    let document = inspect_path(&fixture, InspectOptions::default()).unwrap();
    assert!(!document.paragraphs.is_empty());
    assert!(
        nonempty_files(&root).is_empty(),
        "스풀 폴더에 이름 붙은 파일이 있다: {:?}",
        nonempty_files(&root)
    );
    let new: Vec<_> = temp_dir_tmp_files()
        .into_iter()
        .filter(|path| !before.contains(path))
        .collect();
    assert!(new.is_empty(), "시스템 임시 폴더에 새 .tmp 파일: {new:?}");
    drop(document);
}

#[test]
#[ignore = "자식 프로세스 도우미 — abort_leaves_nothing 이 부른다"]
fn abort_helper() {
    let fixture = PathBuf::from(std::env::var_os("SEOAN_TEST_FIXTURE").unwrap());
    let _document = inspect_path(&fixture, InspectOptions::default()).unwrap();
    println!("OPENED");
    std::process::abort();
}

#[test]
fn abort_leaves_nothing() {
    let root = root();
    let _scan = TEMP_SCAN.lock().unwrap_or_else(|e| e.into_inner());
    let work = tempfile::tempdir().unwrap();
    let fixture = fixture_copy(work.path());
    let before = temp_dir_tmp_files();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "abort_helper", "--ignored", "--nocapture"])
        .env("SEOAN_SPOOL_DIR", &root)
        .env("SEOAN_TEST_FIXTURE", &fixture)
        .output()
        .unwrap();
    assert!(!output.status.success(), "자식이 abort 로 죽어야 한다");
    assert!(String::from_utf8_lossy(&output.stdout).contains("OPENED"));
    // 자식은 pid 폴더를 만들었다(스풀이 거기 있었다) — 그래도 파일은 0바이트 합이어야 한다.
    let total: u64 = nonempty_files(&root).iter().map(|(_, size)| size).sum();
    assert_eq!(total, 0);
    let leftover: Vec<_> = temp_dir_tmp_files()
        .into_iter()
        .filter(|path| !before.contains(path))
        .collect();
    assert!(
        leftover.is_empty(),
        "죽은 자식이 시스템 임시 폴더에 남김: {leftover:?}"
    );
}

fn is_dead(pid: i32) -> bool {
    // SAFETY: 신호 0.
    unsafe {
        libc::kill(pid, 0) != 0
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    }
}

#[test]
fn sweep_removes_only_dead_pid_folders() {
    let root = root();
    fs::create_dir_all(&root).unwrap();
    let dead = (3_000_000..3_000_100).find(|pid| is_dead(*pid)).unwrap();
    let dead_dir = root.join(dead.to_string());
    fs::create_dir_all(&dead_dir).unwrap();
    fs::write(dead_dir.join("leftover"), b"x").unwrap();
    let own_dir = spool_dir().unwrap();
    assert!(own_dir.ends_with(std::process::id().to_string()));
    // 살아 있는 다른 프로세스(부모)의 폴더는 남는다.
    let alive_dir = root.join(unsafe { libc::getppid() }.to_string());
    fs::create_dir_all(&alive_dir).unwrap();
    let named = root.join("not-a-pid");
    fs::create_dir_all(&named).unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("keep"), b"x").unwrap();
    let link = root.join((dead + 1).to_string());
    std::os::unix::fs::symlink(outside.path(), &link).unwrap();

    assert_eq!(sweep_stale_spools(), 1);

    assert!(!dead_dir.exists());
    assert!(own_dir.exists());
    assert!(alive_dir.exists());
    assert!(named.exists());
    assert!(fs::symlink_metadata(&link).is_ok());
    assert!(outside.path().join("keep").exists());
}

#[test]
#[ignore = "자식 프로세스 도우미 — sweep_without_root_is_zero 가 부른다"]
fn sweep_missing_root_helper() {
    let _ = root();
    assert_eq!(sweep_stale_spools(), 0);
}

#[test]
fn sweep_without_root_is_zero() {
    let _ = root();
    let missing = tempfile::tempdir().unwrap().path().join("nope");
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "sweep_missing_root_helper", "--ignored"])
        .env("SEOAN_SPOOL_DIR", &missing)
        .status()
        .unwrap();
    assert!(status.success());
}
