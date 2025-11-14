mod from_fn;
mod from_fn_with_cancel;
mod with_times;

use crate::event::Event;
use crate::wait_group;
use acty::Actor;
use futures::{Stream, StreamExt};
use std::pin::pin;
use tokio_util::sync::CancellationToken;

pub use from_fn::*;
pub use from_fn_with_cancel::*;
pub use with_times::*;

/// The owned event Guard, provided by the underlying `gyre` library.
///
/// It wraps the event `E` and releases the underlying buffer space when it is dropped.
pub type Guard<E> = gyre::OwnedEventGuard<E>;

/// Trait defining the event handling logic.
///
/// Every struct implementing `Listener` represents an independent event consumer,
/// which runs as a `ListenerActor` within the `Bus`.
#[trait_variant::make(Send)]
pub trait Listener: Sized + 'static {
    /// The event type expected by this Listener.
    type Event: Event;

    /// Called when the listener starts up.
    ///
    /// Executes before the Actor begins receiving events, useful for initializing resources
    /// or performing asynchronous setup.
    async fn begin(&mut self, cancel: &CancellationToken);

    /// Processes a single received event.
    ///
    /// The `cancel` token provides the ability for self-cancellation.
    async fn handle(&mut self, cancel: &CancellationToken, event: Guard<Self::Event>) -> ();

    /// Called when the listener exits (whether due to cancellation, stream end, or error).
    ///
    /// Useful for cleaning up resources or logging exit status.
    async fn after(&mut self, cancel: &CancellationToken);
}

/// A struct that wraps a `Listener` into an `acty::Actor`.
///
/// It manages the Listener's lifecycle, connects it to the event stream, and responds to cancellation signals.
pub struct ListenerActor<L>(pub L, pub CancellationToken, pub wait_group::GroupGuard);

impl<L: Listener> Actor for ListenerActor<L> {
    /// The message type the Actor receives is the event Guard.
    type Message = gyre::OwnedEventGuard<L::Event>;

    /// Runs the Listener Actor's main loop.
    async fn run(self, inbox: impl Stream<Item = Self::Message>) {
        let ListenerActor(mut listener, cancel, _guard) = self;
        let mut inbox = pin!(inbox);

        // 1. Call the `begin` lifecycle method
        listener.begin(&cancel).await;

        // 2. Main event processing loop, continuously watching for cancellation and the event stream
        while !cancel.is_cancelled() {
            tokio::select! {
                // Prioritize getting the next event from the stream
                event = inbox.next() => match event {
                    Some(event) => {
                        listener.handle(&cancel, event).await;
                    }
                    // Stream has ended (Publisher was dropped), break the loop
                    None => break,
                },
                // Respond to external cancellation signal
                _ = cancel.cancelled() => break,
            }
        }

        // 3. Call the `after` lifecycle method
        listener.after(&cancel).await;

        // 4. When the Actor exits, `_guard` (GroupGuard) is dropped, notifying the WaitGroup that the task is complete.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bus, Emit};
    use std::sync::Arc;
    use tokio::sync::{Mutex, oneshot};

    // A simple event for testing purposes.
    #[derive(Debug, Clone)]
    struct TestEvent(u32);

    // A mock listener that logs its lifecycle events.
    struct MockListener {
        // Log of lifecycle method calls
        lifecycle_log: Arc<Mutex<Vec<String>>>,
        // To signal the test when the 'after' method has completed
        completion_tx: Option<oneshot::Sender<()>>,
    }

    impl Listener for MockListener {
        type Event = TestEvent;

        async fn begin(&mut self, _cancel: &CancellationToken) {
            self.lifecycle_log.lock().await.push("begin".to_string());
        }

        async fn handle(&mut self, _cancel: &CancellationToken, event: Guard<Self::Event>) {
            let log_entry = format!("handle:{}", event.0);
            self.lifecycle_log.lock().await.push(log_entry);
        }

        async fn after(&mut self, _cancel: &CancellationToken) {
            self.lifecycle_log.lock().await.push("after".to_string());
            if let Some(tx) = self.completion_tx.take() {
                tx.send(()).unwrap_or(());
            }
        }
    }

    #[tokio::test]
    async fn test_lifecycle_order_on_normal_completion() {
        let bus = Bus::new(2);
        let lifecycle_log = Arc::new(Mutex::new(Vec::new()));
        let (completion_tx, completion_rx) = oneshot::channel();

        let listener = MockListener {
            lifecycle_log: lifecycle_log.clone(),
            completion_tx: Some(completion_tx),
        };

        // Bind the listener.
        let _handle = bus.bind(listener);

        // Allow the actor to start and call 'begin'.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Emit an event, which should trigger 'handle'.
        bus.emit(TestEvent(42)).await;

        // Drop the bus. This closes the publisher side of the channel,
        // which will cause the listener's stream to end, triggering 'after'.
        drop(bus);

        // Wait for the 'after' method to signal its completion.
        completion_rx
            .await
            .expect("Listener's 'after' method should have been called.");

        // Verify the lifecycle methods were called in the correct order.
        let log = lifecycle_log.lock().await;
        assert_eq!(*log, vec!["begin", "handle:42", "after"]);
    }

    #[tokio::test]
    async fn test_lifecycle_on_cancellation() {
        let bus = Bus::new(2);
        let lifecycle_log = Arc::new(Mutex::new(Vec::new()));
        let (completion_tx, completion_rx) = oneshot::channel();

        let listener = MockListener {
            lifecycle_log: lifecycle_log.clone(),
            completion_tx: Some(completion_tx),
        };

        // Bind the listener and get the handle.
        let handle = bus.bind(listener);

        // Allow the actor to start up.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Cancel the subscription via its handle.
        handle.cancel();

        // Wait for the 'after' method to signal completion. Cancellation should trigger it.
        completion_rx
            .await
            .expect("Cancellation should trigger the 'after' method.");

        // Verify that 'begin' and 'after' were called, but not 'handle'.
        let log = lifecycle_log.lock().await;
        assert_eq!(*log, vec!["begin", "after"]);
    }

    // A listener that cancels itself upon receiving an event.
    struct SelfCancellingListener {
        completion_tx: Option<oneshot::Sender<()>>,
    }

    impl Listener for SelfCancellingListener {
        type Event = TestEvent;

        async fn begin(&mut self, _cancel: &CancellationToken) {}

        // In handle, we get the cancellation token and use it.
        async fn handle(&mut self, cancel: &CancellationToken, _event: Guard<Self::Event>) {
            cancel.cancel();
        }

        async fn after(&mut self, _cancel: &CancellationToken) {
            if let Some(tx) = self.completion_tx.take() {
                let _ = tx.send(());
            }
        }
    }

    #[tokio::test]
    async fn test_cancellation_token_passed_to_listener_is_functional() {
        let bus = Bus::new(2);
        let (completion_tx, completion_rx) = oneshot::channel();

        let listener = SelfCancellingListener {
            completion_tx: Some(completion_tx),
        };

        // Bind the listener.
        let handle = bus.bind(listener);

        // Allow the actor to start up.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Emit an event that will cause the listener to cancel itself.
        bus.emit(TestEvent(100)).await;

        // The handle should complete because its underlying task is cancelled.
        // This is the main assertion: the task finishes.
        handle
            .await
            .expect("Task should terminate gracefully after self-cancellation.");

        // We can also verify that 'after' was called.
        completion_rx
            .await
            .expect("The 'after' method should be called on self-cancellation.");
    }
}
