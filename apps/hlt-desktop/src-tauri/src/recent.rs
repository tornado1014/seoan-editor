//! 최근 문서 · 탭 복원: 앱 설정 폴더 `recent.json` 에 열었던 파일(최근 10개)과 열린 탭(차례대로)을 적는다.
//! 다음 실행 때 탭은 디스크의 파일을 다시 연다(저장 안 한 고침은 끝낼 때 이미 물었다).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const LIMIT: usize = 10;

/// 읽고 고쳐 쓰기를 한 번에 하나씩 — 탭 여럿이 함께 열리면(복원) 서로의 쓰기를 덮었다(실측: 둘 연 뒤 최근 목록에 하나).
static FILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Default, Serialize, Deserialize, PartialEq, Debug)]
pub struct Recent {
    #[serde(default)]
    pub recent: Vec<PathBuf>,
    #[serde(default)]
    pub tabs: Vec<PathBuf>,
}

/// 맨 앞에 넣는다(같은 경로는 빼고 · 10개까지).
pub fn push_recent(list: &[PathBuf], path: &Path) -> Vec<PathBuf> {
    std::iter::once(path.to_path_buf())
        .chain(list.iter().filter(|item| item.as_path() != path).cloned())
        .take(LIMIT)
        .collect()
}

/// 탭 차례: 첫 창 `main` 다음 `doc-N` 을 N 숫자 차례로(글자 차례면 doc-10 이 doc-2 앞에 온다).
pub fn tab_order(label: &str) -> (u8, usize) {
    match label.strip_prefix("doc-").and_then(|n| n.parse().ok()) {
        Some(n) => (1, n),
        None => (0, 0),
    }
}

/// 끝내는 중(⌘Q)에는 탭 목록을 적지 않는다 — 탭을 하나씩 닫으므로 적으면 목록이 비어 버린다.
pub fn should_record_tabs(quitting: bool) -> bool {
    !quitting
}

fn file(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("recent.json"))
}

pub fn read_from(path: &Path) -> Recent {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn write_to(path: &Path, recent: &Recent) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(recent) {
        let _ = std::fs::write(path, bytes);
    }
}

pub fn read(app: &AppHandle) -> Recent {
    file(app).map(|path| read_from(&path)).unwrap_or_default()
}

pub fn write(app: &AppHandle, recent: &Recent) {
    if let Some(path) = file(app) {
        write_to(&path, recent);
    }
}

/// 열기 · 다른 이름 저장(`opened`) · 탭 닫기 · 문서 닫기 뒤: 최근 목록 · 탭 목록을 적고 메뉴를 다시 채운다.
pub fn changed(app: &AppHandle, opened: Option<&Path>) {
    let state = app.state::<crate::editor::EditorState>();
    let _guard = FILE_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let mut recent = read(app);
    if let Some(path) = opened {
        recent.recent = push_recent(&recent.recent, path);
    }
    if should_record_tabs(state.quitting.load(std::sync::atomic::Ordering::SeqCst)) {
        recent.tabs = state.open_sources();
    }
    write(app, &recent);
    if opened.is_some() {
        fill_menu(app, &recent.recent);
    }
}

/// 「목록 지우기」 · 없는 파일 빼기.
pub fn set_recent(app: &AppHandle, list: Vec<PathBuf>) {
    let _guard = FILE_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let mut recent = read(app);
    recent.recent = list;
    write(app, &recent);
    fill_menu(app, &recent.recent);
}

/// 메뉴 「최근 문서」 항목을 목록대로 다시 채운다(`recent:N` · 끝에 「목록 지우기」).
pub fn fill_menu(app: &AppHandle, list: &[PathBuf]) {
    use tauri::menu::{MenuItem, MenuItemKind, PredefinedMenuItem};
    let Some(MenuItemKind::Submenu(submenu)) = app.menu().and_then(|menu| menu.get("recent"))
    else {
        return;
    };
    for item in submenu.items().unwrap_or_default() {
        let _ = submenu.remove(&item);
    }
    if list.is_empty() {
        if let Ok(item) = MenuItem::with_id(app, "recent-none", "(없음)", false, None::<&str>) {
            let _ = submenu.append(&item);
        }
        return;
    }
    for (index, path) in list.iter().enumerate() {
        if let Ok(item) = MenuItem::with_id(
            app,
            format!("recent:{index}"),
            label(path),
            true,
            None::<&str>,
        ) {
            let _ = submenu.append(&item);
        }
    }
    if let Ok(separator) = PredefinedMenuItem::separator(app) {
        let _ = submenu.append(&separator);
    }
    if let Ok(item) = MenuItem::with_id(app, "recent-clear", "목록 지우기", true, None::<&str>)
    {
        let _ = submenu.append(&item);
    }
}

/// 「이름 — 상위 폴더」(메뉴엔 툴팁이 없다 — 같은 이름 파일을 폴더로 가른다).
fn label(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    match path
        .parent()
        .and_then(|dir| dir.file_name())
        .map(|dir| dir.to_string_lossy().into_owned())
    {
        Some(dir) => format!("{name} — {dir}"),
        None => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pushes_to_front_without_duplicates_and_keeps_ten() {
        let p = |n: usize| PathBuf::from(format!("/d/{n}.HLT"));
        let list: Vec<PathBuf> = (0..10).map(p).collect();
        let next = push_recent(&list, &p(5));
        assert_eq!(next.len(), 10);
        assert_eq!(next[0], p(5));
        assert_eq!(next.iter().filter(|item| **item == p(5)).count(), 1);
        assert_eq!(next[1..6], [p(0), p(1), p(2), p(3), p(4)]);
        let next = push_recent(&list, &p(99));
        assert_eq!(next.len(), 10);
        assert_eq!(next[0], p(99));
        assert!(!next.contains(&p(9)));
    }

    #[test]
    fn tabs_come_main_first_then_by_number() {
        let mut labels = vec!["doc-10", "main", "doc-2", "doc-1"];
        labels.sort_by_key(|label| tab_order(label));
        assert_eq!(labels, ["main", "doc-1", "doc-2", "doc-10"]);
    }

    #[test]
    fn does_not_record_tabs_while_quitting() {
        assert!(should_record_tabs(false));
        assert!(!should_record_tabs(true));
    }

    #[test]
    fn round_trips_and_reads_broken_files_as_empty() {
        let dir = std::env::temp_dir().join(format!("seoan-recent-{}", std::process::id()));
        let path = dir.join("sub").join("recent.json");
        let recent = Recent {
            recent: vec![PathBuf::from("/a/명세서.HLT")],
            tabs: vec![PathBuf::from("/a/명세서.HLT"), PathBuf::from("/b/x.fin")],
        };
        write_to(&path, &recent);
        assert_eq!(read_from(&path), recent);
        std::fs::write(&path, b"{not json").unwrap();
        assert_eq!(read_from(&path), Recent::default());
        assert_eq!(read_from(&dir.join("none.json")), Recent::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
