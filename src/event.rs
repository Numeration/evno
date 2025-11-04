pub trait Event: Clone + Send + Sync + 'static {}

impl<T: Clone + Send + Sync + 'static> Event for T {}
