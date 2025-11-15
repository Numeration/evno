//! Event Chain Middleware: Simple Context Injection
//!
//! This example demonstrates using `Chain` and `Step` to inject context
//! into an event, transforming its type before it reaches the listener.
use evno::{Bus, Chain, Close, Emit, Event, Guard, Step, from_fn};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

// Original event type sent by the application
#[derive(Debug, Clone, PartialEq)]
struct AppEvent(String);

// Context data to be injected by the middleware
#[derive(Debug, Clone, PartialEq)]
struct RequestContext {
    request_id: u64,
}

// The transformed event type (GAT output)
#[derive(Debug, Clone, PartialEq)]
struct ContextualEvent<E>(E, RequestContext);

// The Step
#[derive(Clone)]
struct RequestTracer(Arc<AtomicU64>);

impl Step for RequestTracer {
    // For input E, the output type is ContextualEvent<E>
    type Event<E: Event> = ContextualEvent<E>;

    async fn process<E: Event>(self, event: E) -> Self::Event<E> {
        let id = self.0.fetch_add(1, Ordering::Relaxed);
        ContextualEvent(event, RequestContext { request_id: id })
    }
}

#[tokio::main]
async fn main() {
    let bus = Bus::new(4);
    let counter = Arc::new(AtomicU64::new(1));

    // 1. Build the Chain
    let chain = Chain::from(bus.clone()).prepend(RequestTracer(counter));

    // 2. Bind Listener: Must listen for the transformed type (ContextualEvent<AppEvent>)
    bus.on(from_fn(
        |event: Guard<ContextualEvent<AppEvent>>| async move {
            let (original_event, context) = (&event.0, &event.1);
            println!(
                "[Listener] Received '{}' | Injected ID: {}",
                original_event.0, context.request_id
            );
        },
    ));

    // 3. Emit original events through the Chain
    chain.emit(AppEvent("Start Request".to_string())).await;
    chain.emit(AppEvent("End Request".to_string())).await;

    // 4. Graceful shutdown
    chain.close().await;
    bus.close().await;
    println!("\nChain and Bus closed successfully.");
}
