use crate::event::Event;
use crate::{Guard, Listener};
use std::marker::PhantomData;
use tokio_util::sync::CancellationToken;

pub struct WithTimes<E, L> {
    listener: L,
    count: usize,
    times: usize,
    _phantom: PhantomData<E>,
}

impl<E, L> WithTimes<E, L> {
    #[inline]
    pub fn new(times: usize, listener: L) -> Self {
        assert!(times > 0, "limit must be greater than zero, got {times}");
        Self {
            listener,
            count: 0,
            times,
            _phantom: PhantomData,
        }
    }
}

impl<E, L> Listener for WithTimes<E, L>
where
    E: Event,
    L: Listener<Event = E>,
{
    type Event = E;

    #[inline]
    async fn begin(&mut self, cannel: &CancellationToken) {
        self.listener.begin(cannel).await;
    }

    #[inline]
    async fn handle(&mut self, cancel: &CancellationToken, event: Guard<Self::Event>) {
        self.count += 1;
        self.listener.handle(cancel, event).await;
        if self.count >= self.times {
            cancel.cancel();
        }
    }

    #[inline]
    async fn after(&mut self, cannel: &CancellationToken) {
        self.listener.after(cannel).await;
    }
}
