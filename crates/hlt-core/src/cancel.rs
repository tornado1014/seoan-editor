use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProgressTotals {
    pub compressed_bytes: u64,
    pub inflated_bytes: u64,
    pub items: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProgressEvent {
    pub request_id: String,
    pub phase: String,
    pub totals: ProgressTotals,
    pub fraction: f64,
}

pub type ProgressSink = Arc<dyn Fn(ProgressEvent) + Send + Sync + 'static>;

pub struct VerifyOptions {
    pub request_id: String,
    pub max_inflated_bytes: u64,
    /// `false` 인데 선언된 해제 크기가 `large_threshold_bytes` 를 넘으면 풀기 전에 `LargeDocument`.
    pub allow_large: bool,
    pub large_threshold_bytes: u64,
    pub cancellation: CancellationToken,
    pub progress: Option<ProgressSink>,
}

impl Default for VerifyOptions {
    fn default() -> Self {
        Self {
            request_id: String::new(),
            max_inflated_bytes: 4 * 1024 * 1024 * 1024,
            allow_large: allow_large_default(),
            large_threshold_bytes: large_threshold_default(),
            cancellation: CancellationToken::new(),
            progress: None,
        }
    }
}

/// 「큰 문서」 문턱 — 선언된 해제 크기가 이보다 크면 허락 없이는 풀지 않는다.
pub const LARGE_THRESHOLD_BYTES: u64 = 1024 * 1024 * 1024;

static ALLOW_LARGE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 사용자(또는 호출자)가 허락한 문서 경로(또는 폴더 — 그 아래 전부). 정규화한 경로.
static ALLOWED_PATHS: std::sync::Mutex<Vec<std::path::PathBuf>> = std::sync::Mutex::new(Vec::new());

/// 정규화(실패하면 준 경로 그대로 — 사라진 파일도 죽지 않는다).
fn canonical(path: &std::path::Path) -> std::path::PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// 이 문서(폴더면 그 아래 전부)를 큰 문서로도 열도록 허락한다 — 데스크톱 「열기」. 다른 문서 · 일괄 처리에는 새지 않는다.
// ponytail: 허락은 프로세스가 끝날 때까지 남는다(임시 세션 폴더 항목이 쌓이는 양은 무시할 만하다).
pub fn allow_large_path(path: &std::path::Path) {
    ALLOWED_PATHS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(canonical(path));
}

/// 허락된 문서(이거나 허락된 폴더 아래)인가.
pub fn path_allowed(path: &std::path::Path) -> bool {
    let set = ALLOWED_PATHS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    !set.is_empty()
        && canonical(path)
            .ancestors()
            .any(|ancestor| set.iter().any(|allowed| allowed == ancestor))
}

/// 프로세스 전체 허락(CLI `--allow-large` 전용 — 데스크톱은 쓰지 않는다). 이미 만든 옵션은 바뀌지 않는다.
pub fn set_allow_large(allow: bool) {
    ALLOW_LARGE.store(allow, std::sync::atomic::Ordering::SeqCst);
}

/// 옵션의 `allow_large` 기본값 = 프로세스 허락 또는 환경변수 `SEOAN_ALLOW_LARGE=1`.
pub fn allow_large_default() -> bool {
    ALLOW_LARGE.load(std::sync::atomic::Ordering::SeqCst)
        || std::env::var("SEOAN_ALLOW_LARGE").is_ok_and(|value| value == "1")
}

/// 문턱 기본값 = `LARGE_THRESHOLD_BYTES`. 환경변수 `SEOAN_LARGE_THRESHOLD_BYTES`(테스트 · 진단용)가 있으면 그것(0 · 읽을 수 없는 값은 무시).
pub fn large_threshold_default() -> u64 {
    std::env::var("SEOAN_LARGE_THRESHOLD_BYTES")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|bytes| *bytes > 0)
        .unwrap_or(LARGE_THRESHOLD_BYTES)
}

/// 풀기 전 확인(안내일 뿐 보안 경계가 아니다 — 한계는 하드 상한과 디스크 여유 확인이다): 선언된 해제 크기가 문턱을 넘고 허락이 없으면 `LargeDocument`.
pub(crate) fn check_large(
    declared_bytes: u64,
    allow: bool,
    threshold: u64,
) -> Result<(), crate::CoreError> {
    if allow || declared_bytes <= threshold {
        return Ok(());
    }
    let mut error = crate::CoreError::new(crate::ErrorCode::LargeDocument, "large_document", false);
    error.message = format!(
        "이 문서는 풀면 {:.1}GB 입니다(디스크와 메모리를 그만큼 씁니다). 열까요?",
        declared_bytes as f64 / 1_000_000_000.0
    );
    error
        .details
        .insert("inflated_bytes".into(), declared_bytes.to_string());
    Err(error)
}
