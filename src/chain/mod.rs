mod step;
mod with_step;

use crate::event::Event;
use crate::{Bus, Close, Drain, Emit, ToEmitter};

pub use step::*;
pub use with_step::*;

#[derive(Debug, Clone)]
pub struct Chain<T, U> {
    emitter: T,
    step: U,
}

impl<T: Emit, U: Step> Chain<T, U> {
    pub fn prepend<P: Step>(self, step: P) -> Chain<Self, P> {
        Chain {
            emitter: self,
            step,
        }
    }
}

impl<T: Drain, U: Send> Drain for Chain<T, U> {
    async fn drain(self) {
        self.emitter.drain().await;
    }
}

impl<T: Close, U: Send> Close for Chain<T, U> {
    async fn close(self) {
        self.emitter.close().await;
    }
}

impl<T: Emit, U: Step> Emit for Chain<T, U> {
    async fn emit<E: Event>(&self, event: E) {
        let event = self.step.clone().process(event).await;
        self.emitter.emit(event).await
    }
}

impl<ToE: ToEmitter, U: Step> ToEmitter for Chain<ToE, U> {
    type Emitter<E: Event> = WithStep<E, ToE::Emitter<U::Event<E>>, U>;

    fn to_emitter<E: Event>(&self) -> Self::Emitter<E> {
        let emitter = self.emitter.to_emitter();
        WithStep::new(emitter, self.step.clone())
    }
}

impl From<Bus> for Chain<Bus, Identity> {
    fn from(value: Bus) -> Self {
        Self {
            emitter: value,
            step: Identity,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listener::from_fn;
    use crate::{Bus, Guard, TypedEmit};
    use std::any::Any;
    use tokio::sync::oneshot;

    // --- Test Setup: Events and Steps ---

    // 1. Define several event types for transformation
    #[derive(Debug, Clone, PartialEq)]
    struct EventA(i32); // Input event

    #[derive(Debug, Clone, PartialEq)]
    struct EventB(String); // Intermediate event

    #[derive(Debug, Clone, PartialEq)]
    struct EventC(Vec<u8>); // Final event

    // 2. Define a Step to convert EventA -> EventB
    #[derive(Debug, Clone)]
    struct NumberToStringStep;

    impl Step for NumberToStringStep {
        // This GAT says: "For any input event E, the output event will be EventB"
        type Event<E: Event> = EventB;

        async fn process<E: Event>(self, event: E) -> Self::Event<E> {
            // Downcast the generic `event` to the specific type we can handle.
            if let Some(event_a) = (&event as &dyn Any).downcast_ref::<EventA>() {
                EventB(event_a.0.to_string())
            } else {
                panic!("NumberToStringStep only accepts EventA");
            }
        }
    }

    // 3. Define another Step to convert EventB -> EventC
    #[derive(Debug, Clone)]
    struct StringToBytesStep;

    impl Step for StringToBytesStep {
        type Event<E: Event> = EventC;

        async fn process<E: Event>(self, event: E) -> Self::Event<E> {
            if let Some(event_b) = (&event as &dyn Any).downcast_ref::<EventB>() {
                EventC(event_b.0.as_bytes().to_vec())
            } else {
                panic!("StringToBytesStep only accepts EventB");
            }
        }
    }

    #[tokio::test]
    async fn test_identity_chain() {
        let bus = Bus::new(2);
        let pipeline = Chain::from(bus.clone());

        let (tx, rx) = oneshot::channel();
        let mut tx_wrap = Some(tx);

        // The listener expects the original event type
        bus.on(from_fn(move |event: Guard<EventA>| {
            let tx = tx_wrap.take().unwrap();
            async move {
                let _ = tx.send(event.clone());
            }
        }));

        let original_event = EventA(42);
        pipeline.emit(original_event.clone()).await;

        let received_event = rx.await.unwrap();
        assert_eq!(received_event, original_event);
    }

    #[tokio::test]
    async fn test_single_step_chain() {
        let bus = Bus::new(2);
        let pipeline = Chain::from(bus.clone()).prepend(NumberToStringStep);

        let (tx, rx) = oneshot::channel();
        let mut tx_wrap = Some(tx);

        // The listener must expect the *transformed* event type (EventB)
        bus.on(from_fn(move |event: Guard<EventB>| {
            let tx = tx_wrap.take().unwrap();
            async move {
                let _ = tx.send(event.clone());
            }
        }));

        // We emit the *original* event type (EventA)
        pipeline.emit(EventA(123)).await;

        let received_event = rx.await.unwrap();
        assert_eq!(received_event, EventB("123".to_string()));
    }

    #[tokio::test]
    async fn test_chained_pipe_chain() {
        let bus = Bus::new(2);
        let pipeline = Chain::from(bus.clone())
            .prepend(StringToBytesStep)
            .prepend(NumberToStringStep);

        let (tx, rx) = oneshot::channel();
        let mut tx_wrap = Some(tx);

        // The listener must expect the *final* transformed event type (EventC)
        bus.on(from_fn(move |event: Guard<EventC>| {
            let tx = tx_wrap.take().unwrap();
            async move {
                let _ = tx.send(event.clone());
            }
        }));

        // We emit the *initial* event type (EventA)
        pipeline.emit(EventA(999)).await;

        let received_event = rx.await.unwrap();
        assert_eq!(received_event, EventC(b"999".to_vec()));
    }

    #[tokio::test]
    async fn test_chain_to_emitter() {
        let bus = Bus::new(2);
        let pipeline = Chain::from(bus.clone())
            .prepend(StringToBytesStep)
            .prepend(NumberToStringStep);

        let (tx, rx) = oneshot::channel();
        let mut tx_wrap = Some(tx);

        // Listener for the final event type
        bus.on(from_fn(move |event: Guard<EventC>| {
            let tx = tx_wrap.take().unwrap();
            async move {
                let _ = tx.send(event.clone());
            }
        }));

        // Get a typed emitter. The type parameter is the *input* type of the chain.
        let typed_emitter = pipeline.to_emitter::<EventA>();

        // Emit using the typed emitter.
        typed_emitter.emit(EventA(77)).await;

        let received_event = rx.await.unwrap();
        assert_eq!(received_event, EventC(b"77".to_vec()));
    }
}
