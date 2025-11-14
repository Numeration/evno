use crate::event::Event;

#[trait_variant::make(Send)]
pub trait Emit: Send + Sync {
    async fn emit<E: Event>(&self, event: E);
}

#[trait_variant::make(Send)]
pub trait TypedEmit: Send + Sync {
    type Event: Event;

    async fn emit(&self, event: Self::Event);
}

#[trait_variant::make(Send)]
pub trait Drain {
    async fn drain(self);
}

#[trait_variant::make(Send)]
pub trait Close {
    async fn close(self);
}
