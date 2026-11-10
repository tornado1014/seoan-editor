//! 큰 문서 확인: 문서를 여는 모든 길(열기 · 놓기 · 최근 · 이전 판 · 비교 기준)이 이 한 곳을 지난다.

use hlt_core::{CoreError, ErrorCode};
use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

/// 묻는 글 — 풀 크기(GB)는 오류가 실어 온 해제 바이트에서.
pub fn question(error: &CoreError) -> String {
    let gb = error
        .details
        .get("inflated_bytes")
        .and_then(|bytes| bytes.parse::<f64>().ok())
        .unwrap_or_default()
        / 1_000_000_000.0;
    format!("이 문서는 풀면 {gb:.1}GB 입니다(디스크와 메모리를 그만큼 씁니다). 열까요?")
}

/// `open` 이 `LargeDocument` 로 거절되면 `ask` 로 묻는다 — 「열기」면 `allow` 를 부른 뒤 한 번 더 열고,
/// 「취소」면 `Ok(None)`(아무것도 바꾸지 않는다). 다른 오류는 묻지 않고 그대로 낸다.
pub fn retry_if_large<T>(
    mut open: impl FnMut() -> Result<T, CoreError>,
    ask: impl FnOnce(&CoreError) -> bool,
    allow: impl FnOnce(),
) -> Result<Option<T>, CoreError> {
    match open() {
        Err(error) if error.code == ErrorCode::LargeDocument => {
            if !ask(&error) {
                return Ok(None);
            }
            allow();
            open().map(Some)
        }
        other => other.map(Some),
    }
}

/// 묻는 창(막는 호출 — 일꾼 스레드에서 부른다).
pub fn ask_dialog(app: &AppHandle, error: &CoreError) -> bool {
    app.dialog()
        .message(question(error))
        .title("서안 통특기")
        .buttons(MessageDialogButtons::OkCancelCustom(
            "열기".into(),
            "취소".into(),
        ))
        .blocking_show()
}

/// 열기를 시도하고, 큰 문서면 묻는다. 허락은 이 프로그램이 도는 동안 유지된다(세션이 문서를 다시 읽는다).
/// 취소면 `Ok(None)`.
pub fn open_or_ask<T>(
    app: &AppHandle,
    open: impl FnMut() -> Result<T, CoreError>,
) -> Result<Option<T>, CoreError> {
    retry_if_large(
        open,
        |error| ask_dialog(app, error),
        || hlt_core::set_allow_large(true),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use hlt_core::{CoreError, ErrorCode};

    fn large(bytes: u64) -> CoreError {
        let mut error = CoreError::new(ErrorCode::LargeDocument, "large_document", false);
        error
            .details
            .insert("inflated_bytes".into(), bytes.to_string());
        error
    }

    #[test]
    fn question_names_the_size_in_gb() {
        assert_eq!(
            question(&large(3_400_000_000)),
            "이 문서는 풀면 3.4GB 입니다(디스크와 메모리를 그만큼 씁니다). 열까요?"
        );
    }

    #[test]
    fn cancel_asks_once_and_changes_nothing() {
        let mut opens = 0;
        let mut allowed = false;
        let result = retry_if_large(
            || {
                opens += 1;
                Err::<u8, _>(large(2_000_000_000))
            },
            |_| false,
            || allowed = true,
        );
        assert!(matches!(result, Ok(None)));
        assert_eq!(opens, 1);
        assert!(!allowed);
    }

    #[test]
    fn open_allows_then_opens_again() {
        let mut opens = 0;
        let allowed = std::cell::Cell::new(false);
        let result = retry_if_large(
            || {
                opens += 1;
                if allowed.get() {
                    Ok(7_u8)
                } else {
                    Err(large(2_000_000_000))
                }
            },
            |_| true,
            || allowed.set(true),
        );
        assert!(matches!(result, Ok(Some(7))));
        assert_eq!(opens, 2);
    }

    #[test]
    fn other_errors_pass_through_without_asking() {
        let result = retry_if_large(
            || Err::<u8, _>(CoreError::new(ErrorCode::NotHlt, "open", false)),
            |_| panic!("묻지 않는다"),
            || panic!("허락하지 않는다"),
        );
        assert_eq!(result.unwrap_err().code, ErrorCode::NotHlt);
    }
}
