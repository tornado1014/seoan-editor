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
    pub cancellation: CancellationToken,
    pub progress: Option<ProgressSink>,
}

impl Default for VerifyOptions {
    fn default() -> Self {
        Self {
            request_id: String::new(),
            max_inflated_bytes: 4 * 1024 * 1024 * 1024,
            cancellation: CancellationToken::new(),
            progress: None,
        }
    }
}
