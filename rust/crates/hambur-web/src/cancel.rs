//! Cooperative cancellation for one web operation.
//!
//! The runtime cancels turns through a shared `AtomicBool`; the seam forwards a
//! [`CancelToken`] into every provider so a long HTTP call stops with the turn instead of
//! running to its own timeout.

use std::future::{Future, poll_fn};
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Poll;
use std::time::Duration;

use crate::types::WebError;

/// How often an in-flight operation re-checks the shared cancel flag.
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(120);

/// A cloneable view of the turn's cancellation flag. `never()` is a token that cannot fire,
/// used by callers outside a turn (tests, one-shot probes).
#[derive(Clone, Default)]
pub struct CancelToken {
    flag: Option<Arc<AtomicBool>>,
}

impl CancelToken {
    pub fn new(flag: Arc<AtomicBool>) -> Self {
        Self { flag: Some(flag) }
    }

    /// A token that never fires.
    pub fn never() -> Self {
        Self { flag: None }
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::SeqCst))
    }

    /// The stable cancellation error for this token.
    pub fn error(&self) -> WebError {
        WebError::aborted()
    }
}

/// Run `future` to completion unless `cancel` fires first, in which case the future is
/// dropped and [`WebError`] `WEB_ABORTED` is returned.
///
/// Written with `poll_fn` rather than `tokio::select!` so the crate needs no tokio macro
/// feature and the cancellation latency stays independent of the underlying transport.
pub async fn race_cancel<F: Future>(
    future: F,
    cancel: &CancelToken,
) -> Result<F::Output, WebError> {
    if cancel.is_cancelled() {
        return Err(cancel.error());
    }
    let mut future = pin!(future);
    let mut ticker = pin!(tokio::time::sleep(CANCEL_POLL_INTERVAL));
    poll_fn(move |context| {
        if cancel.is_cancelled() {
            return Poll::Ready(Err(cancel.error()));
        }
        if let Poll::Ready(value) = future.as_mut().poll(context) {
            return Poll::Ready(Ok(value));
        }
        if ticker.as_mut().poll(context).is_ready() {
            ticker.set(tokio::time::sleep(CANCEL_POLL_INTERVAL));
            // Poll the fresh timer once so its waker is registered before we park.
            let _ = ticker.as_mut().poll(context);
        }
        Poll::Pending
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::block_on;
    use std::sync::atomic::AtomicBool;
    use std::time::Instant;

    #[test]
    fn returns_the_value_when_not_cancelled() {
        let value = block_on(async {
            let cancel = CancelToken::never();
            race_cancel(async { 7_u32 }, &cancel).await.expect("value")
        });
        assert_eq!(value, 7);
    }

    #[test]
    fn aborts_a_pending_future_when_the_flag_fires() {
        block_on(async {
            let flag = Arc::new(AtomicBool::new(false));
            let cancel = CancelToken::new(flag.clone());
            let trigger = tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(30)).await;
                flag.store(true, Ordering::SeqCst);
            });
            let started = Instant::now();
            let error = race_cancel(
                async {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    "late"
                },
                &cancel,
            )
            .await
            .expect_err("aborted");
            assert_eq!(error.code(), crate::types::WEB_ABORTED);
            assert!(started.elapsed() < Duration::from_secs(5));
            let _ = trigger.await;
        });
    }
}
