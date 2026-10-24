//! 설정: 앱 설정 폴더 `settings.json` — 자동 저장 간격 · 이전 판 보관 수 · 보관 날수 · 문서 배율 · 문단부호 기본 · 상용구 · 판 기록.
//! 옛 파일에 없는 칸은 기본값으로 읽는다. 바꾸면 모든 창에 `settings-changed` 를 보낸다(탭마다 배율 · 간격을 맞춘다).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

static FILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Snippet {
    pub name: String,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    pub autosave_minutes: u32,
    pub history_keep: usize,
    /// 이 날수가 지난 이전 판은 지운다.
    pub history_days: u32,
    pub zoom: f64,
    pub marks_default: bool,
    pub snippets: Vec<Snippet>,
    /// 지난 실행의 판 · 그 앞 판(「이전 판으로 되돌리기」의 대상).
    pub last_version: Option<String>,
    pub previous_version: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        let snippet = |name: &str, text: &str| Snippet {
            name: name.into(),
            text: text.into(),
        };
        Self {
            autosave_minutes: 2,
            history_keep: 10,
            history_days: 30,
            zoom: 1.0,
            marks_default: true,
            snippets: vec![
                snippet(
                    "의견서 첫머리",
                    "출원인은 심사관님의 의견제출통지서에 기재된 거절이유를 검토한 결과, 다음과 같이 의견을 제출합니다.",
                ),
                snippet(
                    "보정 근거",
                    "본 보정은 최초 명세서에 기재된 사항의 범위 내에서 이루어진 것으로, 신규사항을 추가하는 것이 아닙니다.",
                ),
                snippet(
                    "맺음말",
                    "이상과 같이 거절이유가 모두 해소되었으므로, 본원 발명에 대하여 특허결정하여 주시기 바랍니다.",
                ),
            ],
            last_version: None,
            previous_version: None,
        }
    }
}

impl Settings {
    /// 화면이 보낸 값을 쓸 수 있는 범위로(배율 50~200% · 간격 1~60분 · 보관 1~100개).
    pub fn clamped(mut self) -> Self {
        self.zoom = if self.zoom.is_finite() {
            self.zoom.clamp(0.5, 2.0)
        } else {
            1.0
        };
        self.autosave_minutes = self.autosave_minutes.clamp(1, 60);
        self.history_keep = self.history_keep.clamp(1, 100);
        self.history_days = self.history_days.clamp(1, 365);
        self
    }

    /// 이번 실행 판을 적는다 — 판이 바뀌었으면 지난 판을 「이전 판」으로 남긴다. 바뀌었으면 `true`.
    pub fn record_version(&mut self, current: &str) -> bool {
        if self.last_version.as_deref() == Some(current) {
            return false;
        }
        if let Some(last) = self.last_version.take() {
            self.previous_version = Some(last);
        }
        self.last_version = Some(current.to_string());
        true
    }
}

fn file(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("settings.json"))
}

pub fn read_from(path: &Path) -> Settings {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
        .unwrap_or_default()
        .clamped()
}

/// 옆 임시 파일에 다 쓴 뒤 바꿔 끼운다(쓰다 끊겨도 앞 설정이 남는다).
pub fn write_to(path: &Path, settings: &Settings) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, serde_json::to_vec_pretty(settings)?)?;
    std::fs::rename(&temporary, path)
}

pub fn read(app: &AppHandle) -> Settings {
    file(app).map(|path| read_from(&path)).unwrap_or_default()
}

/// 읽고 고쳐 쓰기를 한 번에 하나씩.
pub fn update(app: &AppHandle, change: impl FnOnce(&mut Settings)) -> std::io::Result<Settings> {
    let _guard = FILE_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let path = file(app).ok_or_else(|| std::io::Error::other("no config dir"))?;
    let mut settings = read_from(&path);
    change(&mut settings);
    let settings = settings.clamped();
    write_to(&path, &settings)?;
    Ok(settings)
}

#[tauri::command]
pub fn settings_get(app: AppHandle) -> Settings {
    read(&app)
}

/// 화면이 바꾼 설정(판 기록 칸은 화면이 못 바꾼다) → 저장 · 모든 창에 알림.
#[tauri::command]
pub fn settings_set(app: AppHandle, settings: Settings) -> Result<Settings, hlt_core::CoreError> {
    let saved = update(&app, |current| {
        let versions = (current.last_version.take(), current.previous_version.take());
        *current = settings;
        (current.last_version, current.previous_version) = versions;
    })
    .map_err(|_| hlt_core::CoreError::new(hlt_core::ErrorCode::IoError, "settings", false))?;
    let _ = app.emit("settings-changed", &saved);
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("seoan-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn old_file_without_new_fields_reads_defaults() {
        let dir = scratch("old");
        let path = dir.join("settings.json");
        std::fs::write(&path, br#"{"zoom": 1.3}"#).unwrap();
        let settings = read_from(&path);
        assert_eq!(settings.zoom, 1.3);
        assert_eq!((settings.history_keep, settings.history_days), (10, 30));
        assert_eq!(settings.snippets.len(), 3);
    }

    #[test]
    fn round_trip_and_clamp() {
        let dir = scratch("trip");
        let path = dir.join("sub/settings.json");
        let settings = Settings {
            zoom: 9.0,
            autosave_minutes: 0,
            history_keep: 0,
            history_days: 0,
            ..Settings::default()
        }
        .clamped();
        write_to(&path, &settings).unwrap();
        let back = read_from(&path);
        assert_eq!(back, settings);
        assert_eq!(
            (
                back.zoom,
                back.autosave_minutes,
                back.history_keep,
                back.history_days
            ),
            (2.0, 1, 1, 1)
        );
        // 0일이면 저장 때마다 이전 판을 다 지운다 — 1일 아래로 내려가지 않는다.
        let long = Settings {
            history_days: 9999,
            ..Settings::default()
        }
        .clamped();
        assert_eq!(long.history_days, 365);
        assert_eq!(read_from(&dir.join("none.json")), Settings::default());
    }

    #[test]
    fn version_change_keeps_the_one_before() {
        let mut settings = Settings::default();
        assert!(settings.record_version("0.6.30"));
        assert_eq!(settings.previous_version, None);
        assert!(!settings.record_version("0.6.30"));
        assert!(settings.record_version("0.6.31"));
        assert_eq!(settings.previous_version.as_deref(), Some("0.6.30"));
        assert_eq!(settings.last_version.as_deref(), Some("0.6.31"));
    }
}
