//! 이전 판 보관: 저장이 있는 파일을 덮기 직전에 그 파일을 앱 자료 폴더 `history/<이름>-<경로 표>/<시각>.HLT` 로 복사한다.
//! 파일마다 최근 N개(설정 `history_keep`)만, 그리고 `history_days`(기본 30일)가 지난 판은 지운다. 「파일 › 이전 판…」이 목록을 보이고, 고른 판은 새 탭에 이름 없는 문서로 연다.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Debug, PartialEq)]
pub struct Kept {
    pub path: PathBuf,
    /// `YYYY-MM-DD HH:MM:SS`.
    pub saved: String,
    pub size: u64,
}

pub fn root(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join("history"))
}

/// 경로 표(FNV-1a 64 → 16진 8자): 이름이 같은 다른 폴더의 파일과 섞이지 않게. 실행 · 판이 바뀌어도 같은 값이어야 한다
/// (`DefaultHasher` 는 판마다 달라질 수 있다).
fn path_tag(path: &Path) -> String {
    let hash = path
        .to_string_lossy()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    format!("{:08x}", hash >> 32)
}

/// 이 파일의 보관 폴더.
pub fn folder(root: &Path, path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let stem = absolute
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    root.join(format!("{stem}-{}", path_tag(&absolute)))
}

/// 덮기 직전: `path` 가 있으면 보관하고 `keep` 개를 넘는 오래된 판을 지운다. 없는 파일이면 아무것도 안 한다.
pub fn keep(root: &Path, path: &Path, keep: usize, stamp: &str) -> std::io::Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let folder = folder(root, path);
    std::fs::create_dir_all(&folder)?;
    std::fs::write(folder.join("source.txt"), path.to_string_lossy().as_bytes())?;
    // 같은 초에 두 번 저장해도 덮지 않는다 — 번호를 늘 붙여 글자 차례 = 시간 차례를 지킨다.
    let target = (0..)
        .map(|n| folder.join(format!("{stamp}-{n:03}.HLT")))
        .find(|candidate| !candidate.exists())
        .expect("unbounded");
    std::fs::copy(path, target)?;
    for old in versions(&folder).into_iter().skip(keep.max(1)) {
        let _ = std::fs::remove_file(old);
    }
    Ok(())
}

/// `days` 일이 지난 판을 모든 파일 폴더에서 지운다(이름의 시각으로 잰다 · 빈 폴더도 치운다). 앱을 켤 때와 저장할 때 부른다.
pub fn prune_older_than(root: &Path, days: u32, now: chrono::NaiveDateTime) {
    let cutoff = now - chrono::Duration::days(i64::from(days));
    for folder in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let folder = folder.path();
        if !folder.is_dir() {
            continue;
        }
        for kept in versions(&folder) {
            let saved = kept
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.get(0..15))
                .and_then(|stamp| {
                    chrono::NaiveDateTime::parse_from_str(stamp, "%Y%m%d-%H%M%S").ok()
                });
            if saved.is_some_and(|saved| saved < cutoff) {
                let _ = std::fs::remove_file(kept);
            }
        }
        if versions(&folder).is_empty() {
            let _ = std::fs::remove_file(folder.join("source.txt"));
            let _ = std::fs::remove_dir(&folder);
        }
    }
}

/// 보관한 판(새것부터). 이름 = 시각이라 글자 차례가 시간 차례다.
fn versions(folder: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "HLT"))
        .collect();
    found.sort();
    found.reverse();
    found
}

/// 이 파일의 보관 판 목록(새것부터).
pub fn list(root: &Path, path: &Path) -> Vec<Kept> {
    versions(&folder(root, path))
        .into_iter()
        .map(|kept| {
            let stem = kept
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            Kept {
                saved: readable(&stem),
                size: std::fs::metadata(&kept).map(|meta| meta.len()).unwrap_or(0),
                path: kept,
            }
        })
        .collect()
}

/// `20261024-153012-000` → `2026-10-24 15:30:12`.
fn readable(stem: &str) -> String {
    let digits: String = stem.chars().take(15).collect();
    match (
        digits.get(0..4),
        digits.get(4..6),
        digits.get(6..8),
        digits.get(9..11),
        digits.get(11..13),
        digits.get(13..15),
    ) {
        (Some(y), Some(mo), Some(d), Some(h), Some(mi), Some(s)) => {
            format!("{y}-{mo}-{d} {h}:{mi}:{s}")
        }
        _ => stem.to_string(),
    }
}

pub fn stamp_now() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("seoan-history-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn keeps_newest_n_and_lists_newest_first() {
        let dir = scratch("keep");
        let file = dir.join("명세서.HLT");
        let root = dir.join("history");
        for (n, stamp) in [
            "20261024-100000",
            "20261024-110000",
            "20261024-120000",
            "20261024-130000",
        ]
        .iter()
        .enumerate()
        {
            std::fs::write(&file, vec![b'x'; n + 1]).unwrap();
            keep(&root, &file, 3, stamp).unwrap();
        }
        let kept = list(&root, &file);
        assert_eq!(kept.len(), 3);
        assert_eq!(kept[0].saved, "2026-10-24 13:00:00");
        assert_eq!(kept[0].size, 4);
        assert_eq!(kept[2].saved, "2026-10-24 11:00:00");
        assert_eq!(
            std::fs::read_to_string(folder(&root, &file).join("source.txt")).unwrap(),
            file.to_string_lossy()
        );
    }

    #[test]
    fn same_second_does_not_overwrite_and_missing_file_is_skipped() {
        let dir = scratch("same");
        let file = dir.join("a.HLT");
        let root = dir.join("history");
        keep(&root, &file, 5, "20261024-100000").unwrap();
        assert!(list(&root, &file).is_empty());
        std::fs::write(&file, b"1").unwrap();
        keep(&root, &file, 5, "20261024-100000").unwrap();
        std::fs::write(&file, b"22").unwrap();
        keep(&root, &file, 5, "20261024-100000").unwrap();
        let kept = list(&root, &file);
        assert_eq!(
            kept.iter().map(|kept| kept.size).collect::<Vec<_>>(),
            [2, 1]
        );
    }

    #[test]
    fn versions_older_than_the_limit_are_pruned_everywhere() {
        let dir = scratch("prune");
        let root = dir.join("history");
        let (a, b) = (dir.join("a.HLT"), dir.join("b.HLT"));
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        keep(&root, &a, 10, "20260901-120000").unwrap();
        keep(&root, &a, 10, "20261001-120000").unwrap();
        keep(&root, &b, 10, "20260920-120000").unwrap();
        let now =
            chrono::NaiveDateTime::parse_from_str("20261024-120000", "%Y%m%d-%H%M%S").unwrap();
        prune_older_than(&root, 30, now);
        assert_eq!(list(&root, &a).len(), 1);
        assert_eq!(list(&root, &a)[0].saved, "2026-10-01 12:00:00");
        // 판이 다 지워진 파일의 폴더는 치운다.
        assert!(list(&root, &b).is_empty());
        assert!(!folder(&root, &b).exists());
        // 딱 30일 된 판은 남는다(지난 것만 지운다).
        keep(&root, &b, 10, "20260924-120000").unwrap();
        prune_older_than(&root, 30, now);
        assert_eq!(list(&root, &b).len(), 1);
    }

    #[test]
    fn same_name_in_other_folder_is_separate() {
        let dir = scratch("tag");
        let root = dir.join("history");
        assert_ne!(
            folder(&root, &dir.join("x/a.HLT")),
            folder(&root, &dir.join("y/a.HLT"))
        );
        assert_eq!(
            folder(&root, &dir.join("x/a.HLT")),
            folder(&root, &dir.join("x/a.HLT"))
        );
    }
}
