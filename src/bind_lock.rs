use crossbeam_utils::CachePadded;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct BindGuard(Arc<BindLock>);

impl Drop for BindGuard {
    fn drop(&mut self) {
        self.0.counter.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug, Default)]
pub struct BindLock {
    counter: CachePadded<AtomicUsize>,
}

impl BindLock {
    pub fn lock(self: &Arc<Self>) -> BindGuard {
        self.counter.fetch_add(1, Ordering::AcqRel);
        BindGuard(Arc::clone(self))
    }

    pub async fn wait_for_binds(&self) {
        while self.counter.load(Ordering::Relaxed) != 0 {
            tokio::task::yield_now().await;
        }
    }
}
