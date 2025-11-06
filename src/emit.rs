use crate::emitter::Emitter;
use crate::event::Event;
use std::any::Any;

pub trait EmitterProxy: Send + Sync + 'static {
    fn as_any(&self) -> &dyn Any;
}

pub trait AsEmitter: Send + Sync {
    fn as_emitter<E: Event>(&self) -> &Emitter<E>;
}

impl AsEmitter for &dyn EmitterProxy {
    fn as_emitter<E: Event>(&self) -> &Emitter<E> {
        self.as_any().downcast_ref::<Emitter<E>>().unwrap()
    }
}

pub trait ToEmitter: Send + Sync {
    fn to_emitter<E: Event>(&self) -> Emitter<E>;
}

impl ToEmitter for &dyn EmitterProxy {
    fn to_emitter<E: Event>(&self) -> Emitter<E> {
        self.as_any().downcast_ref::<Emitter<E>>().unwrap().clone()
    }
}

#[trait_variant::make(Send)]
pub trait Emit: Send + Sync {
    async fn emit<E: Event>(&self, event: E);
}