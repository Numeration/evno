use crossbeam_utils::CachePadded;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct OwnedGuard(Arc<Lock>);

impl Drop for OwnedGuard {
    fn drop(&mut self) {
        self.0.counter.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug, Default)]
pub struct Lock {
    counter: CachePadded<AtomicUsize>,
}

impl Lock {
    pub fn acquire_owned(self: &Arc<Self>) -> OwnedGuard {
        self.counter.fetch_add(1, Ordering::AcqRel);
        OwnedGuard(Arc::clone(self))
    }

    pub async fn until_released(&self) {
        while self.counter.load(Ordering::Relaxed) != 0 {
            tokio::task::yield_now().await;
        }
    }
}
