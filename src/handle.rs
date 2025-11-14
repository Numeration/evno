use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::task::{JoinError, JoinHandle};
use tokio_util::sync::CancellationToken;

/// Subscription handle, used to control the lifecycle of a Listener Actor task.
///
/// `SubscribeHandle` combines a `CancellationToken` (for active cancellation) and a
/// `JoinHandle` (for waiting for task completion or checking results).
#[derive(Debug)]
pub struct SubscribeHandle {
    /// Semaphore used to cancel the Listener Actor task.
    cancel: CancellationToken,
    /// The JoinHandle for the Listener Actor task. Used to wait for the task to end when the Future is Poll'd.
    join: Option<JoinHandle<()>>,
}

impl SubscribeHandle {
    /// Creates a new `SubscribeHandle` instance.
    pub fn new(cancel: CancellationToken, join: JoinHandle<()>) -> Self {
        Self {
            cancel,
            join: Some(join),
        }
    }

    /// Cancels the subscription and returns the underlying `JoinHandle`.
    ///
    /// Calling this method triggers the `CancellationToken`'s cancellation,
    /// causing the Listener Actor to gracefully exit its `run` loop.
    ///
    /// The returned `JoinHandle` can be used to wait for the task's actual termination.
    ///
    /// # Panics
    /// Panics if the `join` field has already been taken (e.g., because `self` was already `poll`ed).
    pub fn cancel(mut self) -> JoinHandle<()> {
        self.cancel.cancel();
        self.join.take().expect("join already taken")
    }
}

impl Future for SubscribeHandle {
    /// The result of the task completion, `Ok(())` if the task finished successfully,
    /// otherwise `Err(JoinError)`.
    type Output = Result<(), JoinError>;

    /// Polls the underlying `JoinHandle`.
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Use Pin::get_mut to access internal fields and ensure join exists.
        Pin::new(&mut self.get_mut().join.as_mut().expect("join already taken")).poll(cx)
    }
}