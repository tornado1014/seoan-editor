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

/// 이 바이너리의 모든 테스트가 같은 스풀 바탕을 쓴다(환경변수는 프로세스 전역이라 한 번만 정한다).
/// 스풀 뿌리는 그 아래 `seoan-spool` 이다.
fn root() -> PathBuf {
    INIT.call_once(|| {
        let path = match std::env::var_os("SEOAN_SPOOL_DIR") {
            Some(existing) => PathBuf::from(existing), // 자식 프로세스: 부모가 정해 준 폴더
            None => ROOT
                .get_or_init(|| tempfile::tempdir().unwrap())
                .path()
                .to_path_buf(),
        };
        // SAFETY: 모든 테스트가 첫 줄에서 `root()` 를 불러 이 호출이 끝난 뒤에만 환경을 읽는다.
        unsafe { std::env::set_var("SEOAN_SPOOL_DIR", &path) };
    });
    PathBuf::from(std::env::var_os("SEOAN_SPOOL_DIR").unwrap()).join("seoan-spool")
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
        .env("SEOAN_SPOOL_DIR", root.parent().unwrap())
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

    // 다른 테스트의 죽은 자식이 남긴 빈 폴더도 같이 치워질 수 있다.
    assert!(sweep_stale_spools() >= 1);

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

#[test]
#[ignore = "자식 프로세스 도우미 — run_sweep 이 부른다"]
fn sweep_helper() {
    println!("SWEPT={}", sweep_stale_spools());
}

/// 자기만의 바탕(`SEOAN_SPOOL_DIR`)으로 sweep 을 자식에서 돌려 지운 수를 돌려준다.
fn run_sweep(base: &Path) -> usize {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "sweep_helper", "--ignored", "--nocapture"])
        .env("SEOAN_SPOOL_DIR", base)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("SWEPT="))
        .unwrap()
        .parse()
        .unwrap()
}

fn dead_pid() -> i32 {
    (3_000_000..3_000_100).find(|pid| is_dead(*pid)).unwrap()
}

#[test]
fn sweep_never_deletes_non_empty_folders() {
    let base = tempfile::tempdir().unwrap();
    let folder = base.path().join("seoan-spool").join(dead_pid().to_string());
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("data"), b"x").unwrap();
    assert_eq!(run_sweep(base.path()), 0);
    assert!(folder.join("data").exists());
}

#[test]
fn sweep_only_looks_under_the_seoan_spool_suffix() {
    let base = tempfile::tempdir().unwrap();
    let folder = base.path().join(dead_pid().to_string());
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("data"), b"x").unwrap();
    let empty = base.path().join((dead_pid() + 1).to_string());
    fs::create_dir_all(&empty).unwrap();
    assert_eq!(run_sweep(base.path()), 0);
    assert!(folder.join("data").exists());
    assert!(empty.exists());
}

#[test]
fn sweep_skips_pid_names_out_of_range() {
    let base = tempfile::tempdir().unwrap();
    let folder = base.path().join("seoan-spool").join("99999999999");
    fs::create_dir_all(&folder).unwrap();
    assert_eq!(run_sweep(base.path()), 0);
    assert!(folder.exists());
}

#[test]
fn sweep_ignores_a_symlinked_root() {
    let base = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let empty = elsewhere.path().join(dead_pid().to_string());
    fs::create_dir_all(&empty).unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), base.path().join("seoan-spool")).unwrap();
    assert_eq!(run_sweep(base.path()), 0);
    assert!(empty.exists());
}

#[test]
fn spool_dir_refuses_a_symlinked_root() {
    let base = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), base.path().join("seoan-spool")).unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "spool_dir_helper", "--ignored"])
        .env("SEOAN_SPOOL_DIR", base.path())
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(fs::read_dir(elsewhere.path()).unwrap().count(), 0);
}

#[test]
#[ignore = "자식 프로세스 도우미 — spool_dir_refuses_a_symlinked_root 가 부른다"]
fn spool_dir_helper() {
    assert!(spool_dir().is_err());
}

/// 자식에서 합성 문서를 열어 성공 여부를 돌려준다(스풀 바탕 = `base`).
fn open_in_child(base: &Path) -> bool {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "open_helper", "--ignored"])
        .env("SEOAN_SPOOL_DIR", base)
        .stdout(std::process::Stdio::null()) // 거절을 시험하는 자식의 실패 출력이 스위트 줄에 섞이지 않게
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}

#[test]
#[ignore = "자식 프로세스 도우미 — 스풀 뿌리 시험이 부른다(열리면 성공)"]
fn open_helper() {
    let _ = root();
    let fixture = support::pct_table_fixture();
    inspect_path(fixture.path(), InspectOptions::default()).unwrap();
}

#[test]
fn an_unusable_spool_root_falls_back_to_the_system_temp_folder() {
    let base = tempfile::tempdir().unwrap();
    fs::write(base.path().join("seoan-spool"), b"not a folder").unwrap();
    assert!(
        open_in_child(base.path()),
        "일반 파일이 뿌리 자리에 있어도 열려야 한다"
    );
}

#[test]
fn a_symlinked_spool_root_still_fails_the_open() {
    let base = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), base.path().join("seoan-spool")).unwrap();
    assert!(!open_in_child(base.path()), "심링크 뿌리는 폴백 없이 거절");
    assert_eq!(fs::read_dir(elsewhere.path()).unwrap().count(), 0);
}
