//! 해제본 스풀 전용 임시 폴더. 이름 없는 파일(`tempfile_in`)만 여기에 만든다 — 프로세스가 죽으면 OS 가 거둔다.
//! 폴더 자체(`<root>/<pid>/`)는 비어 남으므로 시작할 때 `sweep_stale_spools` 가 치운다.

use std::{
    fs::{self, File},
    io,
    path::PathBuf,
};

/// 스풀 뿌리. 환경변수 `SEOAN_SPOOL_DIR`(테스트 · 진단용)이 있으면 그것, 아니면 `<temp>/seoan-spool`.
fn spool_root() -> PathBuf {
    match std::env::var_os("SEOAN_SPOOL_DIR") {
        Some(root) if !root.is_empty() => PathBuf::from(root),
        _ => std::env::temp_dir().join("seoan-spool"),
    }
}

/// `<root>/<pid>/` — 없으면 만든다(유닉스 0700).
pub fn spool_dir() -> io::Result<PathBuf> {
    let dir = spool_root().join(std::process::id().to_string());
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
        return false;
    };
    // SAFETY: 신호 0 은 존재 확인뿐이다.
    let result = unsafe { libc::kill(pid, 0) };
    // EPERM = 있으나 남의 것.
    result == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// 뿌리 아래에서 주인 프로세스가 없는 `<pid>/` 폴더를 지운다. 지운 수를 돌려준다.
/// pid 숫자가 아닌 이름 · 심링크 · 뿌리 밖 · 시스템 임시 폴더의 옛 `.tmpXXXXXX` 는 건드리지 않는다.
/// 윈도는 프로세스 조회가 무거워 폴더 수정 시각이 24시간보다 오래된 것만 지운다(오래 사는 세션을 지울 위험과 맞바꿈).
pub fn sweep_stale_spools() -> usize {
    let Ok(entries) = fs::read_dir(spool_root()) else {
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
            .and_then(|name| name.parse::<u32>().ok())
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
        if stale && fs::remove_dir_all(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}
