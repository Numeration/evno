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

struct Inner {
    capacity: usize,
    emitters: papaya::HashMap<TypeId, Box<dyn EmitterProxy>>,
    bind_latch: Arc<bind_latch::BindLatch>,
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

#[derive(Default)]
struct ShutdownSignal(Notify);

#[derive(Clone)]
pub struct Bus {
    inner: Arc<Inner>,
    drop_notifier: Arc<ShutdownSignal>,
}

impl Bus {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Inner::new(capacity)),
            drop_notifier: Default::default(),
        }
    }

    pub fn bind<E: Event>(&self, listener: impl Listener<Event = E>) -> SubscribeHandle {
        self.bind_cancel(CancellationToken::new(), listener)
    }

    pub fn bind_cancel<E: Event>(
        &self,
        cancel: CancellationToken,
        listener: impl Listener<Event = E>,
    ) -> SubscribeHandle {
        let task_guard = self.inner.wait_group.add();
        let bind_guard = self.inner.bind_latch.lock();
        let emitters_guard = self.inner.emitters.owned_guard();
        let emitter = self
            .inner
            .get_emitter_proxy::<E>(&emitters_guard)
            .as_emitter::<E>()
            .clone();

        let actor = ListenerActor(listener, cancel.clone(), task_guard);
        let launcher = Launcher(emitter, bind_guard);
        let join = actor.with(launcher);

        SubscribeHandle::new(cancel, join)
    }

    pub fn on<E: Event>(&self, listener: impl Listener<Event = E>) -> SubscribeHandle {
        self.bind(listener)
    }

    pub fn once<E: Event>(&self, listener: impl Listener<Event = E>) -> SubscribeHandle {
        self.bind(WithTimes::new(1, listener))
    }

    pub fn many<E: Event>(
        &self,
        times: usize,
        listener: impl Listener<Event = E>,
    ) -> SubscribeHandle {
        self.bind(WithTimes::new(times, listener))
    }
}

impl Drain for Bus {
    async fn drain(self) {
        let latch = self.inner.wait_group.clone();
        let barrier = self.drop_notifier.clone();
        let notified = barrier.0.notified();
        drop(self);
        latch.wait().await;
        notified.await;
    }
}

impl Close for Bus {
    async fn close(mut self) {
        if Arc::get_mut(&mut self.inner).is_some() {
            self.drain().await;
        }
    }
}

impl ToEmitter for Bus {
    type Emitter<E: Event> = Publisher<E>;

    fn to_emitter<E: Event>(&self) -> Publisher<E> {
        let emitters_guard = self.inner.emitters.owned_guard();
        let emitter_proxy = self.inner.get_emitter_proxy::<E>(&emitters_guard);

        emitter_proxy.as_emitter().clone()
    }
}

impl Emit for Bus {
    async fn emit<E: Event>(&self, event: E) {
        let emitters_guard = self.inner.emitters.owned_guard();
        let emitter_proxy = self.inner.get_emitter_proxy::<E>(&emitters_guard);

        emitter_proxy.as_emitter().emit(event).await;
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        if Arc::get_mut(&mut self.inner).is_some() {
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

    // 一个简单的事件结构体用于测试
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

        // 使用 Arc<Mutex<Option>> 捕获事件
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

        // 稍等片刻确保绑定完成 (在真实场景中 bind_latch 会处理这个)
        sleep(Duration::from_millis(10)).await;

        let sent_event = TestEvent("hello".to_string());
        bus.emit(sent_event.clone()).await;

        // 等待监听器完成
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

        // 等待事件传播
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

        // 等待 handle 结束，证明任务已完成
        handle.await.expect("Task should complete successfully");

        // 再次确认最终计数
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

        // 立即取消
        let join_handle = handle.cancel();

        // 确保取消操作已传播
        tokio::task::yield_now().await;

        bus.emit(TestEvent("this should not be received".to_string()))
            .await;

        // 等待一段时间以确保监听器没有机会运行
        sleep(Duration::from_millis(50)).await;

        assert_eq!(counter.load(Ordering::SeqCst), 0);
        // 等待任务确认它已经被中止
        assert!(join_handle.await.is_ok());
    }

    #[tokio::test]
    async fn test_drain_waits_for_listeners_to_finish() {
        let bus = Bus::new(2);
        let task_finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let task_finished_clone = task_finished.clone();

        // (tx, rx) 用于确保监听器已经开始执行
        let (tx, rx) = oneshot::channel();
        let mut tx_wrap = Some(tx);

        bus.on(from_fn(move |_event: Guard<TestEvent>| {
            let task_finished = task_finished_clone.clone();
            let tx = tx_wrap.take().unwrap(); // move tx
            async move {
                // 通知测试主体，任务已开始
                let _ = tx.send(());
                // 模拟耗时操作
                sleep(Duration::from_millis(100)).await;
                task_finished.store(true, Ordering::SeqCst);
            }
        }));

        sleep(Duration::from_millis(10)).await;
        bus.emit(TestEvent("long task".to_string())).await;

        // 等待，直到我们确认任务已经开始
        rx.await.unwrap();

        // 任务已经开始，但还没结束
        assert!(!task_finished.load(Ordering::SeqCst));

        // 在一个单独的任务中调用 drain 并等待它
        let drain_handle = tokio::spawn(async move {
            bus.drain().await;
        });

        // 在 drain 运行时，任务应该仍在进行中
        sleep(Duration::from_millis(50)).await;
        assert!(!task_finished.load(Ordering::SeqCst));

        // 等待 drain 完成
        drain_handle.await.unwrap();

        // drain 完成后，任务必须已经结束
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

        // 获取一个类型化的 emitter
        let typed_emitter: Publisher<AnotherTestEvent> = bus.to_emitter();

        typed_emitter.emit(AnotherTestEvent(42)).await;

        sleep(Duration::from_millis(50)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
