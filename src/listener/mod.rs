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

pub type Guard<E> = gyre::OwnedEventGuard<E>;

#[trait_variant::make(Send)]
pub trait Listener: Sized + 'static {
    type Event: Event;

    async fn begin(&mut self, cancel: &CancellationToken);

    async fn handle(&mut self, cancel: &CancellationToken, event: Guard<Self::Event>) -> ();

    async fn after(&mut self, cancel: &CancellationToken);
}

pub struct ListenerActor<L>(pub L, pub CancellationToken, pub wait_group::GroupGuard);

impl<L: Listener> Actor for ListenerActor<L> {
    type Message = gyre::OwnedEventGuard<L::Event>;

    async fn run(self, inbox: impl Stream<Item = Self::Message>) {
        let ListenerActor(mut listener, cancel, _guard) = self;
        let mut inbox = pin!(inbox);

        listener.begin(&cancel).await;
        while !cancel.is_cancelled() {
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
