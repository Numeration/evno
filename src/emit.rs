use crate::emitter::Emitter;
use crate::event::Event;
use std::any::Any;

pub trait EmitterProxy: Send + Sync + 'static {
    fn as_any(&self) -> &dyn Any;
}

pub trait AsEmitter {
    fn as_emitter<E: Event>(&self) -> &Emitter<E>;
}

impl AsEmitter for &dyn EmitterProxy {
    fn as_emitter<E: Event>(&self) -> &Emitter<E> {
        self.as_any().downcast_ref::<Emitter<E>>().unwrap()
    }
}
