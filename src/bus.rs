use crate::event::Event;
use crate::handle::SubscribeHandle;
use crate::launcher::Launcher;
use crate::publisher::Publisher;
use crate::{
    AsEmitter, Close, Drain, Emit, EmitterProxy, Listener, ListenerActor, ToEmitter, TypedEmit,
    WithTimes, bind_latch, wait_group,
};
use acty::ActorExt;
use std::any::TypeId;
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

/// Internal state, storing the core components of the Bus.
///
/// Wrapped in an Arc, allowing the Bus to be cloned, sharing the same event
/// dispatch logic and lifecycle control.
struct Inner {
    /// The base capacity of the ring buffer (must be a power of 2).
    capacity: usize,
    /// Stores EmitterProxy instances for different event types.
    /// Key is the event's TypeId, Value is a Boxed `Publisher<E>` for that specific event type `E`.
    emitters: papaya::HashMap<TypeId, Box<dyn EmitterProxy>>,
    /// Synchronization mechanism to prevent events from being lost before
    /// listeners have finished starting up.
    bind_latch: Arc<bind_latch::BindLatch>,
    /// Used to track all active Listener tasks, ensuring graceful shutdown (Drain).
    wait_group: wait_group::WaitGroup,
}

impl Inner {
    fn new(capacity: usize) -> Self {
        assert!(capacity >= 2, "capacity must be at least 2");
        assert!(capacity.is_power_of_two(), "capacity must be a power of 2");
        Self {
            capacity,
            emitters: Default::default(),
            bind_latch: Default::default(),
            wait_group: Default::default(),
        }
    }

    /// Gets the EmitterProxy (Publisher<E>) for a specific event type `E`.
    /// Creates and inserts a new `Publisher<E>` if the type does not yet exist.
    fn get_emitter_proxy<'guard, E: Event>(
        &self,
        emitters_guard: &'guard papaya::OwnedGuard<'_>,
    ) -> &'guard dyn EmitterProxy {
        self.emitters
            .get_or_insert_with(
                TypeId::of::<E>(),
                || Box::new(Publisher::<E>::new(self.capacity, self.bind_latch.clone())),
                emitters_guard,
            )
            .deref()
    }
}

/// A signal used to notify waiters when the last Bus instance is dropped.
#[derive(Default)]
struct ShutdownSignal(Notify);

/// Asynchronous Event Bus.
///
/// `Bus` is the core of the `evno` library, responsible for managing publishers
/// (`Publisher`) for different event types and controlling the lifecycle of Listeners.
/// It supports cloning, with all cloned instances sharing the same event dispatch mechanism.
#[derive(Clone)]
pub struct Bus {
    inner: Arc<Inner>,
    /// Mechanism used to notify the `drain` method when all `Bus` references have been dropped.
    drop_notifier: Arc<ShutdownSignal>,
}

impl Bus {
    /// Creates a new event bus with the specified capacity.
    ///
    /// # Panics
    ///
    /// *   Capacity must be greater than or equal to 2.
    /// *   Capacity must be a power of 2.
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Inner::new(capacity)),
            drop_notifier: Default::default(),
        }
    }

    /// Binds a Listener to a specific event type `E`.
    ///
    /// This starts a new asynchronous task (Actor) to process the event stream.
    /// The returned `SubscribeHandle` can be used to wait for the task to complete or to actively cancel it.
    pub fn bind<E: Event>(&self, listener: impl Listener<Event = E>) -> SubscribeHandle {
        // Use a default CancellationToken
        self.bind_cancel(CancellationToken::new(), listener)
    }

    /// Binds a Listener, providing an externally controlled `CancellationToken`.
    ///
    /// The Listener's Actor lifecycle will be associated with the `cancel` token.
    pub fn bind_cancel<E: Event>(
        &self,
        cancel: CancellationToken,
        listener: impl Listener<Event = E>,
    ) -> SubscribeHandle {
        // 1. Register with WaitGroup, ensuring Bus.drain() waits for this task.
        let task_guard = self.inner.wait_group.add();
        // 2. Register with BindLatch, ensuring events are not delivered before the Listener is subscribed.
        let bind_guard = self.inner.bind_latch.lock();

        // 3. Get or create the Publisher for the corresponding event type
        let emitters_guard = self.inner.emitters.owned_guard();
        let emitter = self
            .inner
            // get_emitter_proxy ensures the Publisher exists
            .get_emitter_proxy::<E>(&emitters_guard)
            // Dynamically downcast the EmitterProxy to Publisher<E>
            .as_emitter::<E>()
            .clone();

        // 4. Launch the Actor task
        let actor = ListenerActor(listener, cancel.clone(), task_guard);
        let launcher = Launcher(emitter, bind_guard);
        let join = actor.with(launcher);

        SubscribeHandle::new(cancel, join)
    }

    /// Binds a continuously listening Listener (alias for `bind`).
    pub fn on<E: Event>(&self, listener: impl Listener<Event = E>) -> SubscribeHandle {
        self.bind(listener)
    }

    /// Binds a Listener that only fires once.
    ///
    /// The Listener task automatically cancels and exits after the first event.
    pub fn once<E: Event>(&self, listener: impl Listener<Event = E>) -> SubscribeHandle {
        self.bind(WithTimes::new(1, listener))
    }

    /// Binds a Listener that fires for a specified number of times.
    pub fn many<E: Event>(
        &self,
        times: usize,
        listener: impl Listener<Event = E>,
    ) -> SubscribeHandle {
        self.bind(WithTimes::new(times, listener))
    }
}

impl Drain for Bus {
    /// Performs a global drain, waiting for all active tasks to complete and
    /// waiting for all Bus replicas to be dropped.
    ///
    /// **Note:** This method consumes `self`.
    async fn drain(self) {
        // 1. Clone WaitGroup so we can wait even after self is dropped.
        let latch = self.inner.wait_group.clone();
        // 2. Clone the Notify semaphore.
        let signal = self.drop_notifier.clone();
        let notified = signal.0.notified();

        // 3. Immediately drop the current Bus instance.
        // If this is the last Bus replica, the Drop trigger calls notify_waiters() immediately.
        drop(self);

        // 4. Wait for all Listener tasks to finish (WaitGroup reaches zero).
        latch.wait().await;

        // 5. Wait for all Bus replicas to be dropped (if step 3 was not the last one, wait for others).
        notified.await;
    }
}

impl Close for Bus {
    /// Conditional close. If this is the last Bus reference, `drain()` is executed.
    ///
    /// Otherwise, only the current instance is dropped and returns immediately.
    async fn close(mut self) {
        // Check if `inner` has only one remaining reference (i.e., `self.inner`).
        if Arc::get_mut(&mut self.inner).is_some() {
            // If it is the last reference, perform a full drain.
            self.drain().await;
        }
        // If it's not the last reference, `self` is consumed, but `drain()` is not executed,
        // instead relying on the `Drop` implementation to notify `drop_notifier`.
    }
}

impl ToEmitter for Bus {
    /// The associated Emitter type is the `Publisher<E>` for a specific event type.
    type Emitter<E: Event> = Publisher<E>;

    /// Gets the typed Emitter (Publisher<E>) for a specific event type `E`.
    ///
    /// This allows users to send events directly using the `TypedEmit` interface.
    fn to_emitter<E: Event>(&self) -> Publisher<E> {
        let emitters_guard = self.inner.emitters.owned_guard();
        let emitter_proxy = self.inner.get_emitter_proxy::<E>(&emitters_guard);

        // Use the ToEmitter Trait for type conversion
        emitter_proxy.as_emitter().clone()
    }
}

impl Emit for Bus {
    /// Emits an event to the bus.
    ///
    /// The event will be routed to the Publisher corresponding to type `E`.
    async fn emit<E: Event>(&self, event: E) {
        let emitters_guard = self.inner.emitters.owned_guard();
        let emitter_proxy = self.inner.get_emitter_proxy::<E>(&emitters_guard);

        // Delegates to the typed Publisher for actual sending.
        // The Publisher internally waits for the BindLatch to complete.
        emitter_proxy.as_emitter().emit(event).await;
    }
}

impl Drop for Bus {
    /// Called when a `Bus` instance is dropped.
    fn drop(&mut self) {
        // Check if `inner` has only one remaining reference (i.e., `self.inner`).
        if Arc::get_mut(&mut self.inner).is_some() {
            // If it is the last reference, notify all threads waiting for `drain` to complete.
            self.drop_notifier.0.notify_waiters();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Guard;
    use crate::listener::from_fn;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::sync::oneshot;
    use tokio::time::sleep;

    // A simple event struct for testing
    #[derive(Debug, Clone, PartialEq)]
    struct TestEvent(pub String);

    #[derive(Debug, Clone, PartialEq)]
    struct AnotherTestEvent(pub u32);

    #[test]
    #[should_panic(expected = "capacity must be at least 2")]
    fn new_bus_should_panic_on_capacity_less_than_2() {
        Bus::new(1);
    }

    #[test]
    #[should_panic(expected = "capacity must be a power of 2")]
    fn new_bus_should_panic_on_capacity_not_power_of_two() {
        Bus::new(6);
    }

    #[tokio::test]
    async fn test_bind_and_emit_single_event() {
        let bus = Bus::new(2);
        let (tx, rx) = oneshot::channel();
        let mut tx_wrap = Some(tx);

        // Capture the event using Arc<Mutex<Option>>
        let received_event = Arc::new(tokio::sync::Mutex::new(None));
        let received_event_clone = received_event.clone();

        bus.bind(from_fn(move |event: Guard<TestEvent>| {
            let received_event_clone = received_event_clone.clone();
            let tx = tx_wrap.take().unwrap(); // move tx
            async move {
                *received_event_clone.lock().await = Some(event.clone());
                let _ = tx.send(());
            }
        }));

        // Wait briefly to ensure binding completion (handled by bind_latch in real scenarios)
        sleep(Duration::from_millis(10)).await;

        let sent_event = TestEvent("hello".to_string());
        bus.emit(sent_event.clone()).await;

        // Wait for the listener to complete
        rx.await.expect("Listener should have sent a signal");

        let guard = received_event.lock().await;
        assert_eq!(*guard, Some(sent_event));
    }

    #[tokio::test]
    async fn test_emit_respects_event_types() {
        let bus = Bus::new(4);
        let counter_a = Arc::new(AtomicUsize::new(0));
        let counter_b = Arc::new(AtomicUsize::new(0));

        let counter_a_clone = counter_a.clone();
        bus.on(from_fn(move |_event: Guard<TestEvent>| {
            let counter = counter_a_clone.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }));

        let counter_b_clone = counter_b.clone();
        bus.on(from_fn(move |_event: Guard<AnotherTestEvent>| {
            let counter = counter_b_clone.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }));

        sleep(Duration::from_millis(10)).await;

        bus.emit(TestEvent("event for A".to_string())).await;
        bus.emit(TestEvent("another for A".to_string())).await;
        bus.emit(AnotherTestEvent(123)).await;

        // Wait for event propagation
        sleep(Duration::from_millis(50)).await;

        assert_eq!(counter_a.load(Ordering::SeqCst), 2);
        assert_eq!(counter_b.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_once_listener_fires_only_once() {
        let bus = Bus::new(2);
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let handle = bus.once(from_fn(move |_event: Guard<TestEvent>| {
            let counter = counter_clone.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }));

        sleep(Duration::from_millis(10)).await;

        bus.emit(TestEvent("first".to_string())).await;
        bus.emit(TestEvent("second".to_string())).await;

        // Wait for handle completion, proving the task has finished
        handle.await.expect("Task should complete successfully");

        // Reconfirm the final count
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_many_listener_fires_exact_times() {
        let bus = Bus::new(8);
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let handle = bus.many(
            3,
            from_fn(move |_event: Guard<TestEvent>| {
                let counter = counter_clone.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            }),
        );

        sleep(Duration::from_millis(10)).await;

        for i in 0..5 {
            bus.emit(TestEvent(format!("event {}", i))).await;
        }

        handle.await.expect("Task should complete successfully");
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_subscribe_handle_cancel() {
        let bus = Bus::new(2);
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let handle = bus.on(from_fn(move |_event: Guard<TestEvent>| {
            let counter = counter_clone.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }));

        // Cancel immediately
        let join_handle = handle.cancel();

        // Ensure cancellation has propagated
        tokio::task::yield_now().await;

        bus.emit(TestEvent("this should not be received".to_string()))
            .await;

        // Wait briefly to ensure the listener had no chance to run
        sleep(Duration::from_millis(50)).await;

        assert_eq!(counter.load(Ordering::SeqCst), 0);
        // Wait for the task to confirm it was aborted
        assert!(join_handle.await.is_ok());
    }

    #[tokio::test]
    async fn test_drain_waits_for_listeners_to_finish() {
        let bus = Bus::new(2);
        let task_finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let task_finished_clone = task_finished.clone();

        // (tx, rx) ensures the listener has started execution
        let (tx, rx) = oneshot::channel();
        let mut tx_wrap = Some(tx);

        bus.on(from_fn(move |_event: Guard<TestEvent>| {
            let task_finished = task_finished_clone.clone();
            let tx = tx_wrap.take().unwrap(); // move tx
            async move {
                // Notify the test body that the task has started
                tx.send(()).unwrap_or(());
                // Simulate a time-consuming operation
                sleep(Duration::from_millis(100)).await;
                task_finished.store(true, Ordering::SeqCst);
            }
        }));

        sleep(Duration::from_millis(10)).await;
        bus.emit(TestEvent("long task".to_string())).await;

        // Wait until we confirm the task has started
        rx.await.unwrap();

        // Task has started, but not finished
        assert!(!task_finished.load(Ordering::SeqCst));

        // Call drain in a separate task and wait for it
        let drain_handle = tokio::spawn(async move {
            bus.drain().await;
        });

        // While drain is running, the task should still be in progress
        sleep(Duration::from_millis(50)).await;
        assert!(!task_finished.load(Ordering::SeqCst));

        // Wait for drain to complete
        drain_handle.await.unwrap();

        // After drain completes, the task must have finished
        assert!(task_finished.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_to_emitter_works_correctly() {
        let bus = Bus::new(2);
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        bus.on(from_fn(move |_event: Guard<AnotherTestEvent>| {
            let counter = counter_clone.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }));

        sleep(Duration::from_millis(10)).await;

        // Get a typed emitter
        let typed_emitter: Publisher<AnotherTestEvent> = bus.to_emitter();

        typed_emitter.emit(AnotherTestEvent(42)).await;

        sleep(Duration::from_millis(50)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
