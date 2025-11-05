use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use crossbeam_utils::CachePadded;
use tokio::sync::Notify;

#[derive(Debug, Default, Clone)]
pub struct Latch {
    inner: Arc<LatchInner>,
}

#[derive(Debug, Default)]
struct LatchInner {
    count: CachePadded<AtomicI64>,
    notify: Notify,
}

impl Latch {
    pub fn acquire(&self) -> Guard {
        self.inner.count.fetch_add(1, Ordering::Relaxed);
        Guard(self.inner.clone())
    }

    pub async fn wait(&self) {
        loop {
            if self.inner.count.load(Ordering::Acquire) == 0 {
                break;
            }

            let notified = self.inner.notify.notified();

            if self.inner.count.load(Ordering::Acquire) == 0 {
                break;
            }

            notified.await;
        }
    }
}

pub struct Guard(Arc<LatchInner>);

impl Drop for Guard {
    fn drop(&mut self) {
        if self.0.count.fetch_sub(1, Ordering::Release) == 1 {
            self.0.notify.notify_waiters();
        }
    }
}
