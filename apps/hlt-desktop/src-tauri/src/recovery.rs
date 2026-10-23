//! 자동 저장 · 복구: 고친 뒤 손을 멈추면 지금 판을 앱 자료 폴더 `recovery/` 에 둔다(`<프로세스>-<탭>.HLT` + `.json`).
//! 저장하거나 탭을 닫으면(저장 안 함 포함) 지운다 — 남은 것은 앱이 비정상으로 끝난 문서뿐이라 다음 실행 때 복구를 묻는다.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct Entry {
    /// 복구본(.HLT).
    #[serde(skip)]
    pub hlt: PathBuf,
    /// 원래 파일(새 문서면 없다).
    pub source: Option<PathBuf>,
    /// 탭 제목(파일 이름).
    pub title: String,
}

pub fn dir(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join("recovery"))
}

/// 이 실행 · 이 탭의 이름(다음 실행의 같은 탭 이름과 겹치지 않게 프로세스 번호를 붙인다).
pub fn key(label: &str) -> String {
    format!("{}-{label}", std::process::id())
}

/// 복구본을 둔다(옆 임시 파일에 다 쓴 뒤 바꿔 끼운다 — 쓰다 끊겨도 앞 복구본이 남는다).
pub fn store(dir: &Path, key: &str, current: &Path, entry: &Entry) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let hlt = dir.join(format!("{key}.HLT"));
    let temporary = dir.join(format!("{key}.HLT.tmp"));
    std::fs::copy(current, &temporary)?;
    std::fs::rename(&temporary, &hlt)?;
    std::fs::write(dir.join(format!("{key}.json")), serde_json::to_vec(entry)?)
}

pub fn remove(dir: &Path, key: &str) {
    for extension in ["HLT", "json", "HLT.tmp"] {
        let _ = std::fs::remove_file(dir.join(format!("{key}.{extension}")));
    }
}

/// 다른 실행이 남긴 복구본(설명 · 복구본이 다 있는 것만).
pub fn leftovers(dir: &Path, pid: u32) -> Vec<Entry> {
    let mine = format!("{pid}-");
    let mut out: Vec<Entry> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|item| item.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter(|path| {
            !path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(&mine))
        })
        .filter_map(|path| {
            let mut entry: Entry = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
            entry.hlt = path.with_extension("HLT");
            entry.hlt.is_file().then_some(entry)
        })
        .collect();
    out.sort_by(|a, b| a.hlt.cmp(&b.hlt));
    out
}

/// 이 경로가 복구본인가(열 때 원래 파일 대신 「다른 이름으로」 저장하게 한다).
pub fn entry_of(app: &AppHandle, path: &Path) -> Option<Entry> {
    let dir = dir(app)?;
    if path.parent()? != dir {
        return None;
    }
    let mut entry: Entry =
        serde_json::from_slice(&std::fs::read(path.with_extension("json")).ok()?).ok()?;
    entry.hlt = path.to_path_buf();
    Some(entry)
}

/// 저장했거나 탭을 닫았다 — 그 탭 복구본은 필요 없다.
pub fn discard(app: &AppHandle, label: &str) {
    if let Some(dir) = dir(app) {
        remove(&dir, &key(label));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_lists_other_runs_only_and_removes() {
        let root = std::env::temp_dir().join(format!("seoan-recovery-{}", std::process::id()));
        let dir = root.join("recovery");
        let current = root.join("v3.HLT");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&current, b"HLT").unwrap();
        let entry = |title: &str| Entry {
            hlt: PathBuf::new(),
            source: Some(PathBuf::from("/d/명세서.HLT")),
            title: title.into(),
        };
        store(&dir, "1-main", &current, &entry("지난 실행")).unwrap();
        store(&dir, "2-doc-1", &current, &entry("이 실행")).unwrap();
        // 설명만 남고 복구본이 없는 것은 뺀다.
        std::fs::write(
            dir.join("3-main.json"),
            serde_json::to_vec(&entry("깨짐")).unwrap(),
        )
        .unwrap();
        let found = leftovers(&dir, 2);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "지난 실행");
        assert_eq!(found[0].hlt, dir.join("1-main.HLT"));
        assert_eq!(std::fs::read(&found[0].hlt).unwrap(), b"HLT");
        remove(&dir, "1-main");
        assert!(leftovers(&dir, 2).is_empty());
        assert!(!dir.join("1-main.HLT").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
