use crate::event::Event;

#[trait_variant::make(Send)]
pub trait Step: Clone + Sync {
    type Event<E: Event>: Event;

    async fn process<E: Event>(self, event: E) -> Self::Event<E>;
}
