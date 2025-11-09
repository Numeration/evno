use crate::event::Event;
use crate::handle::SubscribeHandle;
use crate::launcher::Launcher;
use crate::publisher::Publisher;
use crate::{
    AsEmitter, Emit, EmitterProxy, Listener, ListenerActor, ToEmitter, TypedEmit, WithTimes,
    emit_barrier, task,
};
use acty::ActorExt;
use std::any::TypeId;
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

struct Inner {
    capacity: usize,
    emitters: papaya::HashMap<TypeId, Box<dyn EmitterProxy>>,
    emit_barrier: Arc<emit_barrier::Lock>,
    latch: task::Latch,
}

impl Inner {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            emitters: Default::default(),
            emit_barrier: Default::default(),
            latch: Default::default(),
        }
    }

    fn get_emitter_proxy<'guard, E: Event>(
        &self,
        emitters_guard: &'guard papaya::OwnedGuard<'_>,
    ) -> &'guard dyn EmitterProxy {
        self.emitters
            .get_or_insert_with(
                TypeId::of::<E>(),
                || {
                    Box::new(Publisher::<E>::new(
                        self.capacity,
                        self.emit_barrier.clone(),
                    ))
                },
                emitters_guard,
            )
            .deref()
    }
}

#[derive(Default)]
struct BusBarrier(Notify);

#[derive(Clone)]
pub struct Bus {
    inner: Arc<Inner>,
    barrier: Arc<BusBarrier>,
}

impl Bus {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Inner::new(capacity)),
            barrier: Default::default(),
        }
    }

    pub fn bind<E: Event>(&self, listener: impl Listener<Event = E>) -> SubscribeHandle {
        self.bind_cancel(CancellationToken::new(), listener)
    }

    pub fn bind_cancel<E: Event>(
        &self,
        cancel: CancellationToken,
        listener: impl Listener<Event = E>,
    ) -> SubscribeHandle {
        let task_guard = self.inner.latch.acquire();
        let emit_guard = self.inner.emit_barrier.acquire_owned();
        let emitters_guard = self.inner.emitters.owned_guard();
        let emitter = self
            .inner
            .get_emitter_proxy::<E>(&emitters_guard)
            .as_emitter::<E>()
            .clone();

        let actor = ListenerActor(listener, cancel.clone(), task_guard);
        let launcher = Launcher(emitter, emit_guard);
        let join = actor.with(launcher);

        SubscribeHandle::new(cancel, join)
    }

    pub fn on<E: Event>(&self, listener: impl Listener<Event = E>) -> SubscribeHandle {
        self.bind(listener)
    }

    pub fn once<E: Event>(&self, listener: impl Listener<Event = E>) -> SubscribeHandle {
        self.bind(WithTimes::new(1, listener))
    }

    pub fn many<E: Event>(
        &self,
        times: usize,
        listener: impl Listener<Event = E>,
    ) -> SubscribeHandle {
        self.bind(WithTimes::new(times, listener))
    }

    pub async fn drain(self) {
        let latch = self.inner.latch.clone();
        let barrier = self.barrier.clone();
        let notified = barrier.0.notified();
        drop(self);
        latch.wait().await;
        notified.await;
    }
}

impl ToEmitter for Bus {
    type Emitter<E: Event> = Publisher<E>;

    fn to_emitter<E: Event>(&self) -> Publisher<E> {
        let emitters_guard = self.inner.emitters.owned_guard();
        let emitter_proxy = self.inner.get_emitter_proxy::<E>(&emitters_guard);

        emitter_proxy.as_emitter().clone()
    }
}

impl Emit for Bus {
    async fn emit<E: Event>(&self, event: E) {
        let emitters_guard = self.inner.emitters.owned_guard();
        let emitter_proxy = self.inner.get_emitter_proxy::<E>(&emitters_guard);

        emitter_proxy.as_emitter().emit(event).await;
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        if Arc::get_mut(&mut self.inner).is_some() {
            self.barrier.0.notify_waiters();
        }
    }
}
