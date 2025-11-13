mod with_step;
mod step;

use crate::event::Event;
use crate::{Emit, ToEmitter};

pub use with_step::*;
pub use step::*;

pub trait Pipeline: Send + Sync {
    type Emit: Emit;

    type Processor: Step;

    fn emit(&self) -> &Self::Emit;

    fn step(&self) -> Self::Processor;
}

impl<T, U, P> Emit for T
where
    T: Pipeline<Emit = U, Processor = P>,
    U: Emit,
    P: Step,
{
    async fn emit<E: Event>(&self, event: E) {
        let event = self.step().clone().process(event).await;
        self.emit().emit(event).await;
    }
}

impl<T, U, P> ToEmitter for T
where
    T: Pipeline<Emit = U, Processor = P>,
    U: ToEmitter,
    P: Step,
{
    type Emitter<E: Event> = WithStep<E, U::Emitter<P::Event<E>>, P>;

    fn to_emitter<E: Event>(&self) -> Self::Emitter<E> {
        let emitter = self.emit().to_emitter();
        WithStep::new(emitter, self.step().clone())
    }
}
