//! 해제본 스풀 전용 임시 폴더. 이름 없는 파일(`tempfile_in`)만 여기에 만든다 — 프로세스가 죽으면 OS 가 거둔다.
//! 폴더 자체(`<root>/<pid>/`)는 비어 남으므로 시작할 때 `sweep_stale_spools` 가 치운다.

use std::{
    fs::{self, File},
    io,
    path::PathBuf,
};

/// 스풀 뿌리 `<바탕>/seoan-spool`. 바탕은 환경변수 `SEOAN_SPOOL_DIR`(테스트 · 진단용) 아니면 시스템 임시 폴더.
/// 어느 쪽이든 전용 이름 `seoan-spool` 을 붙인다 — 바탕이 `$HOME` 이어도 그 안의 남의 폴더를 보지 않는다.
fn spool_root() -> PathBuf {
    match std::env::var_os("SEOAN_SPOOL_DIR") {
        Some(base) if !base.is_empty() => PathBuf::from(base),
        _ => std::env::temp_dir(),
    }
    .join("seoan-spool")
}

/// 뿌리가 있다면 심링크가 아닌 진짜 폴더여야 한다(심링크를 따라 남의 폴더에 쓰거나 지우지 않는다).
fn root_is_real_dir_or_absent(root: &std::path::Path) -> bool {
    match fs::symlink_metadata(root) {
        Ok(meta) => meta.is_dir(),
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    }
}

/// `<root>/<pid>/` — 없으면 만든다(유닉스 0700).
pub fn spool_dir() -> io::Result<PathBuf> {
    let root = spool_root();
    if !root_is_real_dir_or_absent(&root) {
        return Err(io::Error::other("seoan-spool is not a plain directory"));
    }
    let dir = root.join(std::process::id().to_string());
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(&dir)?;
    Ok(dir)
}

/// 이름 없는 스풀 파일 하나(유닉스: 만들자마자 unlink · 윈도: 닫히면 삭제).
pub(crate) fn spool_file() -> io::Result<File> {
    tempfile::tempfile_in(spool_dir()?)
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return true; // 못 재면 건드리지 않는다
    };
    // SAFETY: 신호 0 은 존재 확인뿐이다.
    let result = unsafe { libc::kill(pid, 0) };
    // EPERM = 있으나 남의 것.
    result == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// 뿌리 아래에서 주인 프로세스가 없는 **빈** `<pid>/` 폴더를 지운다(재귀 삭제 없음 — 비어 있지 않으면 남기고 세지 않는다).
/// 뿌리가 심링크이거나 폴더가 아니면 아무것도 하지 않고 0 을 돌려준다.
/// pid 숫자가 아닌 이름 · 심링크 · 뿌리 밖 · 시스템 임시 폴더의 옛 `.tmpXXXXXX` 는 건드리지 않는다.
/// 윈도는 프로세스 조회가 무거워 폴더 수정 시각이 24시간보다 오래된 것만, 그것도 빈 폴더만 지운다
/// (윈도의 닫히면 삭제 파일은 열려 있는 동안 이름이 보이므로, 살아 있는 세션의 폴더는 비어 있지 않아 지워지지 않는다).
pub fn sweep_stale_spools() -> usize {
    let root = spool_root();
    if !matches!(fs::symlink_metadata(&root), Ok(meta) if meta.is_dir()) {
        return 0;
    }
    let Ok(entries) = fs::read_dir(&root) else {
        return 0;
    };
    let own = std::process::id();
    let mut removed = 0;
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_dir() {
            continue; // 심링크는 is_dir 가 거짓이다
        }
        let Some(pid) = entry
            .file_name()
            .to_str()
            .filter(|name| !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|name| name.parse::<i32>().ok()) // 범위 밖 숫자는 건너뛴다(죽은 것으로 보지 않는다)
            .and_then(|pid| u32::try_from(pid).ok())
        else {
            continue;
        };
        if pid == own {
            continue;
        }
        #[cfg(unix)]
        let stale = !process_alive(pid);
        #[cfg(not(unix))]
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|time| time.elapsed().ok())
            .is_some_and(|age| age > std::time::Duration::from_secs(24 * 3600));
        if stale && fs::remove_dir(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}
