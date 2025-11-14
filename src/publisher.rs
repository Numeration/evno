use crate::event::Event;
use crate::{EmitterProxy, TypedEmit, bind_latch};
use std::any::Any;
use std::sync::Arc;

pub struct Publisher<E> {
    publisher: gyre::Publisher<E>,
    emit_barrier: Arc<bind_latch::BindLatch>,
}

impl<T: Event> Publisher<T> {
    pub(crate) fn new(capacity: usize, lock: Arc<bind_latch::BindLatch>) -> Self {
        let (publisher, _) = gyre::channel(capacity);
        Self {
            publisher,
            emit_barrier: lock,
        }
    }

    pub async fn subscribe(&self) -> gyre::Consumer<T> {
        self.publisher.subscribe().await
    }
}

impl<T: Event> TypedEmit for Publisher<T> {
    type Event = T;

    #[inline]
    async fn emit(&self, event: Self::Event) {
        self.emit_barrier.wait_for_binds().await;
        self.publisher.publish(event).await.unwrap_or(());
    }
}

impl<T> Clone for Publisher<T> {
    fn clone(&self) -> Self {
        Self {
            publisher: self.publisher.clone(),
            emit_barrier: self.emit_barrier.clone(),
        }
    }
}

impl<E: Event> EmitterProxy for Publisher<E> {
    fn as_any(&self) -> &dyn Any {
        self
    }
}
