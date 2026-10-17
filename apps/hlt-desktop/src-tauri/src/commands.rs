use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use hlt_core::{
    CancellationToken, CoreError, ErrorCode, ProgressEvent, ProgressSink, VerifyOptions,
    VerifyResult, verify_path,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

#[derive(Default)]
pub struct AppState {
    pub handles: Mutex<HashMap<String, PathBuf>>,
    pub cancellations: Mutex<HashMap<String, CancellationToken>>,
}

#[derive(Serialize, Clone)]
pub struct SelectedFileDto {
    pub file_handle: String,
    pub display_name: String,
}

#[derive(Deserialize)]
pub struct VerifyRequestDto {
    pub request_id: String,
    pub file_handle: String,
}

pub type CommandError = CoreError;

#[derive(Serialize, Clone)]
pub struct ProgressEventDto {
    pub request_id: String,
    pub phase: String,
    pub compressed_bytes: String,
    pub inflated_bytes: String,
    pub items: String,
    pub fraction: f64,
}

#[tauri::command]
pub async fn select_hlt(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<SelectedFileDto>, CommandError> {
    let Some(selected) = app
        .dialog()
        .file()
        .add_filter("HAN Lite Document", &["HLT", "hlt"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let path = selected
        .into_path()
        .map_err(|_| CoreError::new(ErrorCode::PathNotAllowed, "select", false))?;
    let display_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CoreError::new(ErrorCode::PathNotAllowed, "select", false))?
        .to_string();
    let file_handle = uuid::Uuid::new_v4().to_string();
    lock(&state.handles, "select")?.insert(file_handle.clone(), path);
    Ok(Some(SelectedFileDto {
        file_handle,
        display_name,
    }))
}

#[tauri::command]
pub async fn verify_file(
    app: AppHandle,
    state: State<'_, AppState>,
    request: VerifyRequestDto,
) -> Result<VerifyResult, CommandError> {
    let path = lock(&state.handles, "handle")?
        .get(&request.file_handle)
        .cloned()
        .ok_or_else(|| CoreError::new(ErrorCode::PathNotAllowed, "handle", false))?;
    let cancellation = CancellationToken::new();
    lock(&state.cancellations, "cancel")?.insert(request.request_id.clone(), cancellation.clone());

    let request_id = request.request_id.clone();
    let progress_app = app.clone();
    let progress: ProgressSink = Arc::new(move |event: ProgressEvent| {
        let _ = progress_app.emit(
            "verify-progress",
            ProgressEventDto {
                request_id: event.request_id,
                phase: event.phase,
                compressed_bytes: event.totals.compressed_bytes.to_string(),
                inflated_bytes: event.totals.inflated_bytes.to_string(),
                items: event.totals.items.to_string(),
                fraction: event.fraction,
            },
        );
    });
    let options = VerifyOptions {
        request_id: request.request_id,
        cancellation,
        progress: Some(progress),
        ..Default::default()
    };

    let verification =
        tauri::async_runtime::spawn_blocking(move || verify_path(&path, options)).await;
    lock(&state.cancellations, "cancel")?.remove(&request_id);
    verification.map_err(|error| {
        let mut core_error = CoreError::new(ErrorCode::VerificationFailed, "worker", false);
        core_error
            .details
            .insert("join_error".to_string(), error.to_string());
        core_error
    })?
}

#[tauri::command]
pub fn cancel_verify(state: State<'_, AppState>, request_id: String) -> Result<(), CommandError> {
    if let Some(cancellation) = lock(&state.cancellations, "cancel")?.get(&request_id) {
        cancellation.cancel();
    }
    Ok(())
}

fn lock<'a, T>(
    mutex: &'a Mutex<T>,
    phase: &str,
) -> Result<std::sync::MutexGuard<'a, T>, CoreError> {
    mutex
        .lock()
        .map_err(|_| CoreError::new(ErrorCode::VerificationFailed, phase, false))
}
