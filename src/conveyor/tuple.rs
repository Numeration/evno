use crate::Emit;
use crate::conveyor::Conveyor;
use crate::event::Event;
use crate::processor::Processor;

pub trait State: Clone + Send + Sync + 'static {}

impl<T: Clone + Send + Sync + 'static> State for T {}

impl<T1: State> Processor for (T1,) {
    type Event<E: Event> = (T1, E);

    #[inline]
    async fn process<E: Event>(self, event: E) -> Self::Event<E> {
        (self.0, event)
    }
}

impl<T1: State, U: Emit> Conveyor for (T1, U) {
    type Emit = U;
    type Processor = (T1,);

    fn emit(&self) -> &Self::Emit {
        &self.1
    }

    fn processor(&self) -> Self::Processor {
        (self.0.clone(),)
    }
}

impl<T1: State, T2: State> Processor for (T1, T2) {
    type Event<E: Event> = (T1, T2, E);

    #[inline]
    async fn process<E: Event>(self, event: E) -> Self::Event<E> {
        (self.0, self.1, event)
    }
}

impl<T1: State, T2: State, U: Emit> Conveyor for (T1, T2, U) {
    type Emit = U;
    type Processor = (T1, T2);

    fn emit(&self) -> &Self::Emit {
        &self.2
    }

    fn processor(&self) -> Self::Processor {
        (self.0.clone(), self.1.clone())
    }
}
