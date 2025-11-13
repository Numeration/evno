use crossbeam_utils::CachePadded;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::sync::Notify;

#[derive(Debug, Default)]
struct Inner {
    count: CachePadded<AtomicI64>,
    notify: Notify,
}

impl WaitGroup {
    pub fn add(&self) -> GroupGuard {
        self.inner.count.fetch_add(1, Ordering::Relaxed);
        GroupGuard(self.inner.clone())
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

#[derive(Debug, Default, Clone)]
pub struct WaitGroup {
    inner: Arc<Inner>,
}

pub struct GroupGuard(Arc<Inner>);

impl Drop for GroupGuard {
    fn drop(&mut self) {
        if self.0.count.fetch_sub(1, Ordering::Release) == 1 {
            self.0.notify.notify_waiters();
        }
    }
}
