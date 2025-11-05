use std::marker::PhantomData;
use tokio_util::sync::CancellationToken;
use crate::event::Event;
use crate::{Listener, Rent};

pub struct WithTimes<E, L> {
    listener: L,
    count: usize,
    times: usize,
    _phantom: PhantomData<E>,
}

impl<E, L> WithTimes<E, L> {
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

impl<E, F> Listener for WithTimes<E, F>
where
    E: Event,
    F: Listener<Event = E>,
{
    type Event = E;

    async fn handle(&mut self, cancel: &CancellationToken, event: Rent<Self::Event>) {
        self.count += 1;
        self.listener.handle(&cancel, event).await;
        if self.count >= self.times {
            cancel.cancel();
        }
    }
}