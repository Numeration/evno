use crate::event::Event;

/// Abstractly represents an entity capable of sending any type of event to the bus.
///
/// `Bus` implements this Trait because it can route events to the corresponding
/// internal publisher based on the event's type.
#[trait_variant::make(Send)]
pub trait Emit: Send + Sync {
    /// Asynchronously sends an event of a specific type.
    ///
    /// This method is generic, allowing it to send any type `E` that implements the `Event` Trait.
    async fn emit<E: Event>(&self, event: E);
}

/// Abstractly represents an entity capable of sending a **specific** event type.
///
/// For example, [`Publisher<E>`] and the Emitter [variant](crate::chain::WithStep) of
/// the `Chain` structure implement this Trait.
#[trait_variant::make(Send)]
pub trait TypedEmit: Send + Sync {
    /// The specific event type handled by this Emitter.
    type Event: Event;

    /// Asynchronously sends the specific event associated with this Emitter.
    async fn emit(&self, event: Self::Event);
}

/// Abstractly represents an entity that can be drained.
///
/// When `drain` is called, the entity must block until all underlying tasks and
/// resources have been cleaned up.
///
/// **Note:** This method usually consumes `self` to ensure no new operations
/// are initiated during the draining process.
#[trait_variant::make(Send)]
pub trait Drain {
    /// Asynchronously performs a complete drain and cleanup.
    async fn drain(self);
}

/// Abstractly represents an entity that can be closed.
///
/// `Close` is typically a safer, conditional version of `Drain`.
/// In `evno`, `Bus::close()` only performs a full drain if it is the last reference.
///
/// **Note:** This method consumes `self`.
#[trait_variant::make(Send)]
pub trait Close {
    /// Asynchronously performs the close operation.
    async fn close(self);
}
