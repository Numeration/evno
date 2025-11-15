//! # evno
//!
//! `evno` is a high-performance, type-safe asynchronous event bus library, designed specifically
//! for the Rust Tokio runtime. It combines an **ultra-fast multicast channel** ([`gyre`](https://docs.rs/gyre))
//! with the **structured concurrency** of the [`acty`](https://docs.rs/acty) Actor model,
//! providing an easy-to-use event distribution system that supports middleware and graceful shutdown.
//!
//! ## Core Design and Features
//!
//! 1.  **Strongly-Typed Event Dispatch:** The `Bus` maintains separate publishers internally for
//!     different event types (`E: Event`), ensuring compile-time type safety for event sending and receiving.
//!
//! 2.  **Startup Guarantee (`BindLatch`):** Publishers wait for all listeners currently starting up to
//!     complete their subscription registration before delivering events. This entirely prevents
//!     transient event loss due to startup race conditions.
//!
//! 3.  **Actor-Driven Lifecycle:** Each subscription (`bind`) launches an independent Actor
//!     (`ListenerActor`), featuring a `begin -> handle -> after` lifecycle. This is tightly integrated
//!     with `CancellationToken` to guarantee **structured concurrency** and **graceful shutdown** for tasks.
//!
//! 4.  **Type-Safe Middleware (`Chain`):** Using the [`Chain`](./chain/struct.Chain.html) and [`Step`](./chain/trait.Step.html)
//!     Traits, you can build rich, type-safe pipelines to preprocess events (e.g., inject context or logging)
//!     before they reach the bus.
//!
//! ## Quick Start
//!
//! This is the simplest example of `Bus` usage, showing how to bind a listener and emit an event.
//!
//! ```rust
//! use evno::{Bus, from_fn, Emit, Guard, Close};
//! use std::time::Duration;
//!
//! // 1. Define the event
//! #[derive(Clone, Debug)]
//! struct UserLoggedIn {
//!     username: String,
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     // 1. Initialize the Bus with a capacity of 4
//!     let bus = Bus::new(4);
//!
//!     // 2. Bind a listener (using from_fn to wrap an async closure)
//!     let handle = bus.on(from_fn(|event: Guard<UserLoggedIn>| async move {
//!         println!("Listener received login for: {}", event.username);
//!         tokio::time::sleep(Duration::from_millis(5)).await;
//!     }));
//!
//!     // 3. Emit events
//!     bus.emit(UserLoggedIn { username: "Alice".to_string() }).await;
//!     bus.emit(UserLoggedIn { username: "Bob".to_string() }).await;
//!
//!     // 4. Graceful shutdown (wait for all event processing to complete)
//!     // close() checks if the bus is the last reference; if so, it executes drain().
//!     // This waits for all listeners to finish their execution.
//!     bus.close().await;
//! }
//! ```
//!
//! ## Core Concepts: Lifecycle and Shutdown
//!
//! `Bus` instances are cloneable.
//!
//! ### `Drain` vs. `Close`
//!
//! | Method | Semantics | Behavior |
//! | :--- | :--- | :--- |
//! | [`Drain`](./emit/trait.Drain.html) | **Global Drain**. Consumes the caller's `Bus` instance. | Blocks until: **1.** All `Bus` clones have been dropped. **2.** All running Listener tasks have finished processing and exited. |
//! | [`Close`](./emit/trait.Close.html) | **Conditional Close**. Consumes the caller's `Bus` instance. | **1.** If the current instance is the **last** `Bus` clone, the behavior is equivalent to `drain()`. **2.** If **other clones still exist**, it only drops the current clone and returns immediately. |
//!
//! **Best Practice:** Always use `close()` in your application. The system will automatically trigger a global drain only when the last holder releases the `Bus`.
//!
//! ### Listener Control Flow and Self-Cancellation
//!
//! The [`Listener`](./listener/trait.Listener.html) Trait allows you to define complex event processing logic.
//!
//! *   The `handle` method receives a `&CancellationToken`. A Listener can initiate its own conditional exit by calling `cancel.cancel()`.
//! *   **Utility Functions:**
//!     *   [`from_fn`](./listener/fn.from_fn.html): Suitable for simple asynchronous closures.
//!     *   [`from_fn_with_cancel`](./listener/fn.from_fn_with_cancel.html): Suitable for closures that need access to the `CancellationToken` within the `handle` logic to perform self-cancellation.
//!
//! ## Event Chain and Context Injection
//!
//! The `Chain` mechanism enables decorating or transforming events before they reach the bus, offering middleware capabilities.
//!
//! ```rust
//! use evno::{Event, Step, Guard, from_fn, Bus, Emit, Close, Chain};
//! use std::sync::atomic::{AtomicU64, Ordering};
//! use std::sync::Arc;
//!
//! #[derive(Debug, Clone, PartialEq)]
//! struct OriginalEvent(String);
//!
//! #[derive(Debug, Clone, PartialEq)]
//! struct RequestContext { request_id: u64 }
//!
//! // The event type after Step transformation (Input E -> Output ContextualEvent<E>)
//! #[derive(Debug, Clone, PartialEq)]
//! struct ContextualEvent<E>(E, RequestContext);
//!
//! // Define a Step to inject RequestContext
//! #[derive(Clone)]
//! struct RequestInjector(Arc<AtomicU64>);
//!
//! impl Step for RequestInjector {
//!     // Use GAT (Generic Associated Type) to define the output type:
//!     // For any incoming event E, the output type is ContextualEvent<E>
//!     type Event<E: Event> = ContextualEvent<E>;
//!
//!     async fn process<E: Event>(self, event: E) -> Self::Event<E> {
//!         let id = self.0.fetch_add(1, Ordering::Relaxed);
//!         ContextualEvent(event, RequestContext { request_id: id })
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let bus = Bus::new(4);
//!     let counter = Arc::new(AtomicU64::new(1000));
//!
//!     // 1. Create the event processing chain: prepend RequestInjector onto the Bus
//!     let chain = Chain::from(bus.clone()).prepend(RequestInjector(counter));
//!
//!     // 2. Bind a listener. Note: It must listen for the type processed by the Step
//!     let handle = bus.on(from_fn(|event: Guard<ContextualEvent<OriginalEvent>>| async move {
//!         // We can safely access the injected context
//!         println!(
//!             "[ID: {}] Processing event: {}",
//!             event.1.request_id,
//!             event.0.0
//!         );
//!         assert!(event.1.request_id == 1000);
//!     }));
//!
//!     // 3. Emit the original event through the pipeline
//!     chain.emit(OriginalEvent("First request".to_string())).await;
//!     chain.emit(OriginalEvent("Second request".to_string())).await;
//!
//!     // 4. Graceful shutdown
//!     chain.close().await;
//!     bus.close().await;
//! }
//! ```
//!
//! ## Module Overview
//!
//! *   [`Bus`](./bus/struct.Bus.html): The core structure of the event bus.
//! *   [`Chain`](./chain/struct.Chain.html) / [`Step`](./chain/trait.Step.html): Mechanisms for building event processing pipelines.
//! *   [`Publisher`](./publisher/struct.Publisher.html): The sending endpoint for a specific event type `E`.
//! *   [`Listener`](./listener/trait.Listener.html): The Trait implemented by users for defining event handling logic.
//! *   [`Emit`](./emit/trait.Emit.html) / [`TypedEmit`](./emit/trait.TypedEmit.html): Traits encapsulating event sending functionality.
//! *   [`Drain`](./emit/trait.Drain.html) / [`Close`](./emit/trait.Close.html): Defines the asynchronous Traits for bus shutdown and cleanup.

mod bind_latch;
mod bus;
mod chain;
mod emit;
mod emitter;
mod event;
mod handle;
mod launcher;
mod listener;
mod publisher;
mod wait_group;

pub use {bus::Bus, chain::*, emit::*, emitter::*, event::Event, handle::*, listener::*};
