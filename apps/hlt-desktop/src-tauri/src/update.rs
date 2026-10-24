//! 앱 업데이트 — GitHub Releases 의 `latest.json`(서명 확인은 플러그인이 한다).
//! 받은 판은 곧바로 깔지 않는다: 보통 종료(문서마다 저장 질문)를 거쳐 `RunEvent::Exit` 에서 깔고 다시 띄운다.
//! 저장 질문에서 취소하면 종료가 멈추고, 받아 둔 판은 다음 종료 때 깔린다.

use std::sync::Mutex;

use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogResult};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<(Update, Vec<u8>)>>);

/// 새 판을 찾아 묻고 받는다. `manual` = 메뉴에서 불렀다(새 판 없음 · 실패도 알린다).
pub async fn check(app: AppHandle, manual: bool) {
    if app.state::<PendingUpdate>().0.lock().unwrap().is_some() {
        if manual {
            tell(
                &app,
                "새 판을 이미 받아 두었습니다. 서안을 끝내면 설치됩니다.",
            );
        }
        return;
    }
    let found = match app.updater() {
        Ok(updater) => updater.check().await,
        Err(error) => Err(error),
    };
    let update = match found {
        Ok(Some(update)) => update,
        Ok(None) => {
            if manual {
                let version = app.package_info().version.to_string();
                tell(&app, &format!("최신판(서안 {version})을 쓰고 있습니다."));
            }
            return;
        }
        Err(error) => {
            if manual {
                tell(&app, &format!("업데이트를 확인하지 못했습니다.\n{error}"));
            }
            return;
        }
    };
    let asked = app
        .dialog()
        .message(format!(
            "서안 {} 이(가) 나왔습니다(지금 {}). 받아서 설치할까요?\n설치 전에 열린 문서를 저장할지 먼저 묻습니다.",
            update.version, update.current_version
        ))
        .title("서안 업데이트")
        .buttons(MessageDialogButtons::OkCancelCustom(
            "설치".into(),
            "나중에".into(),
        ))
        .blocking_show_with_result();
    if !matches!(asked, MessageDialogResult::Ok)
        && !matches!(&asked, MessageDialogResult::Custom(label) if label == "설치")
    {
        return;
    }
    match update.download(|_, _| {}, || {}).await {
        Ok(bytes) => {
            *app.state::<PendingUpdate>().0.lock().unwrap() = Some((update, bytes));
            super::quit(&app);
        }
        Err(error) => tell(&app, &format!("업데이트를 받지 못했습니다.\n{error}")),
    }
}

/// 앱이 끝나는 마지막 순간: 받아 둔 판이 있으면 깔고 다시 띄운다(윈도는 설치기가 앱을 다시 띄운다).
pub fn install_on_exit(app: &AppHandle) {
    let Some((update, bytes)) = app.state::<PendingUpdate>().0.lock().unwrap().take() else {
        return;
    };
    match update.install(bytes) {
        Ok(()) => tauri::process::restart(&app.env()),
        // 메인 스레드라 막는 대화상자를 못 띄운다 — 다음 실행 때 다시 찾아 묻는다.
        Err(error) => eprintln!("update install failed: {error}"),
    }
}

/// 그 판 릴리스에 붙은 `latest.json`(판마다 남는다 — 「최신」 주소와 달리 되돌릴 판을 고를 수 있다).
pub fn release_manifest(version: &str) -> String {
    format!("https://github.com/tornado1014/seoan-editor/releases/download/v{version}/latest.json")
}

/// 「이전 판으로 되돌리기」: 지난번에 쓰던 판(설정 `previous_version`)을 받아 두고, 끝낼 때 까는 것은 업데이트와 같다.
/// 서명 확인도 같은 플러그인이 한다(되돌린 판도 우리 키로 서명된 것만 깔린다).
pub async fn rollback(app: AppHandle) {
    if app.state::<PendingUpdate>().0.lock().unwrap().is_some() {
        tell(
            &app,
            "받아 둔 판이 이미 있습니다. 서안을 끝내면 설치됩니다.",
        );
        return;
    }
    let current = app.package_info().version.to_string();
    let Some(target) = crate::settings::read(&app)
        .previous_version
        .filter(|version| *version != current)
    else {
        tell(
            &app,
            "되돌릴 이전 판을 모릅니다. 이 기능은 서안이 판을 한 번 바꾼 뒤부터 쓸 수 있습니다.",
        );
        return;
    };
    let asked = app
        .dialog()
        .message(format!(
            "서안 {target}(으)로 되돌릴까요? (지금 {current})\n받아 두었다가 서안을 끝내면 깔립니다. 끝내기 전에 열린 문서를 저장할지 먼저 묻습니다."
        ))
        .title("이전 판으로 되돌리기")
        .buttons(MessageDialogButtons::OkCancelCustom(
            "되돌리기".into(),
            "취소".into(),
        ))
        .blocking_show_with_result();
    if !matches!(asked, MessageDialogResult::Ok)
        && !matches!(&asked, MessageDialogResult::Custom(label) if label == "되돌리기")
    {
        return;
    }
    let wanted = target.clone();
    let found = match release_manifest(&target).parse::<tauri::Url>() {
        Ok(url) => match app.updater_builder().endpoints(vec![url]) {
            Ok(builder) => match builder
                .version_comparator(move |_, remote| remote.version.to_string() == wanted)
                .build()
            {
                Ok(updater) => updater.check().await,
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        },
        Err(error) => {
            tell(&app, &format!("되돌릴 판을 찾지 못했습니다.\n{error}"));
            return;
        }
    };
    let update = match found {
        Ok(Some(update)) => update,
        Ok(None) => {
            tell(&app, &format!("서안 {target} 설치 파일을 찾지 못했습니다."));
            return;
        }
        Err(error) => {
            tell(&app, &format!("되돌릴 판을 찾지 못했습니다.\n{error}"));
            return;
        }
    };
    match update.download(|_, _| {}, || {}).await {
        Ok(bytes) => {
            *app.state::<PendingUpdate>().0.lock().unwrap() = Some((update, bytes));
            super::quit(&app);
        }
        Err(error) => tell(&app, &format!("이전 판을 받지 못했습니다.\n{error}")),
    }
}

fn tell(app: &AppHandle, text: &str) {
    app.dialog()
        .message(text)
        .title("서안 업데이트")
        .blocking_show();
}

#[cfg(test)]
mod tests {
    #[test]
    fn release_manifest_points_at_that_version() {
        assert_eq!(
            super::release_manifest("0.6.30"),
            "https://github.com/tornado1014/seoan-editor/releases/download/v0.6.30/latest.json"
        );
    }
}
