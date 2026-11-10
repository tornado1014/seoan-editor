//! 원문 창(문장 단위 나란히 보기)의 Rust 쪽: 창마다 든 원문 상태 · 명령 · 화면에 보내는 모양.
//! 원문은 XLIFF · 워드 · PDF. 정렬은 hlt-core(`align_xliff` · `align_inferred`)가 하고, 여기는 읽기 · 손 고정 · 기억만 맡는다.
//! 오프셋: hlt-core 의 문장 범위는 글자(유니코드 스칼라) 단위지만 화면(JS 문자열)은 UTF-16 이라,
//! 화면에 보내는 `AlignmentDto` 의 범위는 이미 UTF-16 코드 단위로 바꿔 있다(손 고정은 문장 번호라 그대로).

use std::path::{Path, PathBuf};

use hlt_core::{
    Alignment, CoreError, EditorParagraph, ErrorCode, Pair, Pin, Segment, SourcePara,
    align_inferred, align_xliff,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, State, Window};
use tauri_plugin_dialog::DialogExt;

use crate::editor::{EditorState, Slot, same_file};
use crate::source_memory;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Xliff,
    Docx,
    Pdf,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Xliff => "xliff",
            Kind::Docx => "docx",
            Kind::Pdf => "pdf",
        }
    }
}

/// 확장자 → 종류. 모르는 확장자는 한국어로 이유를 단 오류.
pub fn kind_of(path: &Path) -> Result<Kind, CoreError> {
    let ext = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "xlf" | "xliff" | "txlf" | "mxliff" => Ok(Kind::Xliff),
        "docx" => Ok(Kind::Docx),
        "pdf" => Ok(Kind::Pdf),
        _ => {
            let mut error = CoreError::new(ErrorCode::UnsupportedFormat, "source_kind", false);
            error.message = "원문은 .docx · .pdf · XLIFF(.xlf · .xliff · .txlf · .mxliff) 파일만 쓸 수 있습니다.".into();
            Err(error)
        }
    }
}

enum Material {
    Segments(Vec<Segment>),
    Texts(Vec<String>),
}

/// 창이 든 원문(문서 세션 밖 — 문서를 닫거나 바꾸면 비운다).
pub struct SourceState {
    path: PathBuf,
    kind: Kind,
    /// 원문 파일 지문(SHA-256 앞 16자) — 기억에 같이 적는다.
    fingerprint: String,
    material: Material,
    pins: Vec<Pin>,
    pages: Option<(u32, u32)>,
}

/// 화면에 보내는 정렬 — `hlt_core::Alignment` 와 같은 모양이되 문장 범위가 UTF-16 코드 단위.
#[derive(Serialize, Debug, PartialEq)]
pub struct AlignmentDto {
    pub source: Vec<SourcePara>,
    pub target_sentences: Vec<(String, Vec<(u32, u32)>)>,
    pub pairs: Vec<Pair>,
    pub dropped_pins: Vec<Pin>,
}

#[derive(Serialize)]
pub struct SourceDto {
    pub name: String,
    /// `xliff` · `docx` · `pdf`.
    pub kind: String,
    pub alignment: AlignmentDto,
    pub pins: usize,
    /// PDF 만: 쪽 수 · 글을 읽은 쪽 수.
    pub pages: Option<u32>,
    pub pages_with_text: Option<u32>,
}

/// 글자 범위 → UTF-16 범위(글자 번호마다 UTF-16 누적 길이를 한 번 센다).
fn utf16_ranges(text: &str, ranges: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut at = Vec::with_capacity(text.chars().count() + 1);
    let mut sum = 0u32;
    at.push(0);
    for c in text.chars() {
        sum += c.len_utf16() as u32;
        at.push(sum);
    }
    let last = at.len() - 1;
    ranges
        .iter()
        .map(|&(s, e)| (at[(s as usize).min(last)], at[(e as usize).min(last)]))
        .collect()
}

fn to_dto(alignment: Alignment, target: &[EditorParagraph]) -> AlignmentDto {
    let source = alignment
        .source
        .into_iter()
        .map(|mut para| {
            para.sentences = utf16_ranges(&para.text, &para.sentences);
            para
        })
        .collect();
    let target_sentences = alignment
        .target_sentences
        .into_iter()
        .map(|(id, ranges)| {
            let text = target
                .iter()
                .find(|p| p.id == id)
                .map_or("", |p| p.text.as_str());
            let ranges = utf16_ranges(text, &ranges);
            (id, ranges)
        })
        .collect();
    AlignmentDto {
        source,
        target_sentences,
        pairs: alignment.pairs,
        dropped_pins: alignment.dropped_pins,
    }
}

/// 원문 파일을 읽는다 — 막는 일이라 `spawn_blocking` 안에서만 부른다.
fn read_source(path: &Path) -> Result<SourceState, CoreError> {
    let kind = kind_of(path)?;
    let fingerprint = source_memory::fingerprint(path).map_err(|e| {
        let mut error = CoreError::new(ErrorCode::IoError, "source_read", false);
        error.message = format!("원문 파일을 읽지 못했습니다: {e}");
        error.source = Some(path.to_string_lossy().into_owned());
        error
    })?;
    let (material, pages) = match kind {
        Kind::Xliff => (Material::Segments(hlt_core::read_xliff(path)?), None),
        Kind::Docx => (
            Material::Texts(
                hlt_core::inspect_docx_path(path)?
                    .document
                    .paragraphs
                    .iter()
                    .map(|p| p.text.clone())
                    .collect(),
            ),
            None,
        ),
        Kind::Pdf => {
            let text = hlt_core::pdf_text(path)?;
            (
                Material::Texts(text.paragraphs),
                Some((text.pages, text.pages_with_text)),
            )
        }
    };
    Ok(SourceState {
        path: path.to_path_buf(),
        kind,
        fingerprint,
        material,
        pins: Vec::new(),
        pages,
    })
}

fn align(state: &SourceState, target: &[EditorParagraph]) -> AlignmentDto {
    let alignment = match &state.material {
        Material::Segments(segments) => align_xliff(target, segments, &state.pins),
        Material::Texts(texts) => align_inferred(target, texts, &state.pins),
    };
    to_dto(alignment, target)
}

fn slot_error(phase: &str) -> CoreError {
    CoreError::new(ErrorCode::VerificationFailed, phase, false)
}

/// 지금 문서 문단으로 다시 정렬. 원문이 없으면 `None`, 문서가 없으면 오류.
fn alignment_of(slot: &Slot) -> Result<Option<AlignmentDto>, CoreError> {
    let Some(held) = &slot.held else {
        return Ok(None);
    };
    let session = slot
        .session
        .as_ref()
        .ok_or_else(|| CoreError::new(ErrorCode::PathNotAllowed, "editor_closed", false))?;
    Ok(Some(align(held, &session.paragraphs())))
}

pub(crate) fn memory_file(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join("source-memory.json"))
}

/// 경로 있는 문서만 디스크에 기억한다(경로 없는 문서 = 메모리에만).
fn persist(app: &AppHandle, slot: &Slot) {
    if let Some(file) = memory_file(app) {
        persist_to(&file, slot);
    }
}

fn persist_to(file: &Path, slot: &Slot) {
    if let (Some(held), Some(doc)) = (&slot.held, &slot.source) {
        source_memory::remember(file, doc, &held.path, &held.fingerprint, &held.pins);
    }
}

fn with_slot<T>(
    state: &State<'_, EditorState>,
    window: &Window,
    run: impl FnOnce(&mut Slot) -> Result<T, CoreError>,
) -> Result<T, CoreError> {
    let slot = state.slot(window.label());
    let mut guard = slot.lock().map_err(|_| slot_error("editor_lock"))?;
    run(&mut guard)
}

async fn load(
    window: &Window,
    app: &AppHandle,
    state: &State<'_, EditorState>,
    path: PathBuf,
) -> Result<SourceDto, CoreError> {
    kind_of(&path)?;
    // 이 문서에 같은 원문으로 기억해 둔 손 고정(지문이 맞을 때만 돌아온다).
    let (file, doc) = (
        memory_file(app),
        with_slot(state, window, |slot| Ok(slot.source.clone()))?,
    );
    let wanted = path.clone();
    let held = tauri::async_runtime::spawn_blocking(move || {
        let mut held = read_source(&wanted)?;
        if let (Some(file), Some(doc)) = (file, doc)
            && let Some(recalled) = source_memory::recall(&file, &doc)
            && same_file(&recalled.source, &wanted)
        {
            held.pins = recalled.pins;
        }
        Ok::<_, CoreError>(held)
    })
    .await
    .map_err(|_| slot_error("worker"))??;
    with_slot(state, window, |slot| {
        let session = slot
            .session
            .as_ref()
            .ok_or_else(|| CoreError::new(ErrorCode::PathNotAllowed, "editor_closed", false))?;
        let target = session.paragraphs();
        let alignment = align(&held, &target);
        let dto = SourceDto {
            name: held
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            kind: held.kind.name().into(),
            alignment,
            pins: held.pins.len(),
            pages: held.pages.map(|p| p.0),
            pages_with_text: held.pages.map(|p| p.1),
        };
        slot.held = Some(held);
        persist(app, slot);
        Ok(dto)
    })
}

/// 고르기 창(취소 = `null`).
#[tauri::command]
pub async fn editor_source_pick(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
) -> Result<Option<SourceDto>, CoreError> {
    let Some(path) = app
        .dialog()
        .file()
        .set_title("나란히 볼 원문")
        .add_filter("원문", &["docx", "pdf", "xlf", "xliff", "txlf", "mxliff"])
        .blocking_pick_file()
        .and_then(|picked| picked.into_path().ok())
    else {
        return Ok(None);
    };
    load(&window, &app, &state, path).await.map(Some)
}

/// 경로로 바로 연다(놓은 파일 · 기억한 원문).
#[tauri::command]
pub async fn editor_source_load(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
    path: String,
) -> Result<SourceDto, CoreError> {
    load(&window, &app, &state, PathBuf::from(path)).await
}

/// 판이 바뀔 때마다: 든 원문을 지금 문단으로 다시 정렬(원문이 없으면 `null`).
#[tauri::command]
pub async fn editor_source_align(
    window: Window,
    state: State<'_, EditorState>,
) -> Result<Option<AlignmentDto>, CoreError> {
    with_slot(&state, &window, |slot| alignment_of(slot))
}

/// 손 고정 하나 더하기(같은 역문 문장의 옛 고정은 바꾼다) → 다시 정렬 · 기억.
#[tauri::command]
pub async fn editor_source_pin(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
    pin: Pin,
) -> Result<AlignmentDto, CoreError> {
    with_slot(&state, &window, |slot| {
        let held = slot.held.as_mut().ok_or_else(no_source)?;
        held.pins.retain(|p| {
            !(p.target_id == pin.target_id && p.target_sentence == pin.target_sentence)
        });
        held.pins.push(pin);
        persist(&app, slot);
        alignment_of(slot)?.ok_or_else(no_source)
    })
}

#[tauri::command]
pub async fn editor_source_unpin_all(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
) -> Result<AlignmentDto, CoreError> {
    with_slot(&state, &window, |slot| {
        slot.held.as_mut().ok_or_else(no_source)?.pins.clear();
        persist(&app, slot);
        alignment_of(slot)?.ok_or_else(no_source)
    })
}

/// 원문 창을 닫는다(기억은 남긴다 — 다시 열면 같은 원문).
#[tauri::command]
pub async fn editor_source_close(
    window: Window,
    state: State<'_, EditorState>,
) -> Result<(), CoreError> {
    with_slot(&state, &window, |slot| {
        slot.held = None;
        Ok(())
    })
}

/// 이 문서에 기억된 원문 경로(파일이 아직 있을 때).
#[tauri::command]
pub async fn editor_source_remembered(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
) -> Result<Option<String>, CoreError> {
    let (Some(file), Some(doc)) = (
        memory_file(&app),
        with_slot(&state, &window, |slot| Ok(slot.source.clone()))?,
    ) else {
        return Ok(None);
    };
    tauri::async_runtime::spawn_blocking(move || {
        source_memory::recall(&file, &doc).map(|r| r.source.display().to_string())
    })
    .await
    .map_err(|_| slot_error("worker"))
}

fn no_source() -> CoreError {
    let mut error = CoreError::new(ErrorCode::PathNotAllowed, "source_closed", false);
    error.message = "열어 둔 원문이 없습니다. 원문을 먼저 열어 주세요.".into();
    error
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(id: &str, text: &str) -> EditorParagraph {
        EditorParagraph {
            id: id.into(),
            text: text.into(),
            wipo_id: None,
            number: None,
            has_objects: false,
            spans: Vec::new(),
        }
    }
    fn texts(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }
    fn held(items: &[&str], pins: Vec<Pin>) -> SourceState {
        SourceState {
            path: PathBuf::from("원문.docx"),
            kind: Kind::Docx,
            fingerprint: "0".repeat(16),
            material: Material::Texts(texts(items)),
            pins,
            pages: None,
        }
    }

    #[test]
    fn extension_decides_the_kind() {
        for (name, kind) in [
            ("a.xlf", Kind::Xliff),
            ("a.XLIFF", Kind::Xliff),
            ("a.txlf", Kind::Xliff),
            ("a.mxliff", Kind::Xliff),
            ("a.docx", Kind::Docx),
            ("a.PDF", Kind::Pdf),
        ] {
            assert_eq!(kind_of(Path::new(name)).unwrap(), kind, "{name}");
        }
        let error = kind_of(Path::new("a.hwp")).unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsupportedFormat);
        assert!(error.message.contains("파일만 쓸 수 있습니다"));
        assert!(kind_of(Path::new("noext")).is_err());
    }

    #[test]
    fn reads_an_xliff_source_into_segments() {
        let dir = std::env::temp_dir().join(format!("seoan-src-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.xlf");
        std::fs::write(&path, "<?xml version=\"1.0\"?><xliff version=\"1.2\"><file original=\"a\" source-language=\"en\" target-language=\"ko\" datatype=\"x\"><body><trans-unit id=\"1\"><source>One.</source><target>하나.</target></trans-unit></body></file></xliff>").unwrap();
        let state = read_source(&path).unwrap();
        assert_eq!(state.kind, Kind::Xliff);
        assert!(matches!(&state.material, Material::Segments(s) if s.len() == 1));
        assert_eq!(state.fingerprint.len(), 16);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 별 평면 글자(𝛼 · 이모지)는 글자 1 · UTF-16 2 — 문장 경계 앞에 있으면 JS 오프셋이 한 칸씩 밀린다.
    #[test]
    fn ranges_reach_the_screen_in_utf16_units() {
        let target = [para("t1", "𝛼 값은 크다. 둘째 문장이다.")];
        let source = texts(&["😀 Alpha is big. Second one."]);
        let dto = to_dto(align_inferred(&target, &source, &[]), &target);
        // 글자 기준: 첫 문장 [0, 8) 둘째 [9, 17) → UTF-16: 𝛼 가 두 칸이라 +1.
        let chars: Vec<char> = "𝛼 값은 크다. 둘째 문장이다.".chars().collect();
        let first_end = chars.iter().position(|&c| c == '.').unwrap() as u32 + 1;
        assert_eq!(dto.target_sentences[0].1[0], (0, first_end + 1));
        let second = dto.target_sentences[0].1[1];
        let units: Vec<u16> = "𝛼 값은 크다. 둘째 문장이다.".encode_utf16().collect();
        assert_eq!(
            String::from_utf16(&units[second.0 as usize..second.1 as usize]).unwrap(),
            "둘째 문장이다."
        );
        let s = &dto.source[0];
        let units: Vec<u16> = s.text.encode_utf16().collect();
        assert_eq!(
            String::from_utf16(&units[s.sentences[1].0 as usize..s.sentences[1].1 as usize])
                .unwrap(),
            "Second one."
        );
    }

    #[test]
    fn realign_after_an_edit_keeps_pins_and_untouched_pairs() {
        let source = ["First is here. Then comes more.", "Another para. With two."];
        let mut target = vec![
            para("t1", "첫째가 여기 있다. 그다음이 더 있다."),
            para("t2", "다른 문단이다. 둘이 있다."),
        ];
        let pin = Pin {
            target_id: "t1".into(),
            target_sentence: 1,
            source_paragraph: 0,
            source_sentence: 1,
        };
        let state = held(&source, vec![pin.clone()]);
        let before = align(&state, &target);
        assert!(before.dropped_pins.is_empty());
        assert!(before.pairs.iter().any(|p| p.target_id == "t1"
            && p.target_sentence == Some(1)
            && p.source_sentence == Some(1)));
        // 둘째 문단만 고친다.
        target[1].text = "다른 문단이다. 고쳐서 둘이 있다.".into();
        let after = align(&state, &target);
        assert!(after.dropped_pins.is_empty());
        let of = |a: &AlignmentDto, id: &str| {
            a.pairs
                .iter()
                .filter(|p| p.target_id == id)
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(of(&before, "t1"), of(&after, "t1"));
        assert!(!of(&after, "t2").is_empty());
    }

    #[test]
    fn replacing_the_document_clears_the_held_source() {
        let mut slot = Slot::default();
        slot.held = Some(held(&["A."], Vec::new()));
        slot.swap_session(None);
        assert!(slot.held.is_none() && slot.session.is_none());
    }

    #[test]
    fn a_document_without_a_path_is_remembered_in_memory_only() {
        let dir = std::env::temp_dir().join(format!("seoan-nopath-{}", std::process::id()));
        let file = dir.join("source-memory.json");
        let mut slot = Slot::default();
        slot.held = Some(held(&["A."], Vec::new()));
        persist_to(&file, &slot);
        assert!(!file.exists());
        let doc = dir.join("역문.HLT");
        std::fs::create_dir_all(&dir).unwrap();
        slot.source = Some(doc.clone());
        persist_to(&file, &slot);
        assert!(file.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
