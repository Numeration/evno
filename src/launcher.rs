use crate::bind_latch;
use crate::event::Event;
use crate::publisher::Publisher;
use acty::{Actor, Launch};
use async_stream::stream;
use std::sync::Arc;
use tokio::task::JoinHandle;

/// `Launcher` is responsible for connecting the `Publisher`'s event stream to an `acty` Actor.
///
/// It implements the `acty::Launch` Trait and handles critical synchronization steps during Actor startup:
/// 1. Completing the `bind_latch`, notifying waiting event publishers that they can begin event delivery.
/// 2. Starting the event consumption loop, forwarding events from the `Publisher` to the `Actor`'s `inbox`.
#[allow(dead_code)]
pub struct Launcher<E>(pub(crate) Publisher<E>, pub(crate) bind_latch::BindGuard);

impl<E: Event> Launch for Launcher<E> {
    /// The message type the Actor receives is the owned Guard of the event.
    type Message = gyre::OwnedEventGuard<E>;
    /// The return type after the Actor is launched, which is a Tokio task's JoinHandle.
    type Result<A> = JoinHandle<()>;

    /// Launches the Actor task.
    fn launch<A: Actor<Message = Self::Message>>(self, actor: A) -> Self::Result<A> {
        // `self` owns the `Publisher` and `BindGuard`.
        tokio::spawn(async move {
            // 1. Subscribe to the Publisher to get a Consumer.
            let consumer = Arc::new(self.0.subscribe().await);

            // 2. Critical step: After the subscription is complete, drop self.
            // At this point, self.1 (BindGuard) is dropped, and its Drop implementation
            // decrements the BindLatch counter. This ensures event publishers only start
            // delivery after the Listener is ready to receive events.
            drop(self);

            // 3. Wrap the Consumer into a Stream for the Actor.
            let stream = stream! {
                while let Some(event) = consumer.next_owned().await {
                    yield event;
                }
            };

            // 4. Run the Actor's main loop.
            actor.run(stream).await
        })
    }
}
