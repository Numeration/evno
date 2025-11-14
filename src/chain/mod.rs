mod step;
mod with_step;

use crate::event::Event;
use crate::{Bus, Close, Drain, Emit, ToEmitter};

pub use step::*;
pub use with_step::*;

/// The core structure of the event processing chain.
///
/// `Chain<T, U>` wraps a base Emitter `T` and an event processing step `U` (`Step`).
/// Events are transformed by `U` before being sent to `T`.
///
/// The structure of the chain is recursive: `Chain<Chain<Bus, Step1>, Step2>` means
/// the event first passes through `Step2`, then `Step1`, and finally reaches the `Bus`.
#[derive(Debug, Clone)]
pub struct Chain<T, U> {
    /// The next processing stage Emitter in the chain (can be a Bus or another Chain).
    emitter: T,
    /// The transformation step executed before the event is passed to `emitter`.
    step: U,
}

impl<T: Emit, U: Step> Chain<T, U> {
    /// Prepends a new step `P` to the front of the current chain.
    ///
    /// The new step `P` will be executed before the current chain's step `U`.
    ///
    /// # Generic Arguments
    /// * `T`: The underlying Emitter of the current chain.
    /// * `U`: The existing `Step` of the current chain.
    /// * `P`: The new `Step` to add.
    ///
    /// # Returns
    /// Returns a new `Chain<Self, P>`, where `Self` is the original `Chain<T, U>`.
    pub fn prepend<P: Step>(self, step: P) -> Chain<Self, P> {
        Chain {
            emitter: self,
            step,
        }
    }
}

impl<T: Drain, U: Send> Drain for Chain<T, U> {
    /// Delegates the drain operation to the underlying Emitter.
    ///
    /// Since `Chain` itself doesn't hold tasks, draining relies on the innermost Emitter (usually `Bus`).
    async fn drain(self) {
        self.emitter.drain().await;
    }
}

impl<T: Close, U: Send> Close for Chain<T, U> {
    /// Delegates the close operation to the underlying Emitter.
    async fn close(self) {
        self.emitter.close().await;
    }
}

impl<T: Emit, U: Step> Emit for Chain<T, U> {
    /// Sends an event through the chain.
    ///
    /// 1. Clones `step` and calls its `process` method to transform the event.
    /// 2. Sends the transformed event to the underlying `emitter`.
    async fn emit<E: Event>(&self, event: E) {
        // The Step must be cloneable as it usually contains context that needs to be moved by value.
        let event = self.step.clone().process(event).await;
        self.emitter.emit(event).await
    }
}

impl<ToE: ToEmitter, U: Step> ToEmitter for Chain<ToE, U> {
    /// The associated Emitter type is an Emitter wrapped with a Step.
    ///
    /// The `WithStep` struct ensures that the Step transformation logic is
    /// automatically applied even when sending events through a typed Emitter.
    type Emitter<E: Event> = WithStep<E, ToE::Emitter<U::Event<E>>, U>;

    /// Gets a typed Emitter that automatically applies the Step logic on the chain when used.
    fn to_emitter<E: Event>(&self) -> Self::Emitter<E> {
        // 1. Get the typed Emitter instance from the underlying Emitter (Bus).
        let emitter = self.emitter.to_emitter();
        // 2. Wrap it together with the current Step.
        WithStep::new(emitter, self.step.clone())
    }
}

impl From<Bus> for Chain<Bus, Identity> {
    /// Allows converting a `Bus` into a base chain using `Chain::from(Bus)`, with `Identity` as the first step.
    ///
    /// The `Identity` step performs no transformation, ensuring the `Emit` semantics of the base `Bus` remain unchanged.
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
        let chain = Chain::from(bus.clone());
        let (tx, rx) = oneshot::channel();
        let mut tx_wrap = Some(tx);

        // The listener expects the original event type, as Identity step does nothing.
        bus.on(from_fn(move |event: Guard<OriginalEvent>| {
            let tx = tx_wrap.take().unwrap();
            async move {
                tx.send(event.clone()).unwrap_or(());
            }
        }));

        let original_event = OriginalEvent("hello".to_string());
        chain.emit(original_event.clone()).await;

        let received_event = rx.await.unwrap();
        assert_eq!(received_event, original_event);
    }

    #[tokio::test]
    async fn test_single_step_injects_context() {
        let bus = Bus::new(2);
        let chain = Chain::from(bus.clone()).prepend(WithRequestStep::new());
        let (tx, rx) = oneshot::channel();
        let mut tx_wrap = Some(tx);

        // The listener now expects the *wrapped* event type.
        // This is checked at compile time!
        bus.on(from_fn(
            move |event: Guard<Scoped<OriginalEvent, RequestContext>>| {
                let tx = tx_wrap.take().unwrap();
                async move {
                    tx.send(event.clone()).unwrap_or(());
                }
            },
        ));

        // We still emit the *original* event type.
        let original_event = OriginalEvent("find user".to_string());
        chain.emit(original_event.clone()).await;

        let received_event = rx.await.unwrap();

        // Assert that the original event is preserved inside the wrapper.
        assert_eq!(received_event.event, original_event);
        // Assert that the context was correctly injected.
        assert_eq!(received_event.context.request_id, 1);
    }

    #[tokio::test]
    async fn test_chained_steps_nest_context_correctly() {
        let bus = Bus::new(2);
        let chain = Chain::from(bus.clone())
            .prepend(WithUserStep { user_id: 123 }) // Outer wrapper
            .prepend(WithRequestStep::new()); // Inner wrapper

        let (tx, rx) = oneshot::channel();
        let mut tx_wrap = Some(tx);

        // The listener's expected type reflects the nested structure.
        // Type: Scoped<Scoped<OriginalEvent, RequestContext>, UserContext>
        // This complex type signature is the proof of compile-time safety.
        bus.on(from_fn(
            move |event: Guard<Scoped<Scoped<OriginalEvent, RequestContext>, UserContext>>| {
                let tx = tx_wrap.take().unwrap();
                async move {
                    tx.send(event.clone()).unwrap_or(());
                }
            },
        ));

        // We emit the simple, original event.
        let original_event = OriginalEvent("update profile".to_string());
        chain.emit(original_event.clone()).await;

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
        let chain = Chain::from(bus.clone())
            .prepend(WithUserStep { user_id: 456 })
            .prepend(WithRequestStep::new());

        let (tx, rx) = oneshot::channel();
        let mut tx_wrap = Some(tx);

        // Listener for the final, fully-wrapped event type.
        bus.on(from_fn(
            move |event: Guard<Scoped<Scoped<OriginalEvent, RequestContext>, UserContext>>| {
                let tx = tx_wrap.take().unwrap();
                async move {
                    tx.send(event.clone()).unwrap_or(());
                }
            },
        ));

        // Get a typed emitter. The type parameter is the *input* type of the chain.
        let typed_emitter = chain.to_emitter::<OriginalEvent>();

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
