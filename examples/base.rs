use evno::{Bus, Close, Emit, Guard, from_fn};
use std::time::Duration;

#[derive(Clone)]
struct EventA;

#[derive(Clone)]
struct EventB;

#[derive(Clone)]
struct EventC;

#[derive(Clone)]
struct EventD;

async fn event_c_listener(_: Guard<EventC>) {
    println!("Handled  Event C");
}

#[tokio::main]
async fn main() {
    let bus = Bus::new(2);

    // Register EventA listener
    bus.once::<EventA>(from_fn(|_| async {
        println!("Handled  Event A");
    }));

    // Register EventB listener
    let handle2 = bus.on(from_fn(|_: Guard<EventB>| async {
        println!("Handled  Event B");
    }));

    // Register EventC listener
    bus.many(3, from_fn(event_c_listener));

    // Emit events
    bus.emit(EventA).await;
    bus.emit(EventB).await;
    bus.emit(EventC).await;
    bus.emit(EventD).await;

    // Cancel EventB listener and register EventD listener
    tokio::time::sleep(Duration::from_millis(100)).await;
    handle2.cancel();
    tokio::time::sleep(Duration::from_millis(100)).await;
    bus.on(from_fn(|_: Guard<EventD>| async {
        println!("Handled  Event D");
    }));

    // Emit again
    bus.emit(EventA).await;
    bus.emit(EventB).await;
    {
        bus.clone().emit(EventD).await;
    }
    bus.close().await;
}
