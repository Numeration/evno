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

// chain/mod.rs

// ... (file contents above the tests remain the same) ...

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listener::from_fn;
    use crate::{Bus, TypedEmit};
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::sync::oneshot;

    // 1. A generic wrapper to hold an event and its associated context.
    #[derive(Debug, Clone, PartialEq)]
    struct Scoped<E, C> {
        event: E,
        context: C,
    }

    // 2. The original event we want to emit.
    #[derive(Debug, Clone, PartialEq)]
    struct OriginalEvent(String);

    // 3. Contextual data we want to inject.
    #[derive(Debug, Clone, PartialEq)]
    struct RequestContext {
        request_id: u64,
    }

    #[derive(Debug, Clone, PartialEq)]
    struct UserContext {
        user_id: u32,
    }

    // 4. A Step to inject RequestContext.
    // It takes any event `E` and returns `Scoped<E, RequestContext>`.
    #[derive(Clone)]
    struct WithRequestStep {
        // Use an atomic to generate unique IDs for each request.
        request_counter: std::sync::Arc<AtomicU64>,
    }

    impl WithRequestStep {
        fn new() -> Self {
            Self {
                request_counter: std::sync::Arc::new(AtomicU64::new(1)),
            }
        }
    }

    impl Step for WithRequestStep {
        // GAT: For any incoming event `E`, the output is `Scoped<E, RequestContext>`.
        type Event<E: Event> = Scoped<E, RequestContext>;

        async fn process<E: Event>(self, event: E) -> Self::Event<E> {
            let request_id = self.request_counter.fetch_add(1, Ordering::Relaxed);
            Scoped {
                event,
                context: RequestContext { request_id },
            }
        }
    }

    // 5. A Step to inject UserContext.
    // It also takes any event `E` and returns `Scoped<E, UserContext>`.
    // This demonstrates how different steps can be composed.
    #[derive(Clone)]
    struct WithUserStep {
        user_id: u32,
    }

    impl Step for WithUserStep {
        type Event<E: Event> = Scoped<E, UserContext>;

        async fn process<E: Event>(self, event: E) -> Self::Event<E> {
            Scoped {
                event,
                context: UserContext {
                    user_id: self.user_id,
                },
            }
        }
    }

    #[tokio::test]
    async fn test_identity_chain_passes_event_through() {
        let bus = Bus::new(2);
        let pipeline = Chain::from(bus.clone());
        let (tx, rx) = oneshot::channel();
        let mut tx_wrap = Some(tx);

        // The listener expects the original event type, as Identity step does nothing.
        bus.on::<OriginalEvent>(from_fn(move |event| {
            let tx = tx_wrap.take().unwrap();
            async move {
                tx.send(OriginalEvent::clone(&*event)).unwrap_or(());
            }
        }));

        let original_event = OriginalEvent("hello".to_string());
        pipeline.emit(original_event.clone()).await;

        let received_event = rx.await.unwrap();
        assert_eq!(received_event, original_event);
    }

    #[tokio::test]
    async fn test_single_step_injects_context() {
        let bus = Bus::new(2);
        let pipeline = Chain::from(bus.clone()).prepend(WithRequestStep::new());
        let (tx, rx) = oneshot::channel();
        let mut tx_wrap = Some(tx);

        // The listener now expects the *wrapped* event type.
        // This is checked at compile time!
        bus.on::<Scoped<OriginalEvent, RequestContext>>(from_fn(move |event| {
            let tx = tx_wrap.take().unwrap();
            async move {
                tx.send(Scoped::clone(&*event)).unwrap_or(());
            }
        }));

        // We still emit the *original* event type.
        let original_event = OriginalEvent("find user".to_string());
        pipeline.emit(original_event.clone()).await;

        let received_event = rx.await.unwrap();

        // Assert that the original event is preserved inside the wrapper.
        assert_eq!(received_event.event, original_event);
        // Assert that the context was correctly injected.
        assert_eq!(received_event.context.request_id, 1);
    }

    #[tokio::test]
    async fn test_chained_steps_nest_context_correctly() {
        let bus = Bus::new(2);
        let pipeline = Chain::from(bus.clone())
            .prepend(WithUserStep { user_id: 123 }) // Outer wrapper
            .prepend(WithRequestStep::new()); // Inner wrapper

        let (tx, rx) = oneshot::channel();
        let mut tx_wrap = Some(tx);

        // The listener's expected type reflects the nested structure.
        // Type: Scoped<Scoped<OriginalEvent, RequestContext>, UserContext>
        // This complex type signature is the proof of compile-time safety.
        bus.on::<Scoped<Scoped<OriginalEvent, RequestContext>, UserContext>>(from_fn(
            move |event| {
                let tx = tx_wrap.take().unwrap();
                async move {
                    tx.send(Scoped::clone(&*event)).unwrap_or(());
                }
            },
        ));

        // We emit the simple, original event.
        let original_event = OriginalEvent("update profile".to_string());
        pipeline.emit(original_event.clone()).await;

        let received = rx.await.unwrap();

        // Assert the outer context (UserContext)
        assert_eq!(received.context.user_id, 123);
        // Assert the inner context (RequestContext)
        assert_eq!(received.event.context.request_id, 1);
        // Assert the original event is still preserved at the core.
        assert_eq!(received.event.event, original_event);
    }

    #[tokio::test]
    async fn test_chain_to_emitter_with_context_injection() {
        let bus = Bus::new(2);
        let pipeline = Chain::from(bus.clone())
            .prepend(WithUserStep { user_id: 456 })
            .prepend(WithRequestStep::new());

        let (tx, rx) = oneshot::channel();
        let mut tx_wrap = Some(tx);

        // Listener for the final, fully-wrapped event type.
        bus.on::<Scoped<Scoped<OriginalEvent, RequestContext>, UserContext>>(from_fn(
            move |event| {
                let tx = tx_wrap.take().unwrap();
                async move {
                    tx.send(Scoped::clone(&*event)).unwrap_or(());
                }
            },
        ));

        // Get a typed emitter. The type parameter is the *input* type of the chain.
        let typed_emitter = pipeline.to_emitter::<OriginalEvent>();

        let original_event = OriginalEvent("create post".to_string());
        // Emit using the typed emitter.
        typed_emitter.emit(original_event.clone()).await;

        let received = rx.await.unwrap();

        // Assertions are the same as the previous test, proving `to_emitter` works.
        assert_eq!(received.context.user_id, 456);
        assert_eq!(received.event.context.request_id, 1);
        assert_eq!(received.event.event, original_event);
    }
}
