use crate::emit::EmitterProxy;
use crate::emit_barrier;
use crate::event::Event;
use std::any::Any;
use std::sync::Arc;

#[derive(Debug)]
pub struct Emitter<T> {
    publisher: gyre::Publisher<T>,
    emit_barrier: Arc<emit_barrier::Lock>,
}

impl<T: Event> Emitter<T> {
    pub(crate) fn new(capacity: usize, lock: Arc<emit_barrier::Lock>) -> Self {
        let (publisher, _) = gyre::channel(capacity);
        Self {
            publisher,
            emit_barrier: lock,
        }
    }

    pub async fn subscribe(&self) -> gyre::Consumer<T> {
        self.publisher.subscribe().await
    }

    pub fn subscribe_owned(&self) -> gyre::OwnedSubscribe<T> {
        self.publisher.subscribe_owned()
    }

    pub async fn emit(&self, event: T) {
        self.emit_barrier.until_released().await;
        self.publisher.publish(event).await;
    }
}

impl<T> Clone for Emitter<T> {
    fn clone(&self) -> Self {
        Self {
            publisher: self.publisher.clone(),
            emit_barrier: self.emit_barrier.clone(),
        }
    }
}

impl<E: Event> EmitterProxy for Emitter<E> {
    fn as_any(&self) -> &dyn Any {
        self
    }
}
