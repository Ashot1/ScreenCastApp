//! One cancel-aware wait for every providers transport. Core owns the flag and sets it over FFI,
//! so nothing can wake on it: the pending future is polled in 25 ms slices instead.
use std::{
    future::Future,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stop {
    Cancelled,
    Timeout,
    Failed,
}
pub(crate) async fn waiting<T>(
    future: impl Future<Output = Result<T, reqwest::Error>>,
    flag: &AtomicBool,
    deadline: Instant,
) -> Result<T, Stop> {
    let mut pending = std::pin::pin!(future);
    loop {
        if flag.load(Ordering::Acquire) {
            return Err(Stop::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(Stop::Timeout);
        }
        match tokio::time::timeout(Duration::from_millis(25), &mut pending).await {
            Ok(Ok(value)) => return Ok(value),
            Ok(Err(e)) if e.is_timeout() => return Err(Stop::Timeout),
            Ok(Err(_)) => return Err(Stop::Failed),
            Err(_) => {}
        }
    }
}
