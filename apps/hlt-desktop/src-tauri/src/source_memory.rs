//! 문서마다 기억한 원문: 앱 자료 폴더 `source-memory.json`.
//! `{ "<역문 정규 경로>": { source, fingerprint(원문 SHA-256 앞 16자), pins } }` — 같은 문서를 다시 열면 원문 · 손 고정이 돌아온다.
//! 원문 파일이 바뀌었으면(지문이 다르면) 손 고정은 버리고 경로만 남긴다. 깨진 파일은 `.bad` 로 옮기고 빈 것으로 읽는다.

use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

use hlt_core::Pin;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 읽고 고쳐 쓰기를 한 번에 하나씩(탭 여럿이 함께 쓴다).
static FILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
struct Entry {
    source: PathBuf,
    fingerprint: String,
    #[serde(default)]
    pins: Vec<Pin>,
}

type Map = BTreeMap<String, Entry>;

/// 기억해 둔 원문: 경로 · (지문이 맞을 때만) 손 고정.
#[derive(Debug, PartialEq)]
pub struct Recalled {
    pub source: PathBuf,
    pub pins: Vec<Pin>,
}

/// 원문 파일 SHA-256 의 앞 16자(소문자 16진).
pub fn fingerprint(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize())[..16].to_string())
}

fn key(doc: &Path) -> String {
    std::fs::canonicalize(doc)
        .unwrap_or_else(|_| doc.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn read_map(file: &Path) -> Map {
    let Ok(bytes) = std::fs::read(file) else {
        return Map::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_else(|_| {
        let mut bad = file.as_os_str().to_owned();
        bad.push(".bad");
        let _ = std::fs::rename(file, bad);
        Map::new()
    })
}

/// 옆 임시 파일에 쓴 뒤 바꿔 끼운다(쓰다 끊겨도 옛 파일이 남는다).
fn write_map(file: &Path, map: &Map) {
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Ok(bytes) = serde_json::to_vec_pretty(map) else {
        return;
    };
    let mut tmp = file.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, file);
    }
}

fn locked<T>(run: impl FnOnce() -> T) -> T {
    let _guard = FILE_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    run()
}

/// 이 문서(`doc`)의 원문 · 손 고정을 적는다.
pub fn remember(file: &Path, doc: &Path, source: &Path, fingerprint: &str, pins: &[Pin]) {
    locked(|| {
        let mut map = read_map(file);
        map.insert(
            key(doc),
            Entry {
                source: source.to_path_buf(),
                fingerprint: fingerprint.to_string(),
                pins: pins.to_vec(),
            },
        );
        write_map(file, &map);
    });
}

/// 기억한 원문 — 파일이 아직 있을 때만. 지문이 달라졌으면 손 고정은 비운다(경로는 남긴다).
pub fn recall(file: &Path, doc: &Path) -> Option<Recalled> {
    let entry = locked(|| read_map(file)).remove(&key(doc))?;
    if !entry.source.is_file() {
        return None;
    }
    let same = fingerprint(&entry.source).is_ok_and(|now| now == entry.fingerprint);
    Some(Recalled {
        pins: if same { entry.pins } else { Vec::new() },
        source: entry.source,
    })
}

/// 「다른 이름으로 저장」: 기억을 새 경로 열쇠로 옮긴다.
pub fn move_key(file: &Path, from: &Path, to: &Path) {
    locked(|| {
        let mut map = read_map(file);
        if let Some(entry) = map.remove(&key(from)) {
            map.insert(key(to), entry);
            write_map(file, &map);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("seoan-mem-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn pin(n: u32) -> Pin {
        Pin {
            target_id: "p1".into(),
            target_sentence: n,
            source_paragraph: 2,
            source_sentence: n,
        }
    }
    /// 역문 · 원문 파일을 만들고 (기억 파일, 역문, 원문)을 준다.
    fn fixture(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let d = dir(name);
        let (doc, src) = (d.join("역문.HLT"), d.join("원문.xlf"));
        std::fs::write(&doc, "x").unwrap();
        std::fs::write(&src, "원문 하나").unwrap();
        (d.join("source-memory.json"), doc, src)
    }

    #[test]
    fn round_trip() {
        let (file, doc, src) = fixture("rt");
        assert_eq!(recall(&file, &doc), None);
        let fp = fingerprint(&src).unwrap();
        assert_eq!(fp.len(), 16);
        remember(&file, &doc, &src, &fp, &[pin(0), pin(1)]);
        assert_eq!(
            recall(&file, &doc),
            Some(Recalled {
                source: src,
                pins: vec![pin(0), pin(1)]
            })
        );
    }

    #[test]
    fn corrupt_json_is_empty_and_moved_aside() {
        let (file, doc, src) = fixture("bad");
        std::fs::write(&file, "{ 깨짐").unwrap();
        assert_eq!(recall(&file, &doc), None);
        assert!(file.with_extension("json.bad").exists());
        assert!(!file.exists());
        // 덮어쓰기도 된다.
        remember(&file, &doc, &src, &fingerprint(&src).unwrap(), &[pin(0)]);
        assert!(recall(&file, &doc).is_some());
    }

    #[test]
    fn changed_source_drops_pins_but_keeps_the_path() {
        let (file, doc, src) = fixture("fp");
        remember(&file, &doc, &src, &fingerprint(&src).unwrap(), &[pin(0)]);
        std::fs::write(&src, "원문이 달라졌다").unwrap();
        assert_eq!(
            recall(&file, &doc),
            Some(Recalled {
                source: src.clone(),
                pins: Vec::new()
            })
        );
        // 원문이 사라지면 기억하지 않는다.
        std::fs::remove_file(&src).unwrap();
        assert_eq!(recall(&file, &doc), None);
    }

    #[test]
    fn save_as_moves_the_key() {
        let (file, doc, src) = fixture("mv");
        remember(&file, &doc, &src, &fingerprint(&src).unwrap(), &[pin(3)]);
        let other = doc.with_file_name("새 이름.HLT");
        std::fs::write(&other, "y").unwrap();
        move_key(&file, &doc, &other);
        assert_eq!(recall(&file, &doc), None);
        assert_eq!(recall(&file, &other).unwrap().pins, vec![pin(3)]);
    }
}
