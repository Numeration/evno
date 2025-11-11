use crate::TypedEmit;
use crate::event::Event;
use crate::conveyor::processor::Processor;
use std::marker::PhantomData;

pub struct WithProcessor<E, T, U> {
    publisher: T,
    processor: U,
    _phantom: PhantomData<E>,
}

impl<E, T, U> WithProcessor<E, T, U> {
    pub fn new(publisher: T, processor: U) -> Self {
        Self {
            publisher,
            processor,
            _phantom: PhantomData,
        }
    }
}

impl<E, T: Clone, U: Clone> Clone for WithProcessor<E, T, U> {
    fn clone(&self) -> Self {
        Self::new(self.publisher.clone(), self.processor.clone())
    }
}

impl<E, T, U> TypedEmit for WithProcessor<E, T, U>
where
    E: Event,
    T: TypedEmit<Event = U::Event<E>>,
    U: Processor,
{
    type Event = E;

    async fn emit(&self, event: E) {
        let event = self.processor.clone().process(event).await;
        self.publisher.emit(event).await;
    }
}
