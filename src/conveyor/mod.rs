mod with_processor;

use crate::event::Event;
use crate::processor::Processor;
use crate::{Emit, ToEmitter};

pub use with_processor::*;

pub trait Conveyor: Send + Sync {
    type Emit: Emit;

    type Processor: Processor;

    fn emit(&self) -> &Self::Emit;

    fn processor(&self) -> Self::Processor;
}

impl<T, U, P> Emit for T
where
    T: Conveyor<Emit = U, Processor = P>,
    U: Emit,
    P: Processor,
{
    async fn emit<E: Event>(&self, event: E) {
        let event = self.processor().clone().process(event).await;
        self.emit().emit(event).await;
    }
}

impl<T, U, P> ToEmitter for T
where
    T: Conveyor<Emit = U, Processor = P>,
    U: ToEmitter,
    P: Processor,
{
    type Emitter<E: Event> = WithProcessor<E, U::Emitter<P::Event<E>>, P>;

    fn to_emitter<E: Event>(&self) -> Self::Emitter<E> {
        let emitter = self.emit().to_emitter();
        WithProcessor::new(emitter, self.processor().clone())
    }
}
