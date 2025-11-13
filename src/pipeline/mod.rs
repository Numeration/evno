mod step;
mod with_step;

use crate::event::Event;
use crate::{Bus, Emit, ToEmitter};

pub use step::*;
pub use with_step::*;

#[derive(Debug, Clone)]
pub struct Pipeline<T, U> {
    emitter: T,
    step: U,
}

impl<T: Emit, U: Step> Pipeline<T, U> {
    pub fn pipe<P: Step>(self, step: P) -> Pipeline<Self, P> {
        Pipeline {
            emitter: self,
            step,
        }
    }
}

impl<T: Emit, U: Step> Emit for Pipeline<T, U> {
    async fn emit<E: Event>(&self, event: E) {
        let event = self.step.clone().process(event).await;
        self.emitter.emit(event).await
    }
}

impl<ToE: ToEmitter, U: Step> ToEmitter for Pipeline<ToE, U> {
    type Emitter<E: Event> = WithStep<E, ToE::Emitter<U::Event<E>>, U>;

    fn to_emitter<E: Event>(&self) -> Self::Emitter<E> {
        let emitter = self.emitter.to_emitter();
        WithStep::new(emitter, self.step.clone())
    }
}

impl From<Bus> for Pipeline<Bus, Identity> {
    fn from(value: Bus) -> Self {
        Self {
            emitter: value,
            step: Identity,
        }
    }
}
