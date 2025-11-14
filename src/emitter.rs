use crate::TypedEmit;
use crate::event::Event;
use crate::publisher::Publisher;
use std::any::Any;

/// A trait object that encapsulates a `Publisher<E>` for a specific event type `E`.
///
/// `EmitterProxy` allows the `Bus`'s internal `papaya::HashMap` to store `Publisher<E>`
/// instances with different generic parameters `E` by using type erasure for dynamic lookup.
pub trait EmitterProxy: Send + Sync + 'static {
    /// Returns a `dyn Any` reference to itself, used for subsequent downcasting.
    fn as_any(&self) -> &dyn Any;
}

/// Defines how an entity is converted into a reference to its associated typed Emitter.
///
/// This Trait is primarily used for type-safe access on `EmitterProxy`.
pub trait AsEmitter {
    /// The associated typed Emitter reference type. In `Bus`, it is fixed to `Publisher<E>`.
    type Emitter<E: Event>: TypedEmit<Event = E>;

    /// Attempts to convert the entity into an Emitter reference for the specific event type `E`.
    ///
    /// # Panics
    ///
    /// The implementation might panic if the type conversion fails (e.g., in the implementation for `&dyn EmitterProxy`).
    fn as_emitter<E: Event>(&self) -> &Self::Emitter<E>;
}

impl AsEmitter for &dyn EmitterProxy {
    type Emitter<E: Event> = Publisher<E>;

    /// Implements the `AsEmitter` Trait, downcasting to a reference of `Publisher<E>` via `as_any()`.
    ///
    /// # Panics
    ///
    /// Panics if the type stored inside `EmitterProxy` is not `Publisher<E>`.
    fn as_emitter<E: Event>(&self) -> &Self::Emitter<E> {
        self.as_any().downcast_ref::<Publisher<E>>().unwrap()
    }
}

/// Defines how to obtain a typed Emitter instance (usually a clone or ownership) from an entity.
///
/// This Trait allows external callers (like `Bus` or `Chain`) to obtain an independently
/// operable sender endpoint.
pub trait ToEmitter {
    /// The associated typed Emitter instance type.
    type Emitter<E: Event>: TypedEmit<Event = E>;

    /// Returns an Emitter instance for the specific event type `E`.
    fn to_emitter<E: Event>(&self) -> Self::Emitter<E>;
}

impl ToEmitter for &dyn EmitterProxy {
    type Emitter<E: Event> = Publisher<E>;

    /// Implements the `ToEmitter` Trait, obtaining a clone of `Publisher<E>` via downcasting.
    ///
    /// # Panics
    ///
    /// Panics if the type stored inside `EmitterProxy` is not `Publisher<E>`.
    fn to_emitter<E: Event>(&self) -> Self::Emitter<E> {
        self.as_any()
            .downcast_ref::<Publisher<E>>()
            .unwrap()
            .clone()
    }
}