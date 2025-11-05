use crate::event::Event;
use acty::{Actor, Inbox};
use futures::StreamExt;
use std::marker::PhantomData;
use std::pin::pin;
use tokio_util::sync::CancellationToken;

pub type Rent<E> = gyre::OwnedEventGuard<E>;

#[trait_variant::make(Send)]
pub trait Listener: Sized + 'static {
    type Event: Event;

    async fn handle(&mut self, cancel: &CancellationToken, event: Rent<Self::Event>) -> ();
}

pub struct ListenerActor<L>(pub L, pub CancellationToken);

impl<L: Listener> Actor for ListenerActor<L> {
    type Message = gyre::OwnedEventGuard<L::Event>;

    async fn run(self, inbox: impl Inbox<Item = Self::Message>) {
        let ListenerActor(mut listener, cancel) = self;
        let mut inbox = pin!(inbox);

        loop {
            tokio::select! {
                event = inbox.next() => match event {
                    Some(event) => {
                        listener.handle(&cancel, event).await;
                    }
                    None => break,
                },
                _ = cancel.cancelled() => break,
            }
        }
    }
}

pub struct On<E, F>(F, PhantomData<E>);

impl<E, F, Fut> Listener for On<E, F>
where
    E: Event,
    F: Send + FnMut(Rent<E>) -> Fut + 'static,
    Fut: Send + Future<Output = ()>,
{
    type Event = E;

    async fn handle(&mut self, _: &CancellationToken, event: Rent<Self::Event>) {
        (self.0)(event).await;
    }
}

pub fn on<E, F>(f: F) -> On<E, F>
where
    On<E, F>: Listener,
{
    On(f, PhantomData)
}

pub struct Once<E, F>(F, PhantomData<E>);

impl<E, F, Fut> Listener for Once<E, F>
where
    E: Event,
    F: Send + FnMut(Rent<E>) -> Fut + 'static,
    Fut: Send + Future<Output = ()>,
{
    type Event = E;

    async fn handle(&mut self, cancel: &CancellationToken, event: Rent<Self::Event>) {
        (self.0)(event).await;
        cancel.cancel();
    }
}

pub fn once<E, F>(f: F) -> Once<E, F>
where
    Once<E, F>: Listener,
{
    Once(f, PhantomData)
}

struct Counter {
    count: usize,
    times: usize,
}

impl Counter {
    fn new(times: usize) -> Self {
        assert!(times > 0, "limit must be greater than zero, got {times}");
        Self { count: 0, times }
    }

    fn try_increment(&mut self) -> bool {
        self.count += 1;
        self.count >= self.times
    }
}

pub struct Many<E, F>(F, Counter, PhantomData<E>);

impl<E, F, Fut> Listener for Many<E, F>
where
    E: Event,
    F: Send + FnMut(Rent<E>) -> Fut + 'static,
    Fut: Send + Future<Output = ()>,
{
    type Event = E;

    async fn handle(&mut self, cancel: &CancellationToken, event: Rent<Self::Event>) {
        let cancel_flag = self.1.try_increment();

        (self.0)(event).await;

        if cancel_flag {
            cancel.cancel();
        }
    }
}

pub fn many<E, F>(times: usize, f: F) -> Many<E, F>
where
    Many<E, F>: Listener,
{
    Many(f, Counter::new(times), PhantomData)
}
