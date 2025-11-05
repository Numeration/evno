pub trait Event: Send + Sync + 'static {}

impl<T: Send + Sync + 'static> Event for T {}
