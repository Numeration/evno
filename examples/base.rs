use evno::{Bus, many, on, once};
use std::time::Duration;
use tokio::join;

#[derive(Clone)]
struct EventA;

#[derive(Clone)]
struct EventB;

#[derive(Clone)]
struct EventC;

#[derive(Clone)]
struct EventD;

async fn event_c_listener(_: EventC) {
    println!("Handled  Event C");
}

#[tokio::main]
async fn main() {
    let bus = Bus::new(2);

    // Register EventA listener
    let handle1 = bus.bind::<EventA>(once(|_| async {
        println!("Handled  Event A");
    }));

    // Register EventB listener
    let handle2 = bus.bind(on(|_: EventB| async {
        println!("Handled  Event B");
    }));

    // Register EventC listener
    let handle3 = bus.bind(many(3, event_c_listener));

    // Emit events
    bus.emit(EventA).await;
    bus.emit(EventB).await;
    bus.emit(EventC).await;
    bus.emit(EventD).await;

    // Cancel EventB listener and register EventD listener
    handle2.cancel();
    tokio::time::sleep(Duration::from_millis(1)).await;
    let handle4 = bus.bind(on(|_: EventD| async {
        println!("Handled  Event D");
    }));

    // Emit again
    bus.emit(EventA).await;
    bus.emit(EventB).await;
    bus.clone().emit(EventD).await;

    drop(bus);

    let _ = join!(handle1, handle3, handle4);
}
