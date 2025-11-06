use std::marker::PhantomData;
use tokio_util::sync::CancellationToken;
use crate::event::Event;
use crate::{Listener, Rent};

pub struct FromFn<E, F>(F, PhantomData<E>);

impl<E, F> FromFn<E, F> {

    #[inline]
    pub fn new(f: F) -> Self {
        Self(f, PhantomData)
    }
}

impl<E, F, Fut> Listener for FromFn<E, F>
where
    E: Event,
    F: Send + FnMut(Rent<E>) -> Fut + 'static,
    Fut: Send + Future<Output = ()>,
{
    type Event = E;

    #[inline]
    async fn begin(&mut self, _: &CancellationToken) {}

    #[inline]
    async fn handle(&mut self, _: &CancellationToken, event: Rent<Self::Event>) {
        (self.0)(event).await;
    }

    #[inline]
    async fn after(&mut self, _: &CancellationToken)  {}
}

#[inline]
pub fn from_fn<E, F>(f: F) -> FromFn<E, F> 
where
    FromFn<E, F>: Listener,
{
    FromFn::new(f)
}