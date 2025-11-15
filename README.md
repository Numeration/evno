# evno: High-Performance, Type-Safe Asynchronous Event Bus

**English** | [简体中文](./README.cn.md)

[![Crates.io](https://img.shields.io/crates/v/evno.svg?style=flat-square)](https://crates.io/crates/evno)
[![Docs.rs](https://docs.rs/evno/badge.svg?style=flat-square)](https://docs.rs/evno)

`evno` is a high-performance, type-safe asynchronous event bus library built on the Tokio runtime. It leverages the low-latency, multicast ring buffer provided by [gyre](https://crates.io/crates/gyre), combined with the structured concurrency of the [acty](https://crates.io/crates/acty) Actor model, to deliver an event distribution system that features middleware capabilities and reliable lifecycle management.

## Core Concepts and Features

`evno` is designed to provide a high-performance event system with robust and predictable lifecycle management.

### 1. Zero-Loss Startup Guarantee

`evno` ensures that event delivery does not begin until all listeners currently starting up have completed their subscription registration. This eliminates concerns about transient event loss due to asynchronous startup race conditions, guaranteeing that listeners always start receiving events from the initial point in the stream.

### 2. Structured Concurrency and Task Lifecycle

Every Listener started via `Bus::bind` runs in an independent asynchronous task with strict lifecycle control.

| Method | Description |
| :--- | :--- |
| `Bus::bind` / `Bus::on` / `Bus::once` / `Bus::many` | Starts a new event listening task. |
| `SubscribeHandle` | Provides explicit control for active task cancellation (`cancel()`) and waiting for completion (`.await`). |
| `CancellationToken` | Embedded within the `Listener`'s `handle` method, enabling the Listener's internal logic to perform conditional self-cancellation. |

### 3. Type Safety and Event Chain (Chain/Step)

`evno` allows you to build event processing pipelines (middleware) using the `Chain` and `Step` Traits. A `Step` is responsible for transforming an event from type `E_in` to type `E_out`, enabling features like context injection, logging, or data normalization.

*   **Type Safety:** The input and output types of the pipeline are determined at compile time, ensuring downstream listeners receive the expected, processed event type.
*   **Chaining:** Use `chain.prepend(Step)` to add a new processing step to the front of the existing chain.

### 4. Graceful Shutdown (Drain/Close)

`Bus` instances are cloneable, with all clones sharing the underlying event system and lifecycle state.

| Method | Semantics | Behavior |
| :--- | :--- | :--- |
| `bus.drain()` | **Global Forced Drain**. Consumes `self`. | Blocks until **1.** All `Bus` clones have been dropped, and **2.** All running Listener tasks have completed processing and exited. |
| `bus.close()` | **Conditional Graceful Shutdown**. Consumes `self`. | If the current `Bus` instance is the **last** remaining reference, it executes a full `drain()`. Otherwise, it only drops the current reference and returns immediately. |

**Best Practice:** When exiting the application, call `close()` on the objects holding `Bus` references. The system will automatically trigger a global drain only when the very last reference is released.

---

## Getting Started and Tutorials

We will demonstrate the core usage of `evno` through a series of examples.

**Adding Dependencies:**

Add `evno` and `tokio` to your `Cargo.toml`.

```toml
[dependencies]
evno = "1"
tokio = { version = "1", features = ["full"] }
```

### 1. Basic Event Dispatch

Define an event, start a continuous listener, and send events.

```rust
// main.rs
use evno::{Bus, Emit, Close, Guard, from_fn};

// 1. Define the event
#[derive(Debug, Clone)]
struct UserAction(String);

#[tokio::main]
async fn main() {
    // Initialize Bus with capacity 4
    let bus = Bus::new(4);

    // 2. Bind a continuous listener (Bus::on is an alias for Bus::bind)
    bus.on(from_fn(|event: Guard<UserAction>| async move {
        println!("[Listener A] Received action: {}", event.0);
    }));
    
    // 3. Bind a second listener; both will receive the same events
    bus.on(from_fn(|event: Guard<UserAction>| async move {
        // Guard<E> is an ownership wrapper for the event. When it is dropped,
        // the bus releases the underlying buffer resources.
        println!("[Listener B] Confirming: {}", event.0);
    }));

    // 4. Emit events
    bus.emit(UserAction("Login".to_string())).await;
    bus.emit(UserAction("UpdateProfile".to_string())).await;

    // 5. Graceful shutdown, waiting for all event processing to complete
    bus.close().await;
    println!("Bus closed successfully, all listeners finished.");
}
```

### 2. Limit Listeners and Active Cancellation

`Bus` provides `once` (listen once) and `many` (listen N times) methods, as well as active cancellation via `SubscribeHandle`.

```rust
use evno::{Bus, Emit, Guard, Close, from_fn};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone)]
struct CounterEvent(u32);

#[tokio::main]
async fn main() {
    let bus = Bus::new(4);
    let counter = Arc::new(AtomicUsize::new(0));

    // 1. Listen once (once)
    let counter_clone = counter.clone();
    bus.once(from_fn(move |_event: Guard<CounterEvent>| {
        let c = counter_clone.clone();
        async move { c.fetch_add(1, Ordering::SeqCst); }
    }));
    
    // 2. Listen three times (many)
    let counter_clone = counter.clone();
    let handle_many = bus.many(3, from_fn(move |_event: Guard<CounterEvent>| {
        let c = counter_clone.clone();
        async move { c.fetch_add(1, Ordering::SeqCst); }
    }));
    
    // 3. Emit 5 events
    for i in 0..5 {
        bus.emit(CounterEvent(i)).await;
    }

    // Wait for the 'many' listener to complete (it automatically exits after the 3rd event)
    handle_many.await.unwrap();
    assert_eq!(counter.load(Ordering::SeqCst), 4); // 1 (once) + 3 (many)

    // 4. Demonstrate active cancellation
    let handle_cancel = bus.on(from_fn(move |_event: Guard<CounterEvent>| async move {
        unreachable!("This task should have been cancelled.");
    }));
    
    // Immediately cancel the task
    let join_handle = handle_cancel.cancel();
    // Wait for the task to confirm exit
    assert!(join_handle.await.is_ok());

    bus.close().await;
}
```

### 3. Middleware: Type-Safe Context Injection

Use `Chain` and `Step` to implement an event pipeline that injects context data before the event reaches the `Bus`.

```rust
use evno::{Bus, Chain, Close, Emit, Event, Guard, Step, from_fn};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

// 1. Original event type
#[derive(Debug, Clone, PartialEq)]
struct OriginalEvent(String);

// 2. Injected context
#[derive(Debug, Clone, PartialEq)]
struct RequestContext { request_id: u64 }

// 3. Transformed event type
#[derive(Debug, Clone, PartialEq)]
struct ContextualEvent<E>(E, RequestContext);

// 4. Define Step: Request ID Injector
#[derive(Clone)]
struct RequestInjector(Arc<AtomicU64>);

impl Step for RequestInjector {
    // Define the output type: Input E -> Output ContextualEvent<E>
    type Event<E: Event> = ContextualEvent<E>;

    async fn process<E: Event>(self, event: E) -> Self::Event<E> {
        let id = self.0.fetch_add(1, Ordering::Relaxed);
        ContextualEvent(event, RequestContext { request_id: id })
    }
}

#[tokio::main]
async fn main() {
    let bus = Bus::new(4);
    let counter = Arc::new(AtomicU64::new(100));

    // 5. Build the event chain: Bus <- RequestInjector
    // All OriginalEvents will first pass through RequestInjector
    let chain = Chain::from(bus.clone()).prepend(RequestInjector(counter));

    // 6. Bind listener: Must listen for the final type ContextualEvent<OriginalEvent>
    bus.on(from_fn(
        move |event: Guard<ContextualEvent<OriginalEvent>>| async move {
            let (original, context) = (&event.0, &event.1);
            println!(
                "[Listener] ID: {} -> Event: {}",
                context.request_id,
                original.0
            );
        },
    ));

    // 7. Emit the original event through the Chain
    chain.emit(OriginalEvent("First request".to_string())).await;
    chain.emit(OriginalEvent("Second request".to_string())).await;

    // 8. Graceful shutdown
    chain.close().await;
    bus.close().await;
}
```

### 4. Using `to_emitter` to Get a Typed Emitter

You can use `to_emitter::<E>()` to obtain a sender endpoint for a specific event type. This is convenient for encapsulating sending logic or integrating with other systems. If obtained from a `Chain`, the returned Emitter automatically applies all `Step` logic in the chain.

```rust
use evno::{Bus, Chain, Emit, TypedEmit, Close};
// Reuse RequestInjector and OriginalEvent definitions from the previous example

#[tokio::main]
async fn main() {
    let bus = Bus::new(4);
    let counter = Arc::new(AtomicU64::new(200));

    let chain = Chain::from(bus.clone()).prepend(RequestInjector(counter));

    // 1. Get a Typed Emitter from the Chain
    // Events sent through this emitter will automatically pass RequestInjector
    let chained_emitter = chain.to_emitter::<OriginalEvent>();
    
    chained_emitter.emit(OriginalEvent("Action via Chained Emitter".to_string())).await;

    // 2. Get a raw Typed Emitter directly from the Bus (bypasses the Chain)
    let raw_emitter = bus.to_emitter::<OriginalEvent>();
    raw_emitter.emit(OriginalEvent("Action via Raw Emitter".to_string())).await;
    // Note: Events sent via raw_emitter will NOT be processed by RequestInjector

    bus.close().await;
}
```

---

## API Overview

| Trait / Struct | Description |
| :--- | :--- |
| `Bus` | The core event bus structure, used for event distribution and lifecycle management. |
| `Emit` | Generic sending Trait, allowing the sending of any type implementing `Event` (`bus.emit(E)`). |
| `TypedEmit` | Specific type sending Trait, used by type-fixed Emitters. |
| `Drain` / `Close` | Defines the asynchronous Traits for graceful bus shutdown and resource cleanup. |
| `Listener` | The Trait implemented by users to define event handling logic, including `begin`, `handle`, and `after` lifecycle hooks. |
| `Guard<E>` | The event data wrapper type, representing ownership, whose `Drop` behavior controls the release of underlying resources. |
| `SubscribeHandle` | The handle for a listener task, used for cancellation or waiting for completion. |
| `Chain` | The event processing pipeline structure, used to combine multiple `Step`s. |
| `Step` | The Trait defining event transformation logic, implementing event type modification. |

## License

This project is licensed under the [Apache 2.0 License](LICENSE-APACHE).