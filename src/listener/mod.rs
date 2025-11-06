mod from_fn;
mod with_times;

use crate::event::Event;
use acty::{Actor, Inbox};
use futures::StreamExt;
use std::pin::pin;
use tokio_util::sync::CancellationToken;
use crate::task;

pub use from_fn::*;
pub use with_times::*;

pub type Rent<E> = gyre::OwnedEventGuard<E>;

#[trait_variant::make(Send)]
pub trait Listener: Sized + 'static {
    type Event: Event;

    async fn begin(&mut self, cancel: &CancellationToken);
    
    async fn handle(&mut self, cancel: &CancellationToken, event: Rent<Self::Event>) -> ();
    
    async fn after(&mut self, cancel: &CancellationToken);
}

pub struct ListenerActor<L>(pub L, pub CancellationToken, pub task::Guard);

impl<L: Listener> Actor for ListenerActor<L> {
    type Message = gyre::OwnedEventGuard<L::Event>;

    async fn run(self, inbox: impl Inbox<Item = Self::Message>) {
        let ListenerActor(mut listener, cancel, _guard) = self;
        let mut inbox = pin!(inbox);

        listener.begin(&cancel).await;
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
        listener.after(&cancel).await;
    }
}