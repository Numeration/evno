use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::task::{JoinError, JoinHandle};
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub struct SubscribeHandle {
    cancel: CancellationToken,
    join: Option<JoinHandle<()>>,
}

impl SubscribeHandle {
    pub fn new(cancel: CancellationToken, join: JoinHandle<()>) -> Self {
        Self {
            cancel,
            join: Some(join),
        }
    }

    pub fn cancel(mut self) -> JoinHandle<()> {
        self.cancel.cancel();
        self.join.take().expect("join already taken")
    }
}

impl Future for SubscribeHandle {
    type Output = Result<(), JoinError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.get_mut().join.as_mut().expect("join already taken")).poll(cx)
    }
}
