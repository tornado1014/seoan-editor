//! 편집 화면 명령(P5). 문서 상태는 `hlt_core::EditorSession` 이 들고, 여기는 대화상자와 직렬화만 한다.
//! 쪽은 화면에 보일 때 한 쪽씩 보낸다(1,288쪽 = 글자 80만 개를 한 번에 넘기지 않는다).

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use base64::Engine;
use hlt_core::{
    CharacterFormat, CoreError, EditorParagraph, EditorSession, ErrorCode, FigureKind, PlannedPage,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder, Window, ipc::Response};
use tauri_plugin_dialog::DialogExt;

/// 창(탭) 하나의 문서.
#[derive(Default)]
pub struct Slot {
    pub session: Option<EditorSession>,
    /// 연 원본 파일(제출파일 기본 이름 · 폴더).
    pub source: Option<PathBuf>,
    /// `source` 를 열거나 저장한 때의 표지 — 저장 전에 다른 곳에서 바뀌었는지 견준다.
    pub stamp: Option<Stamp>,
}

/// 파일 표지: 수정 시각 · 크기.
pub type Stamp = (std::time::SystemTime, u64);

pub fn stamp_of(path: &std::path::Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// 적어 둔 표지와 지금 파일이 다른가. 적어 둔 것이 없거나 파일이 없으면 묻지 않는다(없는 파일은 덮어도 잃을 것이 없다).
pub fn changed_since(stamp: Option<Stamp>, path: &std::path::Path) -> bool {
    match (stamp, stamp_of(path)) {
        (Some(before), Some(now)) => before != now,
        _ => false,
    }
}

/// 같은 파일인가 — 유닉스는 장치 · 아이노드(대소문자 · NFD/NFC · 심링크가 달라도), 그 밖은 정규 경로를 소문자로.
pub fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(a), Ok(b)) = (std::fs::metadata(a), std::fs::metadata(b)) {
            return a.dev() == b.dev() && a.ino() == b.ino();
        }
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase(),
        _ => a == b,
    }
}

/// 여는 방식(창에 맡겨 두면 `open_path` 가 가져간다).
#[derive(Clone, Debug)]
pub enum OpenMode {
    /// 원본 보호: 화면이 고침을 막고 ⌘S 를 거절한다.
    ReadOnly,
    /// 이전 판 · 사본: 이름 없는 문서로(저장하면 이름을 묻는다) · 탭 제목.
    Copy(String),
}

/// 창(탭)마다 문서 하나. 문서마다 따로 잠근다 — 큰 문서를 다시 굽는 동안 다른 탭이 멈추지 않게.
#[derive(Default)]
pub struct EditorState {
    windows: Mutex<HashMap<String, Arc<Mutex<Slot>>>>,
    /// 창 이름 → 열 파일(Finder 더블클릭 · 다른 탭에서 연 파일) — 그 창 화면이 준비되면 `editor_open_pending` 으로 가져간다.
    pending: Mutex<HashMap<String, PathBuf>>,
    tabs: AtomicUsize,
    /// 앱을 끝내는 중(⌘Q) — 탭을 하나씩 닫으며 저장을 묻고, 다 닫히면 끝낸다. 취소하면 멈춘다.
    pub quitting: std::sync::atomic::AtomicBool,
    /// 앱이 다 떴다(그 전에 온 Finder 파일은 첫 창 몫).
    pub ready: std::sync::atomic::AtomicBool,
    /// 창 → 확인 표를 기다리는 불러오기(CP949 밖 글자를 고른 뒤 `editor_import_finish` 가 굽는다).
    pending_import: Mutex<HashMap<String, PendingImport>>,
    /// 일괄 처리 「멈춤」(다음 파일부터 멈춘다).
    batch_stop: std::sync::atomic::AtomicBool,
    /// 창 → 여는 방식(읽기 전용 · 이전 판).
    modes: Mutex<HashMap<String, OpenMode>>,
}

/// 불러온 원본(CP949 바꾸기 전) · 굽을 자리.
struct PendingImport {
    document: hlt_core::HmlDocument,
    payloads: Vec<hlt_core::GeneratedImagePayload>,
    input: PathBuf,
    output: PathBuf,
    dialect: hlt_core::GenerationDialect,
}

/// 불러오기 결과: 연 화면, 또는 CP949 밖 글자 확인 표(이때 `editor_import_finish` 로 잇는다).
#[derive(Serialize)]
pub struct ImportDto {
    pub view: Option<EditorViewDto>,
    pub review: Vec<hlt_core::Cp949Finding>,
}

impl ImportDto {
    fn cancelled() -> Self {
        Self {
            view: None,
            review: Vec::new(),
        }
    }
}

impl EditorState {
    fn slot(&self, label: &str) -> Arc<Mutex<Slot>> {
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        windows.entry(label.to_string()).or_default().clone()
    }

    /// 문서가 열려 있는가(열기 · Finder 파일을 이 창에 둘지 새 탭에 둘지).
    pub fn has_document(&self, label: &str) -> bool {
        self.slot(label)
            .lock()
            .map(|slot| slot.session.is_some())
            .unwrap_or(true)
            || self
                .pending
                .lock()
                .map(|pending| pending.contains_key(label))
                .unwrap_or(true)
    }

    /// 문서가 열린 창들(닫을 때 저장을 물을 차례).
    pub fn open_windows(&self) -> Vec<String> {
        let windows = self
            .windows
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut labels: Vec<String> = windows
            .iter()
            .filter(|(_, slot)| {
                slot.lock()
                    .map(|slot| slot.session.is_some())
                    .unwrap_or(false)
            })
            .map(|(label, _)| label.clone())
            .collect();
        labels.sort_by_key(|label| crate::recent::tab_order(label));
        labels
    }

    /// 열린 탭의 파일(탭 차례) — 다음 실행 때 되살린다.
    pub fn open_sources(&self) -> Vec<PathBuf> {
        self.open_windows()
            .iter()
            .filter_map(|label| self.slot(label).lock().ok()?.source.clone())
            .collect()
    }

    fn is_dirty(&self, label: &str) -> bool {
        self.slot(label)
            .lock()
            .map(|slot| slot.session.as_ref().is_some_and(EditorSession::is_dirty))
            .unwrap_or(false)
    }

    /// 그 파일을 든 창(열린 문서 · 열려고 맡긴 파일). 다른 일로 잠긴 탭(긴 다시 굽기)은 건너뛴다 —
    /// ponytail: 그 탭이 같은 파일이면 놓친다. 잦아지면 창 → 파일 색인을 따로 둔다.
    pub fn window_of(&self, path: &std::path::Path) -> Option<String> {
        let slots: Vec<(String, Arc<Mutex<Slot>>)> = self
            .windows
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .iter()
            .map(|(label, slot)| (label.clone(), slot.clone()))
            .collect();
        slots
            .into_iter()
            .find(|(_, slot)| {
                slot.try_lock()
                    .ok()
                    .and_then(|slot| slot.source.clone())
                    .is_some_and(|source| same_file(&source, path))
            })
            .map(|(label, _)| label)
            .or_else(|| {
                self.pending
                    .lock()
                    .ok()?
                    .iter()
                    .find(|(_, pending)| same_file(pending, path))
                    .map(|(label, _)| label.clone())
            })
    }

    fn set_mode(&self, label: &str, mode: OpenMode) {
        if let Ok(mut modes) = self.modes.lock() {
            modes.insert(label.to_string(), mode);
        }
    }

    pub fn set_pending(&self, label: &str, path: PathBuf) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(label.to_string(), path);
        }
    }

    /// 그 창이 문서를 들었거나 열 파일이 맡겨져 있다.
    pub fn is_taken(&self, label: &str) -> bool {
        self.has_document(label)
            || self
                .pending
                .lock()
                .is_ok_and(|pending| pending.contains_key(label))
    }

    /// 창을 닫으면 그 문서를 놓는다.
    pub fn forget(&self, label: &str) {
        if let Ok(mut windows) = self.windows.lock() {
            windows.remove(label);
        }
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(label);
        }
        if let Ok(mut modes) = self.modes.lock() {
            modes.remove(label);
        }
    }
}

/// 이미 다른 탭에 열린 파일이면 그 탭을 앞으로 가져온다(같은 파일을 두 탭에서 고치면 나중 저장이 앞 저장을 덮는다).
/// 옮겼으면 `true`.
pub fn focus_if_open(app: &AppHandle, path: &std::path::Path) -> bool {
    use tauri::Emitter;
    let Some(label) = app.state::<EditorState>().window_of(path) else {
        return false;
    };
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    let _ = app.emit_to(
        label.as_str(),
        "notice",
        "이미 이 탭에 열려 있는 파일입니다 — 같은 파일을 두 탭에서 고치지 않게 이 탭으로 옮겼습니다.",
    );
    true
}

/// 사유가 붙은 거절(화면이 사유로 알린다).
fn refused(reason: &str) -> CoreError {
    let mut error = CoreError::new(ErrorCode::PathNotAllowed, "editor", false);
    error.details.insert("reason".into(), reason.into());
    error
}

/// 다른 탭에 열린 파일에는 쓰지 않는다(그 탭의 문서와 디스크 파일이 어긋난다).
fn refuse_if_open_elsewhere(
    state: &State<'_, EditorState>,
    window: &Window,
    path: &std::path::Path,
) -> Result<(), CoreError> {
    match state.window_of(path) {
        Some(label) if label != window.label() => Err(refused("open_elsewhere")),
        _ => Ok(()),
    }
}

/// 있는 파일을 덮기 직전에 그 파일을 이전 판으로 남긴다. 못 남겨도 저장은 잇는다(알린다).
fn keep_history(app: &AppHandle, path: &std::path::Path) {
    let settings = crate::settings::read(app);
    let kept = crate::history::root(app).map(|root| {
        crate::history::prune_older_than(
            &root,
            settings.history_days,
            chrono::Local::now().naive_local(),
        );
        crate::history::keep(
            &root,
            path,
            settings.history_keep,
            &crate::history::stamp_now(),
        )
    });
    if !matches!(kept, Some(Ok(()))) {
        app.dialog()
            .message(format!(
                "「{}」의 이전 판을 남기지 못했습니다. 저장은 그대로 합니다.",
                path.file_name().unwrap_or_default().to_string_lossy()
            ))
            .title("서안 통특기")
            .show(|_| {});
    }
}

/// 새 탭(macOS 창 탭 — 같은 `tabbing_identifier` 창에 붙인다). 파일을 주면 그 탭 화면이 뜨자마자 연다.
pub fn new_tab(
    app: &AppHandle,
    parent: Option<&str>,
    path: Option<PathBuf>,
) -> Result<(), CoreError> {
    new_tab_with(app, parent, path, None)
}

fn new_tab_with(
    app: &AppHandle,
    parent: Option<&str>,
    path: Option<PathBuf>,
    mode: Option<OpenMode>,
) -> Result<(), CoreError> {
    let state = app.state::<EditorState>();
    let label = format!("doc-{}", state.tabs.fetch_add(1, Ordering::SeqCst) + 1);
    if let Some(path) = path {
        state.set_pending(&label, path);
    }
    if let Some(mode) = mode {
        state.set_mode(&label, mode);
    }
    let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title("서안 통특기")
        .inner_size(1120.0, 780.0)
        .min_inner_size(720.0, 620.0);
    // 창 탭 묶기는 macOS 만 있다(윈도는 창마다 따로 뜬다).
    #[cfg(target_os = "macos")]
    let builder = builder.tabbing_identifier("seoan");
    let window = builder
        .build()
        .map_err(|_| CoreError::new(ErrorCode::IoError, "new_tab", false))?;
    #[cfg(target_os = "macos")]
    if let Some(parent) = parent.and_then(|label| app.get_webview_window(label)) {
        attach_tab(&parent, &window);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (parent, window);
    Ok(())
}

/// 새 창을 부모 창의 탭으로 붙인다(AppKit `addTabbedWindow:ordered:` · 메인 스레드에서).
#[cfg(target_os = "macos")]
fn attach_tab(parent: &tauri::WebviewWindow, child: &tauri::WebviewWindow) {
    use objc2_app_kit::{NSWindow, NSWindowOrderingMode};
    let (Ok(parent_window), Ok(child_window)) = (parent.ns_window(), child.ns_window()) else {
        return;
    };
    // 창 포인터는 Send 가 아니라 주소로 넘긴다 — 두 창은 방금 만들었고 메인 스레드에서만 만진다.
    let (parent_window, child_window) = (parent_window as usize, child_window as usize);
    let _ = parent.run_on_main_thread(move || unsafe {
        let parent = &*(parent_window as *const NSWindow);
        let child = &*(child_window as *const NSWindow);
        parent.addTabbedWindow_ordered(child, NSWindowOrderingMode::Above);
    });
}

#[derive(Serialize)]
pub struct EditorViewDto {
    pub title: String,
    pub editable: bool,
    /// 읽기 전용으로 열었다(`editable` 도 거짓) — 화면이 「보기 전용」과 다른 까닭을 알린다.
    pub read_only: bool,
    pub page_count: usize,
    pub page_width: f64,
    pub page_height: f64,
    pub paragraphs: Vec<EditorParagraph>,
    /// 문단 → 처음 나오는 쪽(0 기준). 구조 창에서 그 쪽으로 간다.
    pub paragraph_pages: std::collections::BTreeMap<String, usize>,
    /// 편집 모델을 뒤에서 만드는 중(첫 고침이 그만큼 기다린다).
    pub preparing: bool,
    /// 표 칸 글(칸 id → 글 · 그림 `_`) — 쪽 위 칸 편집. 모델이 아직 없으면 비고 화면이 칸마다 묻는다.
    pub cells: std::collections::BTreeMap<String, String>,
    /// 방금 고침에서 CP949 에 없어 바꾼 글자들(한 번만 실린다 — 화면이 알린다).
    pub cp949_substitutions: Vec<hlt_core::Cp949Substitution>,
}

fn view(session: &mut EditorSession, title: String) -> EditorViewDto {
    session.poll();
    EditorViewDto {
        preparing: session.preparing(),
        title,
        editable: session.editable() && !session.read_only,
        read_only: session.read_only,
        page_count: session.plan.pages.len(),
        page_width: session.plan.width,
        page_height: session.plan.height,
        paragraphs: session.paragraphs(),
        cells: session.cell_texts(),
        cp949_substitutions: std::mem::take(&mut session.cp949_substitutions),
        paragraph_pages: session
            .plan
            .pages
            .iter()
            .enumerate()
            .rev()
            .flat_map(|(index, page)| {
                page.lines
                    .iter()
                    .filter_map(move |line| line.paragraph.clone().map(|id| (id, index)))
            })
            .collect(),
    }
}

fn title(session: &EditorSession) -> String {
    session.document.summary.document_tree_label.clone()
}

/// 이 창(탭)의 문서로 일한다.
fn with_session<T>(
    state: &State<'_, EditorState>,
    window: &Window,
    run: impl FnOnce(&mut EditorSession) -> Result<T, CoreError>,
) -> Result<T, CoreError> {
    let slot = state.slot(window.label());
    let mut guard = slot
        .lock()
        .map_err(|_| CoreError::new(ErrorCode::VerificationFailed, "editor_lock", false))?;
    let session = guard
        .session
        .as_mut()
        .ok_or_else(|| CoreError::new(ErrorCode::PathNotAllowed, "editor_closed", false))?;
    run(session)
}

#[tauri::command]
pub async fn editor_open(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
) -> Result<Option<EditorViewDto>, CoreError> {
    let Some(picked) = app
        .dialog()
        .file()
        .add_filter("명세서 · 제출파일", &["HLT", "hlt", "fin", "hlz"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|_| CoreError::new(ErrorCode::PathNotAllowed, "select", false))?;
    if focus_if_open(&app, &path) {
        return Ok(None);
    }
    open_here(&app, &state, &window, path, None).await
}

/// 읽기 전용으로 열기(원본 보호): 고칠 수 없고 ⌘S 를 거절한다 — 사본은 「다른 이름으로 저장」.
#[tauri::command]
pub async fn editor_open_read_only(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
) -> Result<Option<EditorViewDto>, CoreError> {
    let Some(path) = app
        .dialog()
        .file()
        .add_filter("명세서 · 제출파일", &["HLT", "hlt", "fin", "hlz"])
        .blocking_pick_file()
        .and_then(|picked| picked.into_path().ok())
    else {
        return Ok(None);
    };
    if focus_if_open(&app, &path) {
        return Ok(None);
    }
    open_here(&app, &state, &window, path, Some(OpenMode::ReadOnly)).await
}

/// 이 탭에 문서가 있으면 새 탭에서(`None` = 이 탭은 그대로), 없으면 여기서 연다.
async fn open_here(
    app: &AppHandle,
    state: &State<'_, EditorState>,
    window: &Window,
    path: PathBuf,
    mode: Option<OpenMode>,
) -> Result<Option<EditorViewDto>, CoreError> {
    if state.has_document(window.label()) {
        new_tab_with(app, Some(window.label()), Some(path), mode)?;
        return Ok(None);
    }
    if let Some(mode) = mode {
        state.set_mode(window.label(), mode);
    }
    open_path(state, window, path).await.map(Some)
}

/// 지금 문서의 이전 판(새것부터 · 저장할 때 덮인 판).
#[tauri::command]
pub async fn editor_history_list(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
) -> Result<Vec<crate::history::Kept>, CoreError> {
    let source = state
        .slot(window.label())
        .lock()
        .ok()
        .and_then(|slot| slot.source.clone());
    Ok(match (crate::history::root(&app), source) {
        (Some(root), Some(source)) => crate::history::list(&root, &source),
        _ => Vec::new(),
    })
}

/// 이전 판 하나(목록 차례)를 새 탭에 이름 없는 문서로 연다 — 원본에 덮지 않는다(저장하면 이름을 묻는다).
/// 경로는 화면에서 받지 않는다(목록을 다시 만들어 차례로 고른다).
#[tauri::command]
pub async fn editor_history_open(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
    index: usize,
) -> Result<Option<EditorViewDto>, CoreError> {
    let source = state
        .slot(window.label())
        .lock()
        .ok()
        .and_then(|slot| slot.source.clone())
        .ok_or_else(|| refused("history_none"))?;
    let root = crate::history::root(&app).ok_or_else(|| refused("history_none"))?;
    let kept = crate::history::list(&root, &source)
        .into_iter()
        .nth(index)
        .ok_or_else(|| refused("history_none"))?;
    let name = source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let title = format!("이전 판: {name} ({})", kept.saved);
    open_here(
        &app,
        &state,
        &window,
        kept.path,
        Some(OpenMode::Copy(title)),
    )
    .await
}

/// 불러와 HLT 만들기: 워드 · KIPO XML · HML → 새 `.HLT`(CLI `generate-hlt` 와 같은 길) → 연다.
/// 출원 종류(PCT · 국내)는 묻는다 — 생성 규칙(섹션 표제 · 줄지도)이 갈리는데 파일만 보고는 못 가른다.
#[tauri::command]
pub async fn editor_import(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
) -> Result<ImportDto, CoreError> {
    let Some(input) = app
        .dialog()
        .file()
        .add_filter("워드 · KIPO XML · HML", &["docx", "xml", "hml"])
        .blocking_pick_file()
        .and_then(|picked| picked.into_path().ok())
    else {
        return Ok(ImportDto::cancelled());
    };
    let Some(dialect) = ask_dialect(&app, &window, "불러와 HLT 만들기") else {
        return Ok(ImportDto::cancelled());
    };
    let mut dialog = app
        .dialog()
        .file()
        .set_file_name(format!("{}.HLT", stem(&input.to_string_lossy())))
        .add_filter("HLT", &["HLT"]);
    if let Some(folder) = input.parent() {
        dialog = dialog.set_directory(folder);
    }
    let Some(output) = dialog
        .blocking_save_file()
        .and_then(|picked| picked.into_path().ok())
    else {
        return Ok(ImportDto::cancelled());
    };
    refuse_if_open_elsewhere(&state, &window, &output)?;
    let source = input.clone();
    let (document, payloads, review) = tauri::async_runtime::spawn_blocking(move || {
        let (document, payloads) = hlt_core::load_generation_source(&source)?;
        let review = hlt_core::scan_cp949(&document);
        Ok::<_, CoreError>((document, payloads, review))
    })
    .await
    .map_err(|_| CoreError::new(ErrorCode::VerificationFailed, "worker", false))??;
    let pending = PendingImport {
        document,
        payloads,
        input,
        output,
        dialect,
    };
    // CP949 밖 글자가 있으면 확인 표부터(화면이 고른 뒤 `editor_import_finish`).
    if !review.is_empty() {
        state
            .pending_import
            .lock()
            .map_err(|_| CoreError::new(ErrorCode::VerificationFailed, "import_lock", false))?
            .insert(window.label().to_string(), pending);
        return Ok(ImportDto { view: None, review });
    }
    let view = bake_import(
        &window,
        &app,
        &state,
        pending,
        hlt_core::Cp949Choices::new(),
    )
    .await?;
    Ok(ImportDto {
        view,
        ..ImportDto::cancelled()
    })
}

/// 원문과 나란히 보기: 영문 원문(.docx)을 골라 문단 + 짝 이름을 준다(취소 = `null`).
#[tauri::command]
pub async fn editor_source_open(
    app: AppHandle,
) -> Result<Option<Vec<hlt_core::SourceParagraph>>, CoreError> {
    let Some(path) = app
        .dialog()
        .file()
        .set_title("나란히 볼 원문(영문 .docx)")
        .add_filter("Word", &["docx"])
        .blocking_pick_file()
        .and_then(|picked| picked.into_path().ok())
    else {
        return Ok(None);
    };
    tauri::async_runtime::spawn_blocking(move || hlt_core::source_paragraphs(&path))
        .await
        .map_err(|_| CoreError::new(ErrorCode::VerificationFailed, "worker", false))?
        .map(Some)
}

/// 지금 판 문단마다 원문 짝 이름(`p:0001` · `claim:3` · 없으면 `null`) — 화면 문단 차례 그대로.
#[tauri::command]
pub async fn editor_paragraph_keys(
    window: Window,
    state: State<'_, EditorState>,
) -> Result<Vec<Option<String>>, CoreError> {
    with_session(&state, &window, |session| {
        Ok(hlt_core::paragraph_keys(&session.paragraphs()))
    })
}

/// 번호 다시 매기기 미리 보기: `figures` 면 도면, 아니면 청구항. 바꿀 문단 목록(전 · 후 글 · 비면 이미 차례).
#[tauri::command]
pub async fn editor_renumber_plan(
    window: Window,
    state: State<'_, EditorState>,
    figures: bool,
) -> Result<Vec<hlt_core::RenumberEdit>, CoreError> {
    with_session(&state, &window, |session| {
        let paragraphs = session.paragraphs();
        Ok(if figures {
            hlt_core::figure_renumber(&paragraphs)
        } else {
            hlt_core::claim_renumber(&paragraphs)
        })
    })
}

/// 여러 문단 글을 한 번에(되돌리기 한 번) — 번호 다시 매기기 적용.
#[tauri::command]
pub async fn editor_replace_texts(
    window: Window,
    state: State<'_, EditorState>,
    edits: Vec<(String, String)>,
) -> Result<EditorViewDto, CoreError> {
    with_session(&state, &window, |session| {
        session.replace_texts(&edits)?;
        let title = title(session);
        Ok(view(session, title))
    })
}

/// 제출 전 점검 「한 칸으로 모두 고치기」.
#[tauri::command]
pub async fn editor_fix_double_spaces(
    window: Window,
    state: State<'_, EditorState>,
) -> Result<EditorViewDto, CoreError> {
    with_session(&state, &window, |session| {
        session.fix_double_spaces()?;
        let title = title(session);
        Ok(view(session, title))
    })
}

/// 부호 목록(상세설명 「이름(부호)」 · 【부호의 설명】 — 부호 숫자 차례).
#[tauri::command]
pub async fn editor_sign_table(
    window: Window,
    state: State<'_, EditorState>,
) -> Result<Vec<hlt_core::SignRow>, CoreError> {
    with_session(&state, &window, |session| {
        Ok(hlt_core::sign_table(&session.paragraphs()))
    })
}

/// 일괄 처리 진행(창마다): 지금 파일 차례(0부터) · 전체 · 파일 이름.
#[derive(Clone, Serialize)]
struct BatchProgressDto {
    at: usize,
    total: usize,
    file: String,
}

/// 일괄 처리의 한 걸음: 진행을 그 창에 알리고, 「멈춤」을 눌렀으면 `false`.
fn batch_step(
    app: &AppHandle,
    label: &str,
) -> impl FnMut(usize, usize, &str) -> bool + Send + use<> {
    use tauri::Emitter;
    let app = app.clone();
    let label = label.to_string();
    move |at, total, file| {
        let _ = app.emit_to(
            &label,
            "batch-progress",
            BatchProgressDto {
                at,
                total,
                file: file.to_string(),
            },
        );
        !app.state::<EditorState>().batch_stop.load(Ordering::SeqCst)
    }
}

/// 일괄 HLT 만들기: 폴더를 고르고 출원 종류를 한 번 묻고, 폴더 안 워드 · XML · HML 을 하나씩 HLT 로(있으면 건너뜀). 취소 = `null`.
#[tauri::command]
pub async fn editor_batch_generate(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
) -> Result<Option<Vec<hlt_core::BatchRow>>, CoreError> {
    let Some(folder) = app
        .dialog()
        .file()
        .set_title("HLT 로 만들 워드 · XML · HML 이 든 폴더")
        .blocking_pick_folder()
        .and_then(|picked| picked.into_path().ok())
    else {
        return Ok(None);
    };
    let Some(dialect) = ask_dialect(&app, &window, "폴더 HLT 만들기") else {
        return Ok(None);
    };
    state.batch_stop.store(false, Ordering::SeqCst);
    let step = batch_step(&app, window.label());
    tauri::async_runtime::spawn_blocking(move || {
        hlt_core::batch_generate_hlt(&folder, dialect, step)
    })
    .await
    .map(Some)
    .map_err(|_| CoreError::new(ErrorCode::VerificationFailed, "worker", false))
}

/// 일괄 제출 전 점검: 폴더를 고르고 그 안 HLT · 최종본마다 점검(종류별 수). 취소 = `null`.
#[tauri::command]
pub async fn editor_batch_check(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
) -> Result<Option<Vec<hlt_core::BatchCheckRow>>, CoreError> {
    let Some(folder) = app
        .dialog()
        .file()
        .set_title("점검할 HLT · 최종본(.fin)이 든 폴더")
        .blocking_pick_folder()
        .and_then(|picked| picked.into_path().ok())
    else {
        return Ok(None);
    };
    state.batch_stop.store(false, Ordering::SeqCst);
    let step = batch_step(&app, window.label());
    tauri::async_runtime::spawn_blocking(move || hlt_core::batch_submission_checks(&folder, step))
        .await
        .map(Some)
        .map_err(|_| CoreError::new(ErrorCode::VerificationFailed, "worker", false))
}

/// 일괄 처리 멈춤(지금 파일을 끝내고 멈춘다).
#[tauri::command]
pub fn editor_batch_stop(state: State<'_, EditorState>) {
    state.batch_stop.store(true, Ordering::SeqCst);
}

/// 표를 CSV 로 저장(엑셀이 한글을 읽게 BOM 을 붙인다). 저장한 경로(취소 = `null`).
#[tauri::command]
pub async fn editor_save_csv(
    app: AppHandle,
    name: String,
    text: String,
) -> Result<Option<String>, CoreError> {
    let Some(path) = pick_save(&app, &name, "csv") else {
        return Ok(None);
    };
    write_replacing(&path, format!("\u{feff}{text}").as_bytes())
        .map_err(|_| CoreError::new(ErrorCode::IoError, "save_csv", false))?;
    Ok(Some(path.display().to_string()))
}

/// 출원 종류(PCT · 국내)를 묻는다 — 생성 규칙(섹션 표제 · 줄지도)이 갈리는데 파일만 보고는 못 가른다. 취소 = `None`.
fn ask_dialect(
    app: &AppHandle,
    window: &Window,
    title: &str,
) -> Option<hlt_core::GenerationDialect> {
    use hlt_core::GenerationDialect;
    use tauri_plugin_dialog::{MessageDialogButtons, MessageDialogResult};
    match app
        .dialog()
        .message("어떤 출원으로 만들까요?")
        .title(title)
        .parent(&window)
        .buttons(MessageDialogButtons::YesNoCancelCustom(
            "PCT 국내진입".into(),
            "국내 출원".into(),
            "취소".into(),
        ))
        .blocking_show_with_result()
    {
        MessageDialogResult::Yes => Some(GenerationDialect::PctPatent),
        MessageDialogResult::Custom(label) if label == "PCT 국내진입" => {
            Some(GenerationDialect::PctPatent)
        }
        MessageDialogResult::No => Some(GenerationDialect::PatentSpecificationKo),
        MessageDialogResult::Custom(label) if label == "국내 출원" => {
            Some(GenerationDialect::PatentSpecificationKo)
        }
        _ => None,
    }
}

/// 확인 표 뒤: 고른 대응(글자 → 바꿀 글 · 빈 글 = 지움)으로 굽고 연다. `None` = 취소(불러오기를 버린다).
#[tauri::command]
pub async fn editor_import_finish(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
    choices: Option<std::collections::BTreeMap<String, String>>,
) -> Result<Option<EditorViewDto>, CoreError> {
    let pending = state
        .pending_import
        .lock()
        .map_err(|_| CoreError::new(ErrorCode::VerificationFailed, "import_lock", false))?
        .remove(window.label());
    let (Some(pending), Some(choices)) = (pending, choices) else {
        return Ok(None);
    };
    let choices = choices
        .into_iter()
        .filter_map(|(from, to)| from.chars().next().map(|ch| (ch, to)))
        .collect();
    bake_import(&window, &app, &state, pending, choices).await
}

/// CP949 밖 글자 찾기(편집기 붙여넣기 · 확인 표의 「직접 입력」 칸 검사).
#[tauri::command]
pub fn editor_cp949_scan(text: String) -> Vec<hlt_core::Cp949Finding> {
    hlt_core::scan_cp949_text(&text)
}

async fn bake_import(
    window: &Window,
    app: &AppHandle,
    state: &State<'_, EditorState>,
    pending: PendingImport,
    choices: hlt_core::Cp949Choices,
) -> Result<Option<EditorViewDto>, CoreError> {
    let output = pending.output.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let PendingImport {
            mut document,
            payloads,
            input,
            output,
            dialect,
        } = pending;
        hlt_core::apply_cp949(&mut document, &choices)?;
        bake_replacing(&output, |staged| {
            hlt_core::save_generated_hlt_as_new(
                &input,
                &document,
                dialect,
                &payloads,
                staged,
                hlt_core::WriteOptions::default(),
            )
            .map(|_| ())
        })
    })
    .await
    .map_err(|_| CoreError::new(ErrorCode::VerificationFailed, "worker", false))??;
    // 열기와 같다: 이 탭에 문서가 있으면 새 탭에서.
    if state.has_document(window.label()) {
        new_tab(app, Some(window.label()), Some(output))?;
        return Ok(None);
    }
    open_path(state, window, output).await.map(Some)
}

/// 새 의견서: 의견서 서식(빈 문서 + 【의견내용】)으로 새 `.HLT` 를 만들고 연다(이 탭에 문서가 있으면 새 탭).
/// `body` 가 있으면 그 글(줄 하나 = 본문 문단 하나)로 채운다 — 「보정 근거」 초안의 「새 의견서로 열기」.
#[tauri::command]
pub async fn editor_new_opinion(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
    body: Option<String>,
) -> Result<Option<EditorViewDto>, CoreError> {
    let Some(output) = pick_save(&app, "의견서.HLT", "HLT") else {
        return Ok(None);
    };
    refuse_if_open_elsewhere(&state, &window, &output)?;
    let target = output.clone();
    tauri::async_runtime::spawn_blocking(move || {
        bake_replacing(&target, |staged| match body {
            Some(body) => hlt_core::opinion_hlt_from_text(&body, staged),
            None => hlt_core::save_new_opinion_as_new(staged, hlt_core::WriteOptions::default())
                .map(|_| ()),
        })
    })
    .await
    .map_err(|_| CoreError::new(ErrorCode::VerificationFailed, "worker", false))??;
    if state.has_document(window.label()) {
        new_tab(&app, Some(window.label()), Some(output))?;
        return Ok(None);
    }
    open_path(&state, &window, output).await.map(Some)
}

/// 닫기(⌘W · 창 닫기 단추 · 앱 끝내기): 화면이 친 글을 다 보낸 뒤 부른다. 고친 것이 있으면 저장을 묻는다.
/// `false` = 취소(창을 두고 앱 끝내기도 멈춘다).
#[tauri::command]
pub async fn editor_close(
    app: AppHandle,
    window: Window,
    state: State<'_, EditorState>,
) -> Result<bool, CoreError> {
    if !confirm_discard(&app, &window, &state)? {
        return Ok(cancel_close(&state));
    }
    let _ = window.destroy();
    Ok(true)
}

/// 문서 닫기: 고친 것이 있으면 저장을 묻고 문서만 내린다(창 · 탭은 빈 편집기로 남는다). `false` = 취소.
#[tauri::command]
pub async fn editor_close_document(
    app: AppHandle,
    window: Window,
    state: State<'_, EditorState>,
) -> Result<bool, CoreError> {
    if !confirm_discard(&app, &window, &state)? {
        return Ok(false);
    }
    if let Ok(mut slot) = state.slot(window.label()).lock() {
        slot.session = None;
        slot.source = None;
    }
    crate::recovery::discard(&app, window.label());
    crate::recent::changed(&app, None);
    let _ = window.set_title("서안 통특기");
    Ok(true)
}

/// 고친 것이 있으면 「저장 · 저장 안 함 · 취소」를 묻는다. 저장 = 연 HLT 에 덮어쓰기(다른 형식이면 다른 이름으로).
/// `true` = 문서를 내려도 된다.
fn confirm_discard(
    app: &AppHandle,
    window: &Window,
    state: &State<'_, EditorState>,
) -> Result<bool, CoreError> {
    use tauri_plugin_dialog::{MessageDialogButtons, MessageDialogResult};
    if !state.is_dirty(window.label()) {
        return Ok(true);
    }
    let name = window.title().unwrap_or_default();
    let answer = app
        .dialog()
        .message(format!(
            "「{name}」에 저장하지 않은 고침이 있습니다. 저장할까요?"
        ))
        .title("서안 통특기")
        .parent(window)
        .buttons(MessageDialogButtons::YesNoCancelCustom(
            "저장".into(),
            "저장 안 함".into(),
            "취소".into(),
        ))
        .blocking_show_with_result();
    let save = match answer {
        MessageDialogResult::Yes => true,
        MessageDialogResult::No => false,
        MessageDialogResult::Custom(label) if label == "저장" => true,
        MessageDialogResult::Custom(label) if label == "저장 안 함" => false,
        _ => return Ok(false),
    };
    if save && save_current(app, window, state)?.is_none() {
        return Ok(false);
    }
    Ok(true)
}

/// 저장: 연 파일이 HLT 면 그 자리에 덮어쓰고, 제출파일(.fin · .hlz)이나 새 문서면 다른 이름으로 묻는다.
/// `None` = 저장 대화상자를 취소했다.
fn save_current(
    app: &AppHandle,
    window: &Window,
    state: &State<'_, EditorState>,
) -> Result<Option<PathBuf>, CoreError> {
    use tauri_plugin_dialog::{MessageDialogButtons, MessageDialogResult};
    if with_session(state, window, |session| Ok(session.read_only))? {
        return Err(refused("read_only_save"));
    }
    let (source, stamp) = state
        .slot(window.label())
        .lock()
        .map(|slot| (slot.source.clone(), slot.stamp))
        .unwrap_or_default();
    let source = source.filter(|path| {
        path.extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("hlt"))
    });
    let name = stem(&window.title().unwrap_or_default());
    let ask_name = || pick_save(app, &format!("{name}.HLT"), "HLT");
    let path = match source {
        // 연 뒤 다른 곳(통특기 · 다른 앱 · 동기화)에서 바뀐 파일이면 덮기 전에 묻는다 — 말없이 덮으면 그쪽 고침이 사라진다.
        Some(path) if changed_since(stamp, &path) => {
            let answer = app
                .dialog()
                .message(format!(
                    "「{}」이(가) 연 뒤에 다른 곳에서 바뀌었습니다.\n덮어쓰면 그쪽에서 고친 것은 이 파일에서 사라집니다(이전 판으로는 남습니다).",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ))
                .title("서안 통특기")
                .parent(window)
                .buttons(MessageDialogButtons::YesNoCancelCustom(
                    "덮어쓰기".into(),
                    "다른 이름으로".into(),
                    "취소".into(),
                ))
                .blocking_show_with_result();
            match answer {
                MessageDialogResult::Yes => path,
                MessageDialogResult::Custom(label) if label == "덮어쓰기" => path,
                MessageDialogResult::No => match ask_name() {
                    Some(path) => path,
                    None => return Ok(None),
                },
                MessageDialogResult::Custom(label) if label == "다른 이름으로" => {
                    match ask_name() {
                        Some(path) => path,
                        None => return Ok(None),
                    }
                }
                _ => return Ok(None),
            }
        }
        Some(path) => path,
        None => {
            let Some(path) = ask_name() else {
                return Ok(None);
            };
            path
        }
    };
    refuse_if_open_elsewhere(state, window, &path)?;
    keep_history(app, &path);
    with_session(state, window, |session| session.save_over(&path))?;
    if let Ok(mut slot) = state.slot(window.label()).lock() {
        slot.source = Some(path.clone());
        slot.stamp = stamp_of(&path);
    }
    crate::recovery::discard(app, window.label());
    crate::recent::changed(window.app_handle(), Some(&path));
    if let Some(name) = path.file_name() {
        let _ = window.set_title(&name.to_string_lossy());
    }
    Ok(Some(path))
}

/// 자동 저장(고친 뒤 손을 멈추면 화면이 부른다): 고친 것이 있으면 지금 판을 복구본으로 두고, 없으면 복구본을 지운다.
#[tauri::command]
pub async fn editor_autosave(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
) -> Result<(), CoreError> {
    let Some(dir) = crate::recovery::dir(&app) else {
        return Ok(());
    };
    let key = crate::recovery::key(window.label());
    let source = state
        .slot(window.label())
        .lock()
        .ok()
        .and_then(|slot| slot.source.clone());
    let current = with_session(&state, &window, |session| {
        if session.is_dirty() {
            session.materialize().map(Some)
        } else {
            Ok(None)
        }
    })?;
    match current {
        Some(current) => {
            let entry = crate::recovery::Entry {
                hlt: PathBuf::new(),
                title: window.title().unwrap_or_default(),
                source,
            };
            crate::recovery::store(&dir, &key, &current, &entry)
                .map_err(|_| CoreError::new(ErrorCode::IoError, "autosave", false))
        }
        None => {
            crate::recovery::remove(&dir, &key);
            Ok(())
        }
    }
}

/// 저장(⌘S).
#[tauri::command]
pub async fn editor_save(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
) -> Result<Option<String>, CoreError> {
    Ok(save_current(&app, &window, &state)?.map(|path| path.display().to_string()))
}

fn cancel_close(state: &State<'_, EditorState>) -> bool {
    state.quitting.store(false, Ordering::SeqCst);
    false
}

fn stem(name: &str) -> String {
    std::path::Path::new(name)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "편집본".into())
}

/// 새 탭(⌘T · ⌘N).
#[tauri::command]
pub async fn editor_new_tab(app: AppHandle, window: Window) -> Result<(), CoreError> {
    new_tab(&app, Some(window.label()), None)
}

/// 이 창에 맡긴 파일(Finder · 다른 탭의 열기)을 연다(없으면 `None`). 경로는 화면이 아니라 운영체제 · 열기 대화상자가 준 것만 쓴다.
#[tauri::command]
pub async fn editor_open_pending(
    window: Window,
    state: State<'_, EditorState>,
) -> Result<Option<EditorViewDto>, CoreError> {
    let path = state
        .pending
        .lock()
        .ok()
        .and_then(|mut pending| pending.remove(window.label()));
    match path {
        Some(path) => open_path(&state, &window, path).await.map(Some),
        None => Ok(None),
    }
}

async fn open_path(
    state: &State<'_, EditorState>,
    window: &Window,
    path: PathBuf,
) -> Result<EditorViewDto, CoreError> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let opened = path.clone();
    let session = tauri::async_runtime::spawn_blocking(move || EditorSession::open(&opened))
        .await
        .map_err(|_| CoreError::new(ErrorCode::VerificationFailed, "worker", false))??;
    let mut session = session;
    let mode = state
        .modes
        .lock()
        .ok()
        .and_then(|mut modes| modes.remove(window.label()));
    // 복구본: 원래 파일에 덮지 않게 「새 문서」로 둔다(저장하면 이름을 묻는다) · 고친 문서로 친다 · 복구본은 지운다.
    let recovered = crate::recovery::entry_of(window.app_handle(), &path);
    if recovered.is_some() {
        session.mark_unsaved();
    }
    // 이전 판: 복구본과 같게 이름 없는 문서로(원본에 덮지 않는다).
    let copy = match &mode {
        Some(OpenMode::Copy(title)) => {
            session.mark_unsaved();
            Some(title.clone())
        }
        _ => None,
    };
    session.read_only = matches!(mode, Some(OpenMode::ReadOnly));
    let read_only = session.read_only;
    let label = title(&session);
    let dto = view(&mut session, label);
    let slot = state.slot(window.label());
    let mut slot = slot
        .lock()
        .map_err(|_| CoreError::new(ErrorCode::VerificationFailed, "editor_lock", false))?;
    slot.session = Some(session);
    slot.source = (recovered.is_none() && copy.is_none()).then(|| path.clone());
    slot.stamp = slot.source.as_deref().and_then(stamp_of);
    drop(slot);
    if let Some(title) = copy {
        let _ = window.set_title(&title);
        return Ok(dto);
    }
    if let Some(entry) = recovered {
        if let Some(dir) = crate::recovery::dir(window.app_handle()) {
            crate::recovery::remove(
                &dir,
                &path.file_stem().unwrap_or_default().to_string_lossy(),
            );
        }
        let _ = window.set_title(&format!("복구: {}", entry.title));
        return Ok(dto);
    }
    crate::recent::changed(window.app_handle(), Some(&path));
    // 탭 제목 = 파일 이름.
    let _ = window.set_title(&if read_only {
        format!("{name} (읽기 전용)")
    } else {
        name
    });
    Ok(dto)
}

/// 지금 보기(편집 준비가 끝났는지 화면이 물을 때).
#[tauri::command]
pub async fn editor_view(
    window: Window,
    state: State<'_, EditorState>,
) -> Result<EditorViewDto, CoreError> {
    with_session(&state, &window, |session| {
        let title = title(session);
        Ok(view(session, title))
    })
}

#[tauri::command]
pub async fn editor_page(
    window: Window,
    state: State<'_, EditorState>,
    index: usize,
) -> Result<PlannedPage, CoreError> {
    with_session(&state, &window, |session| {
        session
            .plan
            .pages
            .get(index)
            .cloned()
            .ok_or_else(|| CoreError::new(ErrorCode::PathNotAllowed, "page", false))
    })
}

/// 그림 PNG 바이트(화면에서 Blob URL 로 쓴다).
#[tauri::command]
pub async fn editor_image(
    window: Window,
    state: State<'_, EditorState>,
    object_code: u32,
) -> Result<Response, CoreError> {
    with_session(&state, &window, |session| {
        session
            .image_png(object_code)
            .map(Response::new)
            .ok_or_else(|| CoreError::new(ErrorCode::StructureInvalid, "image", false))
    })
}

#[tauri::command]
pub async fn editor_replace_text(
    window: Window,
    state: State<'_, EditorState>,
    paragraph_id: String,
    text: String,
) -> Result<EditorViewDto, CoreError> {
    with_session(&state, &window, |session| {
        session.replace_text(&paragraph_id, &text)?;
        {
            let title = title(session);
            Ok(view(session, title))
        }
    })
}

/// 문단을 앞 문단에 붙인다(쪽 위 편집의 Backspace · Delete).
#[tauri::command]
pub async fn editor_merge_paragraph(
    window: Window,
    state: State<'_, EditorState>,
    paragraph_id: String,
) -> Result<EditorViewDto, CoreError> {
    with_session(&state, &window, |session| {
        session.merge_with_previous(&paragraph_id)?;
        let title = title(session);
        Ok(view(session, title))
    })
}

/// 여러 문단에 걸친 고르기 지우기(쪽 위 편집).
#[tauri::command]
pub async fn editor_delete_range(
    window: Window,
    state: State<'_, EditorState>,
    from_id: String,
    from_offset: usize,
    to_id: String,
    to_offset: usize,
) -> Result<EditorViewDto, CoreError> {
    with_session(&state, &window, |session| {
        session.delete_range(&from_id, from_offset, &to_id, to_offset)?;
        let title = title(session);
        Ok(view(session, title))
    })
}

/// 문단 정렬(양쪽 · 가운데 · 왼쪽) — 커서가 있는 문단.
#[tauri::command]
pub async fn editor_set_alignment(
    window: Window,
    state: State<'_, EditorState>,
    paragraph_id: String,
    align: String,
) -> Result<EditorViewDto, CoreError> {
    with_session(&state, &window, |session| {
        session.set_alignment(&paragraph_id, &align)?;
        let title = title(session);
        Ok(view(session, title))
    })
}

/// 문단 첫 줄 들여쓰기(「들여쓰기」 800 · 「들여쓰기 취소」 0) — 커서가 있는 문단.
#[tauri::command]
pub async fn editor_set_indent(
    window: Window,
    state: State<'_, EditorState>,
    paragraph_id: String,
    indent: i32,
) -> Result<EditorViewDto, CoreError> {
    with_session(&state, &window, |session| {
        session.set_indent(&paragraph_id, indent)?;
        let title = title(session);
        Ok(view(session, title))
    })
}

/// 강제 쪽나눔(⌘/Ctrl+Enter): 문단 나누기와 같되 둘째 조각이 새 쪽에서 시작한다.
#[tauri::command]
pub async fn editor_replace_text_page_break(
    window: Window,
    state: State<'_, EditorState>,
    paragraph_id: String,
    text: String,
) -> Result<EditorViewDto, CoreError> {
    with_session(&state, &window, |session| {
        session.replace_text_with_page_break(&paragraph_id, &text)?;
        let title = title(session);
        Ok(view(session, title))
    })
}

/// 여러 문단에 걸친 고르기의 서식: 문단마다 (id, start, end).
#[tauri::command]
pub async fn editor_toggle_format_spans(
    window: Window,
    state: State<'_, EditorState>,
    spans: Vec<(String, usize, usize)>,
    format: CharacterFormat,
) -> Result<EditorViewDto, CoreError> {
    with_session(&state, &window, |session| {
        session.toggle_format_spans(&spans, format)?;
        let title = title(session);
        Ok(view(session, title))
    })
}

/// 서식째 붙여넣기(클립보드 `text/html` 의 런들): 문단 글의 `offset` 자리.
#[tauri::command]
pub async fn editor_paste_formatted(
    window: Window,
    state: State<'_, EditorState>,
    paragraph_id: String,
    offset: usize,
    runs: Vec<(String, Vec<CharacterFormat>)>,
) -> Result<EditorViewDto, CoreError> {
    with_session(&state, &window, |session| {
        session.paste_formatted(&paragraph_id, offset, &runs)?;
        let title = title(session);
        Ok(view(session, title))
    })
}

/// 복사: 고른 구간을 서식째 HTML 로(클립보드 `text/html`).
#[tauri::command]
pub async fn editor_formatted_html(
    window: Window,
    state: State<'_, EditorState>,
    spans: Vec<(String, usize, usize)>,
) -> Result<String, CoreError> {
    with_session(&state, &window, |session| session.formatted_html(&spans))
}

#[derive(Serialize)]
pub struct ItemInsertedDto {
    pub view: EditorViewDto,
    /// 표제 아래 빈 본문 문단(커서를 둔다).
    pub body: String,
}

/// 식별항목 넣기(우클릭 「식별항목」): 【종류 번호】 표제 + 빈 본문 문단.
#[tauri::command]
pub async fn editor_insert_item(
    window: Window,
    state: State<'_, EditorState>,
    paragraph_id: String,
    before: bool,
    kind: String,
    number: String,
) -> Result<ItemInsertedDto, CoreError> {
    with_session(&state, &window, |session| {
        let body = session.insert_item(&paragraph_id, before, &kind, &number)?;
        let title = title(session);
        Ok(ItemInsertedDto {
            view: view(session, title),
            body,
        })
    })
}

/// 서열목록 넣기: ST.26 XML 파일을 고르면(문서 폴더에서 연다) 【서열목록】 · 첨부 줄을 넣는다. 취소면 `None`.
#[tauri::command]
pub async fn editor_insert_sequence_listing(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
) -> Result<Option<ItemInsertedDto>, CoreError> {
    let source = state
        .slot(window.label())
        .lock()
        .ok()
        .and_then(|slot| slot.source.clone());
    let mut dialog = app
        .dialog()
        .file()
        .set_title("서열목록 파일 고르기(ST.26 XML)")
        .add_filter("서열목록 ST.26(XML)", &["xml", "XML"]);
    if let Some(folder) = source.as_deref().and_then(|path| path.parent()) {
        dialog = dialog.set_directory(folder);
    }
    let Some(path) = dialog
        .blocking_pick_file()
        .and_then(|path| path.into_path().ok())
    else {
        return Ok(None);
    };
    with_session(&state, &window, |session| {
        let body = session.insert_sequence_listing(&path)?;
        let title = title(session);
        Ok(Some(ItemInsertedDto {
            view: view(session, title),
            body,
        }))
    })
}

/// 수식·화학식 넣기: 화면 편집기가 그린 PNG(base64) · 표시 크기 pt.
#[tauri::command]
pub async fn editor_insert_figure(
    window: Window,
    state: State<'_, EditorState>,
    after: String,
    kind: FigureKind,
    png_base64: String,
    width_pt: f64,
    height_pt: f64,
) -> Result<EditorViewDto, CoreError> {
    let png = base64::engine::general_purpose::STANDARD
        .decode(png_base64)
        .map_err(|_| CoreError::new(ErrorCode::StructureInvalid, "figure_png", false))?;
    with_session(&state, &window, |session| {
        session.insert_figure(&after, kind, png, width_pt, height_pt)?;
        {
            let title = title(session);
            Ok(view(session, title))
        }
    })
}

/// 그림 크기 바꾸기(비율 잠금): 판독 그림 id · 배율.
#[tauri::command]
pub async fn editor_resize_image(
    window: Window,
    state: State<'_, EditorState>,
    image: String,
    scale: f64,
) -> Result<EditorViewDto, CoreError> {
    with_session(&state, &window, |session| {
        session.resize_image(&image, scale)?;
        let title = title(session);
        Ok(view(session, title))
    })
}

#[derive(serde::Serialize)]
pub struct ReviewReadDto {
    #[serde(flatten)]
    report: hlt_core::ReviewReport,
    /// 오기 후보마다 지금 문서에서 걸릴 곳 수.
    matches: Vec<usize>,
}

/// 보정안 불러오기 ①: OA 검토보고서(docx)를 골라 읽는다(문서 폴더에서 연다 · 취소면 `None`).
#[tauri::command]
pub async fn editor_read_review(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
) -> Result<Option<ReviewReadDto>, CoreError> {
    let source = state
        .slot(window.label())
        .lock()
        .ok()
        .and_then(|slot| slot.source.clone());
    let mut dialog = app
        .dialog()
        .file()
        .set_title("OA 검토보고서 고르기")
        .add_filter("Word(docx)", &["docx"]);
    if let Some(folder) = source.as_deref().and_then(|path| path.parent()) {
        dialog = dialog.set_directory(folder);
    }
    let Some(path) = dialog
        .blocking_pick_file()
        .and_then(|path| path.into_path().ok())
    else {
        return Ok(None);
    };
    let report = hlt_core::read_review_report(&path)?;
    let matches = with_session(&state, &window, |session| {
        Ok(report
            .notes
            .iter()
            .map(|note| session.review_note_matches(note))
            .collect())
    })?;
    Ok(Some(ReviewReadDto { report, matches }))
}

#[derive(serde::Serialize)]
pub struct ReviewAppliedDto {
    view: EditorViewDto,
    outcome: hlt_core::ReviewOutcome,
}

/// 보정안 불러오기 ②: 고른 보정안의 청구항 · 고른 오기 정정을 한 번의 고침으로 건다.
#[tauri::command]
pub async fn editor_apply_review(
    window: Window,
    state: State<'_, EditorState>,
    claims: Vec<hlt_core::ProposedClaim>,
    tracked: bool,
    notes: Vec<hlt_core::DescriptionNote>,
) -> Result<ReviewAppliedDto, CoreError> {
    with_session(&state, &window, |session| {
        let outcome = session.apply_review(&claims, tracked, &notes)?;
        let title = title(session);
        Ok(ReviewAppliedDto {
            view: view(session, title),
            outcome,
        })
    })
}

/// 보정 표시: 최종본(.fin)으로 연 문서만(아니면 `null`) · 첫 판에 견준 바뀐 글.
#[tauri::command]
pub async fn editor_amendment_marks(
    window: Window,
    state: State<'_, EditorState>,
    original: Option<bool>,
    file: Option<bool>,
) -> Result<Option<hlt_core::AmendmentMarks>, CoreError> {
    with_session(&state, &window, |session| {
        Ok(if file.unwrap_or(false) {
            session.amendment_marks_against_file()
        } else {
            session.amendment_marks_from(original.unwrap_or(false))
        })
    })
}

/// 보정 표시 기준 「다른 파일…」: 견줄 HLT · fin 을 골라 연다(사본 · 원본은 안 건드린다). 고른 파일 이름(취소 = `null`).
#[tauri::command]
pub async fn editor_compare_pick(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
) -> Result<Option<String>, CoreError> {
    let Some(path) = app
        .dialog()
        .file()
        .add_filter("HLT · 최종본", &["HLT", "hlt", "fin", "hlz"])
        .blocking_pick_file()
        .and_then(|picked| picked.into_path().ok())
    else {
        return Ok(None);
    };
    with_session(&state, &window, |session| session.compare_with(&path))?;
    Ok(path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned()))
}

/// 보정 근거 초안: 최종본(.fin)으로 연 문서만(아니면 `null`).
#[tauri::command]
pub async fn editor_amendment_basis(
    window: Window,
    state: State<'_, EditorState>,
    original: Option<bool>,
) -> Result<Option<Vec<hlt_core::ClaimBasis>>, CoreError> {
    with_session(&state, &window, |session| {
        Ok(session.amendment_basis_from(original.unwrap_or(false)))
    })
}

/// 청구항 인용 검사(보정서를 만들기 전에 화면이 묻는다).
#[tauri::command]
pub async fn editor_claim_issues(
    window: Window,
    state: State<'_, EditorState>,
) -> Result<Vec<hlt_core::CitationIssue>, CoreError> {
    with_session(&state, &window, |session| {
        Ok(session.claim_citation_issues())
    })
}

/// 청구항 나무: 항마다 인용하는 앞 항(빈 = 독립항).
#[tauri::command]
pub async fn editor_claim_parents(
    window: Window,
    state: State<'_, EditorState>,
) -> Result<std::collections::BTreeMap<u32, Vec<u32>>, CoreError> {
    with_session(&state, &window, |session| Ok(session.claim_parents()))
}

/// 제출 전 점검(제출파일 직전) — 의견서(종류 7)는 볼 것이 없다.
#[tauri::command]
pub async fn editor_submission_checks(
    window: Window,
    state: State<'_, EditorState>,
) -> Result<Vec<hlt_core::SubmitIssue>, CoreError> {
    with_session(&state, &window, |session| {
        Ok(
            if session.document.summary.document_type_raw & 0x7fff == 7 {
                Vec::new()
            } else {
                session.submission_checks()
            },
        )
    })
}

/// 그림 바꾸기(도면 보정): JPG · TIFF 를 골라 판독 그림 `image` 자리에 · 고르기를 취소하면 문서는 그대로.
#[tauri::command]
pub async fn editor_replace_picture(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
    image: String,
) -> Result<EditorViewDto, CoreError> {
    let bytes = pick_picture(&app, &state, &window)?;
    with_session(&state, &window, |session| {
        if let Some(bytes) = &bytes {
            session.replace_picture(&image, bytes)?;
        }
        let title = title(session);
        Ok(view(session, title))
    })
}

/// 그림 파일(JPG · TIFF) 고르기 — 문서 폴더에서 연다 · 취소면 `None`.
fn pick_picture(
    app: &AppHandle,
    state: &State<'_, EditorState>,
    window: &Window,
) -> Result<Option<Vec<u8>>, CoreError> {
    let source = state
        .slot(window.label())
        .lock()
        .ok()
        .and_then(|slot| slot.source.clone());
    let mut dialog = app
        .dialog()
        .file()
        .add_filter("그림(JPG · TIFF)", &["jpg", "jpeg", "tif", "tiff"]);
    if let Some(folder) = source.as_deref().and_then(|path| path.parent()) {
        dialog = dialog.set_directory(folder);
    }
    dialog
        .blocking_pick_file()
        .and_then(|path| path.into_path().ok())
        .map(|path| {
            std::fs::read(&path).map_err(|_| CoreError::new(ErrorCode::IoError, "picture", false))
        })
        .transpose()
}

/// 그림 파일 넣기: JPG · TIFF 를 골라 `after` 문단 뒤에 그림 문단으로 — `offset` 이 있으면 `after` 는 표 칸이고 칸 글의 그 자리에.
/// 고르기를 취소하면 문서는 그대로.
#[tauri::command]
pub async fn editor_insert_picture(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
    after: String,
    offset: Option<usize>,
) -> Result<EditorViewDto, CoreError> {
    let bytes = pick_picture(&app, &state, &window)?;
    with_session(&state, &window, |session| {
        if let Some(bytes) = &bytes {
            match offset {
                Some(offset) => session.insert_picture_in_cell(&after, offset, bytes)?,
                None => session.insert_picture(&after, bytes)?,
            }
        }
        let title = title(session);
        Ok(view(session, title))
    })
}

/// 글자 서식 켜기/끄기(문단 글 [start, end) · 그림 = 한 칸).
#[tauri::command]
pub async fn editor_toggle_format(
    window: Window,
    state: State<'_, EditorState>,
    paragraph_id: String,
    start: usize,
    end: usize,
    format: CharacterFormat,
) -> Result<EditorViewDto, CoreError> {
    with_session(&state, &window, |session| {
        session.toggle_format(&paragraph_id, start, end, format)?;
        {
            let title = title(session);
            Ok(view(session, title))
        }
    })
}

#[tauri::command]
pub async fn editor_find(
    window: Window,
    state: State<'_, EditorState>,
    query: hlt_core::FindQuery,
) -> Result<Vec<hlt_core::FindMatch>, CoreError> {
    with_session(&state, &window, |session| session.find_checked(&query))
}

/// 바꾸기 · 모두 바꾸기(한 번에 다시 굽는다 · 되돌리기 한 번).
#[tauri::command]
pub async fn editor_replace(
    window: Window,
    state: State<'_, EditorState>,
    matches: Vec<hlt_core::FindMatch>,
    replacement: String,
    format: hlt_core::ReplaceFormat,
    pattern: Option<hlt_core::FindQuery>,
) -> Result<EditorViewDto, CoreError> {
    with_session(&state, &window, |session| {
        // 정규식으로 찾았으면 `$1` 을 찾은 글마다 채운다.
        match pattern.filter(|query| query.regex) {
            Some(query) => session.replace_pattern(&matches, &query, &replacement, format)?,
            None => session.replace(&matches, &replacement, format)?,
        };
        {
            let title = title(session);
            Ok(view(session, title))
        }
    })
}

#[tauri::command]
pub async fn editor_set_size(
    window: Window,
    state: State<'_, EditorState>,
    paragraph_id: String,
    start: usize,
    end: usize,
    size: u32,
) -> Result<EditorViewDto, CoreError> {
    with_session(&state, &window, |session| {
        session.set_size(&paragraph_id, start, end, size)?;
        {
            let title = title(session);
            Ok(view(session, title))
        }
    })
}

#[tauri::command]
pub async fn editor_cell_text(
    window: Window,
    state: State<'_, EditorState>,
    cell_id: String,
) -> Result<String, CoreError> {
    with_session(&state, &window, |session| {
        session
            .cell_text(&cell_id)
            .ok_or_else(|| CoreError::new(ErrorCode::StructureInvalid, "cell", false))
    })
}

#[tauri::command]
pub async fn editor_replace_cell_text(
    window: Window,
    state: State<'_, EditorState>,
    cell_id: String,
    text: String,
) -> Result<EditorViewDto, CoreError> {
    with_session(&state, &window, |session| {
        session.replace_cell_text(&cell_id, &text)?;
        {
            let title = title(session);
            Ok(view(session, title))
        }
    })
}

#[derive(Serialize)]
pub struct TableInsertedDto {
    pub view: EditorViewDto,
    /// 새 표 첫 칸(화면이 칸 창을 연다).
    pub cell: String,
}

/// 표 그리기: `after` 문단 뒤에 `rows`×`columns` 새 표.
#[tauri::command]
pub async fn editor_insert_table(
    window: Window,
    state: State<'_, EditorState>,
    after: String,
    rows: u32,
    columns: u32,
) -> Result<TableInsertedDto, CoreError> {
    with_session(&state, &window, |session| {
        let cell = session.insert_table(&after, rows, columns)?;
        let title = title(session);
        Ok(TableInsertedDto {
            view: view(session, title),
            cell,
        })
    })
}

/// 표 편집 하나(줄 · 칸 넣기/지우기 · 합치기 · 나누기).
#[tauri::command]
pub async fn editor_table_edit(
    window: Window,
    state: State<'_, EditorState>,
    operation: hlt_core::TableOperation,
) -> Result<EditorViewDto, CoreError> {
    with_session(&state, &window, |session| {
        session.table_edit(operation)?;
        let title = title(session);
        Ok(view(session, title))
    })
}

#[tauri::command]
pub async fn editor_delete_table(
    window: Window,
    state: State<'_, EditorState>,
    table_id: String,
) -> Result<EditorViewDto, CoreError> {
    with_session(&state, &window, |session| {
        session.delete_table(&table_id)?;
        let title = title(session);
        Ok(view(session, title))
    })
}

/// 칸의 표 격자 자리(우클릭 「표」 메뉴가 쓴다 · 병합에 덮인 칸이면 null).
#[tauri::command]
pub async fn editor_cell_position(
    window: Window,
    state: State<'_, EditorState>,
    cell_id: String,
) -> Result<Option<hlt_core::CellPosition>, CoreError> {
    with_session(&state, &window, |session| {
        Ok(session.cell_position(&cell_id))
    })
}

#[tauri::command]
pub async fn editor_undo(
    window: Window,
    state: State<'_, EditorState>,
) -> Result<EditorViewDto, CoreError> {
    with_session(&state, &window, |session| {
        session.undo()?;
        {
            let title = title(session);
            Ok(view(session, title))
        }
    })
}

#[tauri::command]
pub async fn editor_redo(
    window: Window,
    state: State<'_, EditorState>,
) -> Result<EditorViewDto, CoreError> {
    with_session(&state, &window, |session| {
        session.redo()?;
        {
            let title = title(session);
            Ok(view(session, title))
        }
    })
}

#[tauri::command]
pub async fn editor_number_paragraphs(
    window: Window,
    state: State<'_, EditorState>,
) -> Result<EditorViewDto, CoreError> {
    with_session(&state, &window, |session| {
        session.number_paragraphs()?;
        {
            let title = title(session);
            Ok(view(session, title))
        }
    })
}

/// 화면의 실패 한 줄을 앱 기록 폴더의 `editor.log` 에 덧붙인다(시각 · 내용).
/// 맥 `~/Library/Logs/<앱 id>` · 윈도 `%LOCALAPPDATA%\<앱 id>\logs`.
#[tauri::command]
pub async fn editor_log(app: AppHandle, line: String) -> Result<(), CoreError> {
    use std::io::Write;
    let dir = app
        .path()
        .app_log_dir()
        .map_err(|_| CoreError::new(ErrorCode::IoError, "editor_log", false))?;
    std::fs::create_dir_all(&dir)
        .and_then(|_| {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("editor.log"))
        })
        .and_then(|mut file| writeln!(file, "{} {line}", chrono::Local::now().to_rfc3339()))
        .map_err(|_| CoreError::new(ErrorCode::IoError, "editor_log", false))
}

fn pick_save(app: &AppHandle, name: &str, extension: &str) -> Option<PathBuf> {
    app.dialog()
        .file()
        .set_file_name(name)
        .add_filter(extension, &[extension])
        .blocking_save_file()?
        .into_path()
        .ok()
}

/// 다른 이름으로 저장 — 덮어쓰기는 저장 대화상자가 이미 물었다(`save_over` = 옆 임시 파일에 쓴 뒤 바꿔 끼운다).
#[tauri::command]
pub async fn editor_save_as(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
) -> Result<Option<String>, CoreError> {
    let Some(path) = pick_save(&app, "편집본.HLT", "HLT") else {
        return Ok(None);
    };
    refuse_if_open_elsewhere(&state, &window, &path)?;
    keep_history(&app, &path);
    // 읽기 전용 문서는 사본을 만들면 그 사본을 고치는 문서가 된다(원본은 그대로).
    let unlocked = with_session(&state, &window, |session| {
        session.save_over(&path)?;
        Ok(std::mem::take(&mut session.read_only))
    })?;
    if unlocked {
        if let Ok(mut slot) = state.slot(window.label()).lock() {
            slot.source = Some(path.clone());
            slot.stamp = stamp_of(&path);
        }
        crate::recent::changed(&app, Some(&path));
        if let Some(name) = path.file_name() {
            let _ = window.set_title(&name.to_string_lossy());
        }
    }
    Ok(Some(path.display().to_string()))
}

/// 인쇄 PDF. 글꼴 = 사용자 시스템 바탕체(동봉하지 않는다).
#[tauri::command]
pub async fn editor_print_pdf(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
) -> Result<Option<String>, CoreError> {
    let font = system_batang()
        .ok_or_else(|| CoreError::new(ErrorCode::IoError, "font_batang_missing", false))?;
    let Some(path) = pick_save(&app, "인쇄.pdf", "pdf") else {
        return Ok(None);
    };
    // 덮어쓰기는 저장 대화상자가 이미 물었다.
    let pdf = with_session(&state, &window, |session| session.print_pdf(font, 1))?;
    write_replacing(&path, &pdf)
        .map_err(|_| CoreError::new(ErrorCode::IoError, "print_pdf", false))?;
    Ok(Some(path.display().to_string()))
}

/// Word(.docx)로 저장 — 표제는 나오고 식별번호는 안 나온다.
#[tauri::command]
pub async fn editor_export_docx(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
) -> Result<Option<String>, CoreError> {
    // 기본 이름 · 자리 = 연 HLT 와 같게(이름.docx).
    let source = state
        .slot(window.label())
        .lock()
        .ok()
        .and_then(|slot| slot.source.clone());
    let name = source
        .as_deref()
        .and_then(|path| path.file_stem())
        .and_then(|stem| stem.to_str())
        .unwrap_or("편집본");
    let mut dialog = app
        .dialog()
        .file()
        .set_file_name(format!("{name}.docx"))
        .add_filter("docx", &["docx"]);
    if let Some(folder) = source.as_deref().and_then(|path| path.parent()) {
        dialog = dialog.set_directory(folder);
    }
    let Some(path) = dialog
        .blocking_save_file()
        .and_then(|path| path.into_path().ok())
    else {
        return Ok(None);
    };
    // 덮어쓰기는 저장 대화상자가 이미 물었다. 옆 임시 파일에 다 쓴 뒤 바꿔 끼워 실패해도 옛 파일이 남는다.
    let docx = with_session(&state, &window, |session| session.docx())?;
    let temp = path.with_extension("docx.tmp");
    std::fs::write(&temp, docx)
        .and_then(|()| std::fs::rename(&temp, &path))
        .map_err(|_| {
            let _ = std::fs::remove_file(&temp);
            CoreError::new(ErrorCode::IoError, "export_docx", false)
        })?;
    Ok(Some(path.display().to_string()))
}

/// 제출파일 만들기(통특기 「제출파일 생성」): 지금 판 → KIPO XML + 도면 → `.hlz` → `.fin`. 이름 = 고른 파일 이름.
#[tauri::command]
pub async fn editor_export_fin(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
) -> Result<Option<String>, CoreError> {
    // 통특기와 같은 차례: `.hlz` 자리를 묻고 `.hlz` 를 먼저 쓴 뒤 `.fin` 을 만든다.
    // `.fin` 은 통특기가 `C:\KIPONET\NKEditor\Data\Fin\<이름>.fin` 에 두지만 맥엔 그 자리가 없어 `.hlz` 옆에 둔다.
    let source = state
        .slot(window.label())
        .lock()
        .ok()
        .and_then(|slot| slot.source.clone());
    let default_name = source
        .as_deref()
        .and_then(|path| path.file_stem())
        .and_then(|stem| stem.to_str())
        .unwrap_or("제출")
        .to_string();
    let mut dialog = app
        .dialog()
        .file()
        .set_file_name(format!("{default_name}.hlz"))
        .add_filter("hlz", &["hlz"]);
    if let Some(folder) = source.as_deref().and_then(|path| path.parent()) {
        dialog = dialog.set_directory(folder);
    }
    let Some(picked) = dialog
        .blocking_save_file()
        .and_then(|picked| picked.into_path().ok())
    else {
        return Ok(None);
    };
    let hlz_path = picked.with_extension("hlz");
    let path = hlz_path.with_extension("fin");
    // 의견서(문서 종류 7)는 `.hlz` 만 낸다 — 특허로에 의견서 hlz 를 올린다(최종본 .fin 은 명세서 몫 · 사용자 2026-10-16).
    let opinion = with_session(&state, &window, |session| {
        Ok(session.document.summary.document_type_raw & 0x7fff == 7)
    })?;
    // 저장 창이 덮어쓰기를 물은 것은 고른 그 파일뿐 — 옆 `.fin`(· 확장자를 붙여 바뀐 `.hlz`)이 이미 있으면 여기서 묻는다.
    let unasked: Vec<String> = [
        (hlz_path != picked).then_some(&hlz_path),
        (!opinion).then_some(&path),
    ]
    .into_iter()
    .flatten()
    .filter(|candidate| candidate.exists())
    .filter_map(|candidate| {
        candidate
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
    })
    .collect();
    if !unasked.is_empty()
        && !app
            .dialog()
            .message(format!(
                "{} 이(가) 이미 있습니다. 덮어쓸까요?",
                unasked.join(" · ")
            ))
            .title("제출파일")
            .parent(&window)
            .buttons(tauri_plugin_dialog::MessageDialogButtons::OkCancelCustom(
                "덮어쓰기".into(),
                "취소".into(),
            ))
            .blocking_show()
    {
        return Ok(None);
    }
    let stem = hlz_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("submission")
        .to_string();
    let now = chrono::Local::now();
    let source = with_session(&state, &window, |session| session.materialize())?;
    let export = hlt_core::export_fin_path(
        &source,
        &hlt_core::KipoExportOptions::default(),
        &hlt_core::FinExportOptions {
            hlz_name: format!("{stem}.hlz"),
            app_date: now.format("%Y-%m-%d").to_string(),
            app_time: now.timestamp().to_string(),
            image_time: now.format("%Y:%m:%d %H:%M:%S").to_string(),
            amendments: Vec::new(),
        },
    )?;
    write_replacing(&hlz_path, &export.hlz)
        .map_err(|_| CoreError::new(ErrorCode::IoError, "export_hlz", false))?;
    if opinion {
        return Ok(Some(hlz_path.display().to_string()));
    }
    write_replacing(&path, &export.fin)
        .map_err(|_| CoreError::new(ErrorCode::IoError, "export_fin", false))?;
    Ok(Some(path.display().to_string()))
}

/// 보정서 한 항목(읽기 전용 보정서 보기 — 통특기 【보정대상 항목】 · 【보정방법】 · 【보정내용】).
#[derive(Debug, Clone, serde::Serialize)]
pub struct AmendItemDto {
    pub target: String,
    pub method: String,
    pub content: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AmendmentDoneDto {
    pub dta: String,
    pub fin: String,
    /// 보정서 차수(1차 · 2차 …).
    pub order: u32,
    pub items: Vec<AmendItemDto>,
    /// 보정 전후 대비표(.docx · dta 옆). 못 썼으면 `None`(보정서는 그대로 낸다).
    pub table: Option<String>,
}

/// 보정서생성(통특기 「보정서생성」): 최종본(.fin)으로 연 문서만. 보정서 파일 이름을 묻고(기본 `<문서>.dta` ·
/// fin 폴더) `.dta` 를 그 자리에 쓰고, **연 `.fin` 을 그 자리에서** 보정서를 더한 판으로 바꾼다(통특기도 그렇다).
/// 문서는 고친 상태 그대로 둔다(통특기도 닫을 때 저장을 묻는다). 고친 것이 없으면 `amendment_empty`(「변경된 내용이 없습니다」).
#[tauri::command]
pub async fn editor_export_amendment(
    window: Window,
    app: AppHandle,
    state: State<'_, EditorState>,
) -> Result<Option<AmendmentDoneDto>, CoreError> {
    let fin = state
        .slot(window.label())
        .lock()
        .ok()
        .and_then(|slot| slot.source.clone())
        .filter(|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("fin"))
        })
        .ok_or_else(|| {
            let mut error = CoreError::new(ErrorCode::UnsupportedFormat, "export_amendment", false);
            error
                .details
                .insert("reason".into(), "amendment_needs_final".into());
            error
        })?;
    let mut dialog = app
        .dialog()
        .file()
        .set_title("보정서 파일이름 입력")
        .set_file_name(format!("{}.dta", stem(&window.title().unwrap_or_default())))
        .add_filter("DTA", &["dta"]);
    if let Some(folder) = fin.parent() {
        dialog = dialog.set_directory(folder);
    }
    let Some(dta_path) = dialog
        .blocking_save_file()
        .and_then(|picked| picked.into_path().ok())
    else {
        return Ok(None);
    };
    let dta_name = dta_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| CoreError::new(ErrorCode::PathNotAllowed, "export_amendment", false))?;
    let now = chrono::Local::now();
    // 식별번호는 편집 세션이 든다(서안이 구운 HLT 에는 실리지 않는다) — 같이 넘겨야 문단을 나누고 지운 보정이 번호대로 간다.
    let (after, numbers) = with_session(&state, &window, |session| {
        Ok((session.materialize()?, session.paragraph_numbers()))
    })?;
    let export = hlt_core::export_amendment_from_fin(
        &fin,
        &after,
        &dta_name,
        &now.format("%Y-%m-%d").to_string(),
        &hlt_core::KipoExportOptions {
            paragraph_numbers: numbers,
            ..Default::default()
        },
        &hlt_core::AmendmentOptions {
            // 비우면 차수로 매긴다(첫 보정서 002 · 실물 320/320) — 기본값 003 은 CLI 비 fin 경로용.
            dta_version: String::new(),
            image_time: now.format("%Y:%m:%d %H:%M:%S").to_string(),
            ..hlt_core::AmendmentOptions::default()
        },
    )?;
    let io = |_| CoreError::new(ErrorCode::IoError, "export_amendment", false);
    write_replacing(&dta_path, &export.dta).map_err(io)?;
    // 연 fin 을 그 자리에서 바꾼다(통특기 보정서생성도 그렇다).
    write_replacing(&fin, &export.fin).map_err(io)?;
    // 보정 전후 대비표 — 보정서와 같은 기준(연 상태 = 최종본). 덤이라 못 써도 보정서는 낸다.
    let table_path = dta_path.with_file_name(format!("{}_대비표.docx", stem(&dta_name)));
    let table = with_session(&state, &window, |session| Ok(session.amendment_table()))
        .ok()
        .flatten()
        .filter(|rows| !rows.is_empty())
        .and_then(|rows| {
            hlt_core::comparison_table_docx(
                &format!("보정 전후 대비표 — {}", stem(&dta_name)),
                &rows,
            )
            .ok()
        })
        .filter(|bytes| write_replacing(&table_path, bytes).is_ok())
        .map(|_| table_path.display().to_string());
    Ok(Some(AmendmentDoneDto {
        table,
        dta: dta_path.display().to_string(),
        fin: fin.display().to_string(),
        order: amendment_order(&export.xml),
        items: amendment_items(&export.xml),
    }))
}

/// 있는 파일도 덮는다 — 옆 임시 파일(`이름.확장자.tmp`)에 다 쓴 뒤 바꿔 끼워, 실패해도 반쯤 쓴 파일 대신 옛 파일이 남는다.
fn write_replacing(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temporary);
    })
}

/// 새 `.HLT` 굽기(코어는 있는 파일을 거절한다)를 덮어쓰기로 — 덮어쓰기는 저장 대화상자가 이미 물었다.
/// 옆 임시 파일(`이름.seoan-tmp.HLT`)에 구운 뒤 바꿔 끼워, 실패해도 옛 파일이 남는다.
fn bake_replacing(
    path: &std::path::Path,
    bake: impl FnOnce(&std::path::Path) -> Result<(), CoreError>,
) -> Result<(), CoreError> {
    let staged = path.with_extension("seoan-tmp.HLT");
    let _ = std::fs::remove_file(&staged);
    let baked = bake(&staged).and_then(|()| {
        std::fs::rename(&staged, path)
            .map_err(|_| CoreError::new(ErrorCode::IoError, "replace", false))
    });
    if baked.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    baked
}

/// 차수 = dtaVersion − 1(첫 보정서 002 — 실물 보정서 320/320).
fn amendment_order(xml: &str) -> u32 {
    xml.split_once("dtaVersion=\"")
        .and_then(|(_, rest)| rest.split_once('"'))
        .and_then(|(value, _)| value.parse::<u32>().ok())
        .map_or(1, |value| value.saturating_sub(1).max(1))
}

/// 보정서 XML 의 `<AmendBody>` 를 통특기 보정서 보기 항목으로.
fn amendment_items(xml: &str) -> Vec<AmendItemDto> {
    let attribute = |head: &str, name: &str| {
        head.split_once(&format!("{name}=\""))
            .and_then(|(_, rest)| rest.split_once('"'))
            .map(|(value, _)| value.to_string())
            .unwrap_or_default()
    };
    xml.split("<AmendBody ")
        .skip(1)
        .filter_map(|chunk| {
            let (head, rest) = chunk.split_once('>')?;
            let body = rest.split("</AmendBody>").next().unwrap_or_default();
            let number = attribute(head, "attributeValue")
                .trim_start_matches('0')
                .to_string();
            let target = match attribute(head, "elementName").as_str() {
                "claim" => format!("청구항 {number}"),
                "p" => format!("식별번호 [{:0>4}]", number),
                "invention-title" => "발명의 명칭".to_string(),
                "summary" => "요약".to_string(),
                "figure" => format!("도 {number}"),
                other => other.to_string(),
            };
            let method = match attribute(head, "status").as_str() {
                "D" => "삭제",
                "I" => "추가",
                _ => "정정",
            }
            .to_string();
            let mut content = String::new();
            let mut in_tag = false;
            for ch in body.replace("<br/>", "\n").chars() {
                match ch {
                    '<' => in_tag = true,
                    '>' => in_tag = false,
                    _ if !in_tag => content.push(ch),
                    _ => {}
                }
            }
            Some(AmendItemDto {
                target,
                method,
                content: content.trim().to_string(),
            })
        })
        .collect()
}

/// 단락삭제(통특기 「단락삭제」 대화상자): 종류 하나 · 번호 여럿. 【청구항 N】은 내용까지 지운다.
#[tauri::command]
pub async fn editor_remove_items(
    window: Window,
    state: State<'_, EditorState>,
    kind: String,
    numbers: Vec<String>,
) -> Result<EditorViewDto, CoreError> {
    with_session(&state, &window, |session| {
        session.remove_items(&kind, &numbers)?;
        let title = title(session);
        Ok(view(session, title))
    })
}

/// 맥에 깔린 바탕체(`batang.ttc` · 1번 = BatangChe). Office 가 사용자 글꼴 폴더에 깐다.
fn system_batang() -> Option<Vec<u8>> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    [
        home.join("Library/Fonts/batang.ttc"),
        PathBuf::from("/Library/Fonts/batang.ttc"),
        PathBuf::from("/Applications/Microsoft Word.app/Contents/Resources/DFonts/batang.ttc"),
    ]
    .into_iter()
    .find_map(|path| std::fs::read(path).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bakes_over_a_file_the_save_dialog_agreed_to_replace() {
        let dir = std::env::temp_dir().join(format!("seoan-bake-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("명세서.HLT");
        std::fs::write(&path, "옛").unwrap();
        // 지난번에 끊겨 남은 임시 파일이 있어도 굽는다.
        std::fs::write(dir.join("명세서.seoan-tmp.HLT"), "끊김").unwrap();
        // 코어처럼 있는 파일은 거절하는 굽기.
        let bake = |text: &'static str| {
            move |staged: &std::path::Path| {
                if staged.exists() {
                    return Err(CoreError::new(ErrorCode::OutputExists, "output", false));
                }
                std::fs::write(staged, text)
                    .map_err(|_| CoreError::new(ErrorCode::IoError, "write", false))
            }
        };
        bake_replacing(&path, bake("새")).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "새");
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            1,
            "임시 파일이 남지 않는다"
        );
        // 굽다 실패하면 옛 파일이 남고 임시 파일은 치운다.
        let failed = bake_replacing(&path, |staged| {
            std::fs::write(staged, "반쯤").unwrap();
            Err(CoreError::new(
                ErrorCode::VerificationFailed,
                "verify",
                false,
            ))
        });
        assert!(failed.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "새");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn each_tab_keeps_its_own_document_slot() {
        let state = EditorState::default();
        assert!(!Arc::ptr_eq(&state.slot("main"), &state.slot("doc-1")));
        assert!(Arc::ptr_eq(&state.slot("main"), &state.slot("main")));
        // 맡긴 파일이 있으면 「문서가 있다」로 본다(그 창에 또 열지 않고 새 탭으로).
        state.set_pending("doc-1", PathBuf::from("a.HLT"));
        assert!(state.has_document("doc-1") && !state.has_document("main"));
        state.forget("doc-1");
        assert!(!state.has_document("doc-1"));
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("seoan-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn notices_a_file_changed_after_it_was_opened() {
        let dir = scratch("stamp");
        let path = dir.join("a.HLT");
        std::fs::write(&path, "열 때").unwrap();
        let stamp = stamp_of(&path);
        assert!(!changed_since(stamp, &path));
        // 다른 앱이 고쳤다(크기가 바뀐다 — 시각만 보면 같은 초 안의 고침을 놓칠 수 있다).
        std::fs::write(&path, "다른 앱이 고친 글").unwrap();
        assert!(changed_since(stamp, &path));
        // 표지가 없거나(새 문서) 파일이 없으면 묻지 않는다.
        assert!(!changed_since(None, &path));
        std::fs::remove_file(&path).unwrap();
        assert!(!changed_since(stamp, &path));
    }

    #[test]
    fn finds_the_tab_holding_the_same_file_by_any_spelling() {
        let dir = scratch("same");
        let (a, b) = (dir.join("a.HLT"), dir.join("b.HLT"));
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        let other_spelling = dir.join("sub/../a.HLT");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        assert!(same_file(&a, &other_spelling));
        assert!(!same_file(&a, &b));

        let state = EditorState::default();
        state.slot("doc-2").lock().unwrap().source = Some(a.clone());
        state.set_pending("doc-3", b.clone());
        assert_eq!(state.window_of(&other_spelling).as_deref(), Some("doc-2"));
        assert_eq!(state.window_of(&b).as_deref(), Some("doc-3"));
        assert_eq!(state.window_of(&dir.join("c.HLT")), None);
    }
}

#[cfg(test)]
mod amendment_view_tests {
    use super::{amendment_items, amendment_order};

    /// 통특기 보정서(dta 의 XML 꼴) → 보정서 보기 항목 · 차수.
    #[test]
    fn reads_the_amendment_like_the_nkeditor_view() {
        let xml = "<KIPO keapsVersion=\"5.6\" dtaVersion=\"003\" editorKind=\"K\">\n<Amendment>\n\
            <AmendBody elementName=\"claim\" status=\"D\" attributeName=\"num\" attributeValue=\"3\">\n</AmendBody>\n\
            <AmendBody elementName=\"p\" status=\"A\" attributeName=\"num\" attributeValue=\"0012\">\n\
            <p num=\"0012\">새 글<br/>옛 <b>글</b></p>\n</AmendBody>\n</Amendment>\n</KIPO>";
        assert_eq!(amendment_order(xml), 2);
        let items = amendment_items(xml);
        assert_eq!(items.len(), 2);
        assert_eq!(
            (
                items[0].target.as_str(),
                items[0].method.as_str(),
                items[0].content.as_str()
            ),
            ("청구항 3", "삭제", "")
        );
        assert_eq!(
            (
                items[1].target.as_str(),
                items[1].method.as_str(),
                items[1].content.as_str()
            ),
            ("식별번호 [0012]", "정정", "새 글\n옛 글")
        );
    }
}
