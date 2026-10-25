#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(clippy::result_large_err)]

mod capabilities;
mod commands;
mod editor;
mod history;
mod menu;
mod recent;
mod recovery;
mod settings;
mod update;

use commands::{AppState, cancel_verify, select_hlt, verify_file};
use editor::{
    EditorState, editor_amendment_basis, editor_amendment_marks, editor_apply_review,
    editor_autosave, editor_batch_check, editor_batch_generate, editor_batch_stop,
    editor_cell_position, editor_cell_text, editor_claim_issues, editor_claim_parents,
    editor_close, editor_close_document, editor_compare_pick, editor_cp949_scan,
    editor_delete_range, editor_delete_table, editor_export_amendment, editor_export_docx,
    editor_export_fin, editor_find, editor_history_list, editor_history_open, editor_image,
    editor_import, editor_import_finish, editor_insert_figure, editor_insert_item,
    editor_insert_picture, editor_insert_table, editor_log, editor_merge_paragraph,
    editor_new_opinion, editor_new_tab, editor_number_paragraphs, editor_open, editor_open_pending,
    editor_open_read_only, editor_page, editor_paragraph_keys, editor_print_pdf,
    editor_read_review, editor_redo, editor_remove_items, editor_renumber_plan, editor_replace,
    editor_replace_cell_text, editor_replace_picture, editor_replace_text,
    editor_replace_text_page_break, editor_replace_texts, editor_resize_image, editor_save,
    editor_save_as, editor_save_csv, editor_set_alignment, editor_set_indent, editor_set_size,
    editor_sign_table, editor_source_open, editor_submission_checks, editor_table_edit,
    editor_toggle_format, editor_toggle_format_spans, editor_undo, editor_view,
};

/// 그 창 화면에 「닫으려 한다」를 알린다(화면이 친 글을 보낸 뒤 `editor_close`).
fn ask_close(app: &tauri::AppHandle, label: &str) {
    use tauri::Emitter;
    let _ = app.emit_to(label, "close-requested", label);
}

/// 파일 하나를 앱에서 연다(Finder · 최근 문서 · 복원): 문서 없는 첫 창이면 거기 · 아니면 포커스 창 옆 새 탭.
fn open_in_app(app: &tauri::AppHandle, path: std::path::PathBuf) {
    use tauri::Manager;
    if editor::focus_if_open(app, &path) {
        return;
    }
    let state = app.state::<EditorState>();
    // 실행 직후엔 첫 창이 아직 없을 수 있다(파일이 창보다 먼저 온다) — 그때도 첫 창 몫이다.
    let main_ready = app.get_webview_window("main").is_some()
        || !state.ready.load(std::sync::atomic::Ordering::SeqCst);
    if main_ready && !state.has_document("main") {
        state.set_pending("main", path);
    } else {
        let parent = app
            .webview_windows()
            .into_iter()
            .find(|(_, window)| window.is_focused().unwrap_or(false))
            .map(|(label, _)| label);
        let _ = editor::new_tab(app, parent.as_deref(), Some(path));
    }
}

/// 메뉴 「최근 문서」 항목: 없는 파일이면 알리고 목록에서 뺀다.
fn open_recent(app: &tauri::AppHandle, index: usize) {
    use tauri::Emitter;
    use tauri_plugin_dialog::DialogExt;
    let list = recent::read(app).recent;
    let Some(path) = list.get(index).cloned() else {
        return;
    };
    if !path.is_file() {
        app.dialog()
            .message(format!("파일이 없습니다: {}", path.display()))
            .title("최근 문서")
            .show(|_| {});
        recent::set_recent(app, list.into_iter().filter(|item| *item != path).collect());
        return;
    }
    open_in_app(app, path);
    let _ = app.emit("open-file", ());
}

/// 지난번 끝낼 때 열려 있던 탭을 다시 연다(지금 있는 파일만 · 첫째는 첫 창 · 나머지는 새 탭).
fn restore_tabs(app: &tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    let state = app.state::<EditorState>();
    if state.has_document("main") {
        return; // Finder · 실행 인자로 연 파일이 있다 — 그 파일을 원했다.
    }
    let mut seen: Vec<std::path::PathBuf> = Vec::new();
    let mut tabs = recent::read(app)
        .tabs
        .into_iter()
        .filter(|path| path.is_file())
        // 같은 파일은 한 탭에만(옛 판이 같은 파일을 두 탭에 열어 두었을 수 있다).
        .filter(|path| {
            let fresh = !seen.iter().any(|other| editor::same_file(other, path));
            seen.push(path.clone());
            fresh
        });
    let Some(first) = tabs.next() else {
        return;
    };
    state.set_pending("main", first);
    for path in tabs {
        let _ = editor::new_tab(app, Some("main"), Some(path));
    }
    let _ = app.emit("open-file", ());
}

/// 지난 실행이 비정상으로 끝나 남은 복구본이 있으면 복구할지 묻는다(복구 = 새 탭마다 하나 · 버리기 = 지운다).
fn offer_recovery(app: &tauri::AppHandle) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
    let Some(dir) = recovery::dir(app) else {
        return;
    };
    let entries = recovery::leftovers(&dir, std::process::id());
    if entries.is_empty() {
        return;
    }
    let names: Vec<String> = entries
        .iter()
        .map(|entry| match &entry.source {
            Some(source) => format!("· {} ({})", entry.title, source.display()),
            None => format!("· {}", entry.title),
        })
        .collect();
    let app = app.clone();
    app.clone()
        .dialog()
        .message(format!(
            "지난번에 저장하지 않고 끝난 문서가 {}개 있습니다. 복구할까요?\n복구한 문서는 원래 파일에 덮지 않습니다(저장할 때 이름을 묻는다).\n\n{}",
            entries.len(),
            names.join("\n")
        ))
        .title("복구")
        .buttons(MessageDialogButtons::OkCancelCustom("복구".into(), "버리기".into()))
        .show(move |restore| {
            use tauri::{Emitter, Manager};
            if !restore {
                for entry in &entries {
                    recovery::remove(&dir, &entry.hlt.file_stem().unwrap_or_default().to_string_lossy());
                }
                return;
            }
            let state = app.state::<EditorState>();
            for entry in entries {
                let main_taken = state.is_taken("main");
                if main_taken {
                    let _ = editor::new_tab(&app, Some("main"), Some(entry.hlt));
                } else {
                    state.set_pending("main", entry.hlt);
                }
            }
            let _ = app.emit("open-file", ());
        });
}

/// 앱 끝내기: 문서가 열린 창이 있으면 하나씩 닫으며 저장을 묻고(`Destroyed` 가 다음 창으로 잇는다), 없으면 끝낸다.
fn quit(app: &tauri::AppHandle) {
    use tauri::Manager;
    let state = app.state::<EditorState>();
    match state.open_windows().first() {
        Some(first) => {
            state
                .quitting
                .store(true, std::sync::atomic::Ordering::SeqCst);
            ask_close(app, first);
        }
        None => app.exit(0),
    }
}

const BUG_REPORT_TO: &str = "tornado1014@gmail.com";

/// `mailto:` 에 넣을 글: 영숫자 · `-_.~` 말고 전부 `%XX`(공백도 `+` 가 아니라 `%20` — 메일 앱이 `+` 를 그대로 보인다).
fn mailto_escape(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// 「버그 신고…」: 메일 앱에 받는 사람 · 제목 · 판/OS 를 채운 새 메일을 연다(파일은 첨부하지 않는다).
/// 오류 기록 위치는 일반형으로 적는다 — 실제 경로를 넣으면 신고자의 사용자 이름이 실린다.
fn report_bug(app: &tauri::AppHandle) {
    use tauri_plugin_dialog::DialogExt;
    let version = app.package_info().version.to_string();
    let id = &app.config().identifier;
    let log = if cfg!(windows) {
        format!("%LOCALAPPDATA%\\{id}\\logs\\editor.log")
    } else {
        format!("~/Library/Logs/{id}/editor.log")
    };
    let subject = format!("[서안 {version}] 버그 신고");
    let body = format!(
        "어떤 일이 있었나요?\n\n\n어떻게 하면 다시 생기나요?\n\n\n---\n서안 {version} · {} {}\n오류 기록: {log} (첨부해 주시면 도움이 됩니다)\n",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let url = format!(
        "mailto:{BUG_REPORT_TO}?subject={}&body={}",
        mailto_escape(&subject),
        mailto_escape(&body)
    );
    // 셸을 거치지 않는다(인자 하나로 넘김).
    let opened = if cfg!(windows) {
        std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", &url])
            .status()
    } else {
        std::process::Command::new("open").arg(&url).status()
    };
    if !opened.is_ok_and(|status| status.success()) {
        app.dialog()
            .message(format!(
                "메일 앱을 열지 못했습니다.\n{BUG_REPORT_TO} 로 보내 주세요."
            ))
            .title("서안 버그 신고")
            .show(|_| {});
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn mailto_escape_keeps_only_unreserved() {
        assert_eq!(super::mailto_escape("aZ9-_.~"), "aZ9-_.~");
        assert_eq!(super::mailto_escape("a b&c\n"), "a%20b%26c%0A");
        assert_eq!(super::mailto_escape("서"), "%EC%84%9C");
    }
}

/// Dock · 로그아웃의 「종료」: macOS 가 보내는 Apple Event(aevt/quit)를 받는 처리기를 우리 것으로 바꿔 끼운다
/// (기본 처리기는 묻지 않고 `terminate:` 로 바로 끝낸다 — 이 판의 tao 는 그 사이에 끼어들 틈을 안 준다).
#[cfg(target_os = "macos")]
mod dock_quit {
    use std::sync::OnceLock;

    use objc2::{
        ClassType, define_class, msg_send,
        rc::Retained,
        runtime::{AnyClass, AnyObject, NSObject},
        sel,
    };

    static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "SeoanQuitHandler"]
        struct QuitHandler;

        impl QuitHandler {
            #[unsafe(method(handleQuit:withReplyEvent:))]
            fn handle_quit(&self, _event: *mut AnyObject, _reply: *mut AnyObject) {
                if let Some(app) = APP.get() {
                    super::quit(app);
                }
            }
        }
    );

    /// 앱이 다 뜬 뒤(AppKit 이 기본 처리기를 깐 뒤) 부른다 · 메인 스레드.
    pub fn install(app: &tauri::AppHandle) {
        if APP.set(app.clone()).is_err() {
            return;
        }
        let Some(manager_class) = AnyClass::get(c"NSAppleEventManager") else {
            return;
        };
        let handler: Retained<QuitHandler> = unsafe { msg_send![QuitHandler::class(), new] };
        let four = |code: &[u8; 4]| u32::from_be_bytes(*code);
        unsafe {
            let manager: *mut AnyObject = msg_send![manager_class, sharedAppleEventManager];
            let _: () = msg_send![
                manager,
                setEventHandler: &*handler,
                andSelector: sel!(handleQuit:withReplyEvent:),
                forEventClass: four(b"aevt"),
                andEventID: four(b"quit")
            ];
        }
        // 이벤트 관리자는 처리기를 붙잡지 않는다 — 앱이 끝날 때까지 산다.
        std::mem::forget(handler);
    }
}

fn main() {
    capabilities::validate_default_capability()
        .expect("checked-in default capability must remain minimal");
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(update::PendingUpdate::default())
        .manage(AppState::default())
        .manage(EditorState::default())
        // 윈도 · 리눅스: 탐색기에서 연 파일(.HLT · .fin · .hlz 더블클릭)은 실행 인자로 온다 — 첫 창이 가져간다.
        // (맥은 Apple Event 로 와서 아래 `RunEvent::Opened` 가 받는다. 전엔 윈도에서 더블클릭하면 빈 창만 떴다.)
        .setup(|app| {
            #[cfg(not(target_os = "macos"))]
            if let Some(path) = std::env::args_os()
                .skip(1)
                .map(std::path::PathBuf::from)
                .find(|path| path.is_file())
            {
                use tauri::Manager;
                app.state::<EditorState>().set_pending("main", path);
            }
            #[cfg(target_os = "macos")]
            let _ = app;
            // 배포판만 켤 때 새 판을 찾는다(개발 빌드는 판 번호가 같아 묻지 않는다).
            #[cfg(not(debug_assertions))]
            tauri::async_runtime::spawn(update::check(app.handle().clone(), false));
            Ok(())
        })
        // ⌘Q: macOS 기본 「종료」는 앱을 바로 끝낸다(이 판의 tao 는 끼어들 틈을 안 준다) — 저장을 묻는 종료로 바꾼다.
        .menu(menu::build)
        .on_menu_event(|app, event| {
            if menu::forward(app, event.id().as_ref()) {
                return;
            }
            if event.id() == "quit" {
                quit(app);
            } else if event.id() == "check-update" {
                tauri::async_runtime::spawn(update::check(app.clone(), true));
            } else if event.id() == "rollback" {
                tauri::async_runtime::spawn(update::rollback(app.clone()));
            } else if event.id() == "report-bug" {
                report_bug(app);
            } else if event.id() == "recent-clear" {
                recent::set_recent(app, Vec::new());
            } else if let Some(index) = event
                .id()
                .as_ref()
                .strip_prefix("recent:")
                .and_then(|n| n.parse().ok())
            {
                open_recent(app, index);
            }
        })
        .invoke_handler(tauri::generate_handler![
            select_hlt,
            verify_file,
            cancel_verify,
            editor_open,
            editor_import,
            editor_import_finish,
            editor_cp949_scan,
            editor_new_opinion,
            editor_open_pending,
            editor_page,
            editor_image,
            editor_replace_text,
            editor_replace_text_page_break,
            editor_set_alignment,
            editor_set_indent,
            editor_merge_paragraph,
            editor_new_tab,
            editor_close,
            editor_close_document,
            editor_save,
            editor_delete_range,
            editor_log,
            editor_insert_figure,
            editor_insert_picture,
            editor_replace_picture,
            editor_read_review,
            editor_apply_review,
            editor_amendment_marks,
            editor_amendment_basis,
            editor_claim_issues,
            editor_claim_parents,
            editor_compare_pick,
            editor_autosave,
            editor_batch_generate,
            editor_batch_check,
            editor_batch_stop,
            editor_save_csv,
            editor_source_open,
            editor_paragraph_keys,
            editor_renumber_plan,
            editor_replace_texts,
            editor::editor_fix_double_spaces,
            editor_sign_table,
            editor_submission_checks,
            editor_resize_image,
            editor_insert_table,
            editor_table_edit,
            editor_delete_table,
            editor_cell_position,
            editor_insert_item,
            editor_toggle_format,
            editor_toggle_format_spans,
            editor_cell_text,
            editor_replace_cell_text,
            editor_undo,
            editor_redo,
            editor_number_paragraphs,
            editor_save_as,
            editor_print_pdf,
            editor_export_docx,
            editor_export_fin,
            editor_export_amendment,
            editor_remove_items,
            editor_set_size,
            editor_find,
            editor_view,
            editor_replace,
            editor_open_read_only,
            editor_history_list,
            editor_history_open,
            settings::settings_get,
            settings::settings_set
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Seoan editor")
        .run(|app, event| {
            use tauri::Manager;
            let state = app.state::<EditorState>();
            match event {
                // 문서가 열린 창 닫기: 바로 닫지 않고 화면에 알린다 — 화면이 친 글을 보내고 `editor_close` 가 저장을 묻는다.
                tauri::RunEvent::WindowEvent {
                    label,
                    event: tauri::WindowEvent::CloseRequested { api, .. },
                    ..
                } if state.open_windows().contains(&label) => {
                    api.prevent_close();
                    ask_close(app, &label);
                }
                // 탭(창)을 닫으면 그 문서를 놓는다 · 앱을 끝내는 중이면 다음 문서 창을 묻는다(없으면 끝낸다).
                tauri::RunEvent::WindowEvent {
                    label,
                    event: tauri::WindowEvent::Destroyed,
                    ..
                } => {
                    state.forget(&label);
                    recovery::discard(app, &label);
                    recent::changed(app, None);
                    if state.quitting.load(std::sync::atomic::Ordering::SeqCst) {
                        match state.open_windows().first() {
                            Some(next) => ask_close(app, next),
                            None => app.exit(0),
                        }
                    }
                }
                // 마지막 창이 닫히며 오는 끝내기 요청: 문서 창이 남아 있으면(아직 묻지 않은 창) 묻는 쪽으로.
                tauri::RunEvent::ExitRequested { api, code, .. }
                    if code.is_none() && !state.open_windows().is_empty() =>
                {
                    api.prevent_exit();
                    quit(app);
                }
                // Finder 에서 연 파일(.HLT · .fin · .hlz 더블클릭): 빈 첫 창이면 거기 · 아니면 새 탭.
                // 실행 직후면 첫 창 화면이 뜬 뒤 가져간다(`editor_open_pending`).
                #[cfg(target_os = "macos")]
                tauri::RunEvent::Opened { urls } => {
                    use tauri::Emitter;
                    for path in urls.iter().filter_map(|url| url.to_file_path().ok()) {
                        open_in_app(app, path);
                    }
                    let _ = app.emit("open-file", ());
                }
                tauri::RunEvent::Exit => update::install_on_exit(app),
                tauri::RunEvent::Ready => {
                    state.ready.store(true, std::sync::atomic::Ordering::SeqCst);
                    recent::fill_menu(app, &recent::read(app).recent);
                    // 판 기록(「이전 판으로 되돌리기」의 대상) · 오래된 이전 판 치우기.
                    let version = app.package_info().version.to_string();
                    let _ = settings::update(app, |settings| {
                        settings.record_version(&version);
                    });
                    if let Some(root) = history::root(app) {
                        history::prune_older_than(
                            &root,
                            settings::read(app).history_days,
                            chrono::Local::now().naive_local(),
                        );
                    }
                    restore_tabs(app);
                    offer_recovery(app);
                    #[cfg(target_os = "macos")]
                    dock_quit::install(app);
                }
                _ => {}
            }
        });
}
