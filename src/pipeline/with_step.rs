use crate::TypedEmit;
use crate::event::Event;
use crate::pipeline::step::Step;
use std::marker::PhantomData;

pub struct WithStep<E, T, U> {
    emitter: T,
    step: U,
    _phantom: PhantomData<E>,
}

impl<E, T, U> WithStep<E, T, U> {
    pub fn new(emitter: T, step: U) -> Self {
        Self {
            emitter,
            step,
            _phantom: PhantomData,
        }
    }
}

impl<E, T: Clone, U: Clone> Clone for WithStep<E, T, U> {
    fn clone(&self) -> Self {
        Self::new(self.emitter.clone(), self.step.clone())
    }
}

impl<E, T, U> TypedEmit for WithStep<E, T, U>
where
    E: Event,
    T: TypedEmit<Event = U::Event<E>>,
    U: Step,
{
    type Event = E;

    async fn emit(&self, event: E) {
        let event = self.step.clone().process(event).await;
        self.emitter.emit(event).await;
    }
}
