use crate::emit_barrier;
use crate::event::Event;
use crate::publisher::Publisher;
use acty::{Actor, Launch};
use async_stream::stream;
use std::sync::Arc;
use tokio::task::JoinHandle;

#[allow(dead_code)]
pub struct Launcher<E>(pub(crate) Publisher<E>, pub(crate) emit_barrier::OwnedGuard);

impl<E: Event> Launch for Launcher<E> {
    type Message = gyre::OwnedEventGuard<E>;
    type Result<A> = JoinHandle<()>;

    fn launch<A: Actor<Message = Self::Message>>(self, actor: A) -> Self::Result<A> {
        tokio::spawn(async move {
            let consumer = Arc::new(self.0.subscribe().await);
            drop(self);
            let stream = stream! {
                while let Some(event) = consumer.next_owned().await {
                    yield event;
                }
            };
            actor.run(stream).await
        })
    }
}
