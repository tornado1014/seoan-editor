//! 한국어 메뉴 막대: 툴바 · ⌘K 와 같은 이름. 항목 id 가 화면이 할 일을 말한다 —
//! `action:<툴바 data-action>` · `tool:<도구 메뉴 이름>` · `batch:generate|check` · `help:tours|shortcuts` ·
//! `go` · `palette`. 그 밖(업데이트 · 버그 신고 · 최근 문서 · 끝내기)은 `main.rs` 가 직접 한다.
//!
//! ⛔ 바로 가기 키는 붙이지 않는다(⌘O · ⌘Q 말고) — 맥 메뉴 바로 가기는 화면보다 먼저 키를 가져가, 화면이 이미 하는
//! ⌘S · ⌘F · ⌘K … 가 화면 칸(찾기 칸 등)에서 안 먹게 된다. 이름 끝에 키를 적어 보인다.

use tauri::menu::{Menu, MenuBuilder, MenuItem, Submenu, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager, Wry};

/// 「도구」 메뉴 — 화면 `TOOLS` 이름과 같아야 한다(화면 테스트가 대조한다).
pub const TOOLS: [&str; 7] = [
    "청구항 번호 다시 매기기…",
    "도면 번호 다시 매기기…",
    "부호 목록",
    "되풀이 문장 찾기",
    "문서 통계",
    "다음 고친 곳 (⌘])",
    "앞 고친 곳 (⌘[)",
];

pub fn build(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let item = |id: &str, text: &str| MenuItem::with_id(app, id, text, true, None::<&str>);
    let action = |name: &str, text: &str| item(&format!("action:{name}"), text);
    let file = SubmenuBuilder::new(app, "파일")
        .item(&action("new-tab", "새 탭 (⌘T)")?)
        .item(&action("new-opinion", "새 의견서")?)
        .item(&action("import", "워드/XML로 HLT 만들기…")?)
        .separator()
        .item(&MenuItem::with_id(
            app,
            "action:open",
            "열기…",
            true,
            Some("CmdOrCtrl+O"),
        )?)
        .item(&Submenu::with_id(app, "recent", "최근 문서", true)?)
        .separator()
        .item(&action("save", "저장 (⌘S)")?)
        .item(&action("save-as", "다른 이름으로 저장… (⇧⌘S)")?)
        .separator()
        .item(&action("pdf", "PDF로 인쇄…")?)
        .item(&action("docx", "Word로 저장…")?)
        .item(&action("fin", "제출파일 만들기…")?)
        .separator()
        .item(&action("close-document", "문서 닫기")?);
    #[cfg(not(target_os = "macos"))]
    let file = file.separator().item(&MenuItem::with_id(
        app,
        "quit",
        "끝내기",
        true,
        Some("CmdOrCtrl+Q"),
    )?);
    let edit = SubmenuBuilder::new(app, "편집")
        .undo_with_text("되돌리기")
        .redo_with_text("다시 하기")
        .separator()
        .cut_with_text("잘라내기")
        .copy_with_text("복사")
        .paste_with_text("붙여넣기")
        .select_all_with_text("모두 선택")
        .separator()
        .item(&action("find", "찾기 · 바꾸기 (⌘F)")?)
        .item(&item("go", "가기 — 식별번호 · 청구항 (⌘G)…")?)
        .item(&item("palette", "명령 찾기 (⌘K)…")?);
    let view = SubmenuBuilder::new(app, "보기")
        .item(&action("marks", "문단부호 보기")?)
        .item(&action("source", "원문 나란히 보기…")?)
        .item(&action("amend-marks", "보정 표시")?)
        .separator()
        .fullscreen_with_text("전체 화면");
    let mut tools = SubmenuBuilder::new(app, "도구");
    for name in TOOLS {
        tools = tools.item(&item(&format!("tool:{name}"), name)?);
    }
    let tools = tools
        .separator()
        .item(&item("batch:generate", "일괄 처리 — 폴더 HLT 만들기…")?)
        .item(&item("batch:check", "일괄 처리 — 폴더 제출 전 점검…")?);
    let amend = SubmenuBuilder::new(app, "보정")
        .item(&action("review", "검토보고서 불러오기…")?)
        .item(&action("amend-marks", "보정 표시")?)
        .item(&action("basis", "보정 근거…")?)
        .item(&action("amend", "보정서 만들기…")?);
    let window = SubmenuBuilder::new(app, "창")
        .minimize_with_text("최소화")
        .maximize_with_text("확대/축소")
        .separator()
        .close_window_with_text("탭 닫기");
    let help = SubmenuBuilder::new(app, "도움말")
        .item(&item("help:tours", "기능 투어…")?)
        .item(&item("help:shortcuts", "단축키 모음")?)
        .separator()
        .item(&item("check-update", "업데이트 확인…")?)
        .item(&item("report-bug", "버그 신고…")?);
    let mut menu = MenuBuilder::new(app);
    #[cfg(target_os = "macos")]
    {
        menu = menu.item(
            &SubmenuBuilder::new(app, "서안 통특기")
                .about_with_text("서안 통특기 정보", None)
                .separator()
                .services_with_text("서비스")
                .separator()
                .hide_with_text("서안 통특기 가리기")
                .hide_others_with_text("기타 가리기")
                .show_all_with_text("모두 보기")
                .separator()
                .item(&MenuItem::with_id(
                    app,
                    "quit",
                    "서안 통특기 종료",
                    true,
                    Some("CmdOrCtrl+Q"),
                )?)
                .build()?,
        );
    }
    menu.item(&file.build()?)
        .item(&edit.build()?)
        .item(&view.build()?)
        .item(&tools.build()?)
        .item(&amend.build()?)
        .item(&window.build()?)
        .item(&help.build()?)
        .build()
}

/// 화면이 할 일 항목이면 앞에 있는 창(없으면 첫 창)에 `menu-action` 으로 넘긴다. 넘겼으면 `true`.
pub fn forward(app: &AppHandle, id: &str) -> bool {
    let screen = ["action:", "tool:", "batch:", "help:"]
        .iter()
        .any(|prefix| id.starts_with(prefix))
        || id == "go"
        || id == "palette";
    if !screen {
        return false;
    }
    let windows = app.webview_windows();
    let label = windows
        .iter()
        .find(|(_, window)| window.is_focused().unwrap_or(false))
        .or_else(|| windows.get_key_value("main"))
        .map(|(label, _)| label.clone());
    if let Some(label) = label {
        let _ = app.emit_to(label.as_str(), "menu-action", id.to_string());
    }
    true
}
