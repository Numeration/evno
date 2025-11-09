use crate::TypedEmit;
use crate::event::Event;
use crate::publisher::Publisher;
use std::any::Any;

pub trait EmitterProxy: Send + Sync + 'static {
    fn as_any(&self) -> &dyn Any;
}

pub trait AsEmitter {
    type Emitter<E: Event>: TypedEmit<Event = E>;

    fn as_emitter<E: Event>(&self) -> &Self::Emitter<E>;
}

impl AsEmitter for &dyn EmitterProxy {
    type Emitter<E: Event> = Publisher<E>;

    fn as_emitter<E: Event>(&self) -> &Self::Emitter<E> {
        self.as_any().downcast_ref::<Publisher<E>>().unwrap()
    }
}

pub trait ToEmitter {
    type Emitter<E: Event>: TypedEmit<Event = E>;

    fn to_emitter<E: Event>(&self) -> Self::Emitter<E>;
}

impl ToEmitter for &dyn EmitterProxy {
    type Emitter<E: Event> = Publisher<E>;

    fn to_emitter<E: Event>(&self) -> Self::Emitter<E> {
        self.as_any()
            .downcast_ref::<Publisher<E>>()
            .unwrap()
            .clone()
    }
}
