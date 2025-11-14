use crate::event::Event;
use crate::{Guard, Listener};
use std::marker::PhantomData;
use tokio_util::sync::CancellationToken;

pub struct FromFnWithCancel<E, F>(F, PhantomData<E>);

impl<E, F> FromFnWithCancel<E, F> {
    #[inline]
    pub fn new(f: F) -> Self {
        Self(f, PhantomData)
    }
}

impl<E, F, Fut> Listener for FromFnWithCancel<E, F>
where
    E: Event,
    F: Send + FnMut(CancellationToken, Guard<E>) -> Fut + 'static,
    Fut: Send + Future<Output = ()>,
{
    type Event = E;

    #[inline]
    async fn begin(&mut self, _: &CancellationToken) {}

    #[inline]
    async fn handle(&mut self, cancel: &CancellationToken, event: Guard<Self::Event>) {
        (self.0)(cancel.clone(), event).await;
    }

    #[inline]
    async fn after(&mut self, _: &CancellationToken) {}
}

#[inline]
pub fn from_fn_with_cancel<E, F>(f: F) -> FromFnWithCancel<E, F>
where
    FromFnWithCancel<E, F>: Listener,
{
    FromFnWithCancel::new(f)
}
