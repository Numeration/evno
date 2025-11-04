use crate::emit::{AsEmitter, EmitterProxy};
use crate::event::Event;
use crate::handle::SubscribeHandle;
use crate::launcher::Launcher;
use crate::{Emitter, Listener, ListenerActor, emit_barrier};
use acty::ActorExt;
use std::any::TypeId;
use std::ops::Deref;
use std::sync::Arc;

struct Inner {
    capacity: usize,
    emitters: papaya::HashMap<TypeId, Box<dyn EmitterProxy>>,
    emit_barrier: Arc<emit_barrier::Lock>,
}

impl Inner {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            emitters: Default::default(),
            emit_barrier: Default::default(),
        }
    }

    fn get_emitter_proxy<'guard, E: Event>(
        &self,
        emitters_guard: &'guard papaya::LocalGuard<'_>,
    ) -> &'guard dyn EmitterProxy {
        self.emitters
            .get_or_insert_with(
                TypeId::of::<E>(),
                || Box::new(Emitter::<E>::new(self.capacity, self.emit_barrier.clone())),
                emitters_guard,
            )
            .deref()
    }
}

#[derive(Clone)]
pub struct Bus {
    inner: Arc<Inner>,
}

impl Bus {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Inner::new(capacity)),
        }
    }

    pub fn bind<E: Event>(&self, listener: impl Listener<Event = E>) -> SubscribeHandle {
        let emit_guard = self.inner.emit_barrier.acquire_owned();
        let emitters_guard = self.inner.emitters.guard();
        let emitter_proxy = self.inner.get_emitter_proxy::<E>(&emitters_guard);

        let cancel = tokio_util::sync::CancellationToken::new();

        let actor = ListenerActor(listener, cancel.clone());
        let launcher = Launcher(emitter_proxy.as_emitter::<E>().clone(), emit_guard);
        let join = actor.with(launcher);

        SubscribeHandle::new(cancel, join)
    }

    pub async fn emit<E: Event>(&self, event: E) {
        let emitters_guard = self.inner.emitters.guard();
        let emitter_proxy = self.inner.get_emitter_proxy::<E>(&emitters_guard);

        emitter_proxy.as_emitter().emit(event).await;
    }

    pub fn emitter<E: Event>(&self) -> Emitter<E> {
        let emitters_guard = self.inner.emitters.guard();
        let emitter_proxy = self.inner.get_emitter_proxy::<E>(&emitters_guard);

        emitter_proxy.as_emitter().clone()
    }
}
