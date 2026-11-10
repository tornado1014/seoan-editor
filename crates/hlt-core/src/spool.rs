//! 해제본 스풀 전용 임시 폴더. 이름 없는 파일(`tempfile_in`)만 여기에 만든다 — 프로세스가 죽으면 OS 가 거둔다.
//! 폴더 자체(`<root>/<pid>/`)는 비어 남으므로 시작할 때 `sweep_stale_spools` 가 치운다.

use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

use crate::{CoreError, ErrorCode};

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

/// 해제본을 이만큼 쓸 때마다 스풀 볼륨의 여유를 본다.
pub const SPOOL_CHECK_EVERY: u64 = 64 * 1024 * 1024;
/// 여유가 이보다 작으면 멈춘다.
pub const SPOOL_MIN_FREE: u64 = 2 * 1024 * 1024 * 1024;

type FreeProbe = fn(&Path) -> Option<u64>;
static PROBE: Mutex<Option<FreeProbe>> = Mutex::new(None);

/// 여유 조회를 갈아 끼운다(테스트 · 진단용 — `None` 이면 운영체제에 묻는다).
#[doc(hidden)]
pub fn set_free_space_probe(probe: Option<FreeProbe>) {
    *PROBE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = probe;
}

/// 그 폴더가 있는 볼륨의 쓸 수 있는 바이트. 못 재면 `None`(막지 않는다).
fn free_bytes(dir: &Path) -> Option<u64> {
    let probe = *PROBE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match probe {
        Some(probe) => probe(dir),
        None => os_free_bytes(dir),
    }
}

#[cfg(unix)]
fn os_free_bytes(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    // SAFETY: 영으로 채운 구조체를 statvfs 가 채운다.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: path 는 NUL 로 끝나는 유효한 C 문자열이다.
    if unsafe { libc::statvfs(path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    (stat.f_bavail as u64).checked_mul(stat.f_frsize as u64)
}

// ponytail: 윈도는 여유를 재지 않는다(= 확인 없음). GetDiskFreeSpaceExW 를 더하면 된다.
#[cfg(not(unix))]
fn os_free_bytes(_dir: &Path) -> Option<u64> {
    None
}

/// 스풀에 쓰는 양을 세다가 64MiB 마다 여유를 본다. 시작할 때도 한 번 본다.
pub(crate) struct SpoolGuard {
    dir: PathBuf,
    written: u64,
    every: u64,
    min_free: u64,
    /// 이 경비만 쓰는 조회(단위 시험용 · 없으면 전역 훅 또는 운영체제).
    probe: Option<FreeProbe>,
}

impl SpoolGuard {
    pub(crate) fn new() -> Result<Self, CoreError> {
        let dir =
            spool_dir().map_err(|_| CoreError::new(ErrorCode::IoError, "inspect_spool", false))?;
        Self::with_limits(dir, SPOOL_CHECK_EVERY, SPOOL_MIN_FREE, None)
    }

    fn with_limits(
        dir: PathBuf,
        every: u64,
        min_free: u64,
        probe: Option<FreeProbe>,
    ) -> Result<Self, CoreError> {
        let guard = Self {
            dir,
            written: 0,
            every,
            min_free,
            probe,
        };
        guard.check_free()?;
        Ok(guard)
    }

    fn check_free(&self) -> Result<(), CoreError> {
        let free = match self.probe {
            Some(probe) => probe(&self.dir),
            None => free_bytes(&self.dir),
        };
        match free {
            Some(free) if free < self.min_free => Err(low_disk(free, self.min_free)),
            _ => Ok(()),
        }
    }

    /// `bytes` 를 쓰기 직전에 부른다.
    pub(crate) fn account(&mut self, bytes: u64) -> Result<(), CoreError> {
        let before = self.written / self.every;
        self.written = self.written.saturating_add(bytes);
        if self.written / self.every != before {
            self.check_free()?;
        }
        Ok(())
    }
}

fn low_disk(free: u64, min_free: u64) -> CoreError {
    const GB: f64 = 1_000_000_000.0;
    let mut error = CoreError::new(ErrorCode::DiskFull, "spool_free_space", false);
    error.message = format!(
        "디스크 여유가 부족해 문서를 열 수 없습니다(여유 {:.1}GB · {}GB 이상 필요).",
        free as f64 / GB,
        (min_free as f64 / GB).round()
    );
    error.details.insert("free_bytes".into(), free.to_string());
    error
}

/// 쓰기 전에 `SpoolGuard` 를 거치는 `Write`(`inflate_raw_to_writer` 용). 모자라면 `CoreError` 를 실은 `io::Error`.
pub(crate) struct GuardedWriter<W> {
    pub(crate) inner: W,
    guard: SpoolGuard,
}

impl<W> GuardedWriter<W> {
    pub(crate) fn new(inner: W) -> Result<Self, CoreError> {
        Ok(Self {
            inner,
            guard: SpoolGuard::new()?,
        })
    }
}

impl<W: Write> Write for GuardedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.guard
            .account(buf.len() as u64)
            .map_err(io::Error::other)?;
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod guard_tests {
    use super::*;

    fn tiny(_: &Path) -> Option<u64> {
        Some(512)
    }

    #[test]
    fn checks_every_interval_and_not_before() {
        let dir = tempfile::tempdir().unwrap();
        // 문턱 0 이면 시작 확인은 통과 · 이어서 문턱을 올려 주기 확인만 본다.
        let mut guard = SpoolGuard::with_limits(dir.path().into(), 100, 0, Some(tiny)).unwrap();
        guard.min_free = 1024;
        assert!(guard.account(99).is_ok(), "간격 안에서는 보지 않는다");
        let error = guard.account(1).unwrap_err();
        assert_eq!(error.code, ErrorCode::DiskFull);
    }
}
