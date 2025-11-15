//! Core Bus Functionality: Emission and Continuous Listening
//!
//! This example shows the simplest usage of evno:
//! 1. Defining an event type.
//! 2. Initializing the Bus.
//! 3. Binding a continuous listener using `Bus::on`.
//! 4. Emitting events.
//! 5. Graceful shutdown using `Bus::close`.
use evno::{Bus, Close, Emit, Guard, from_fn};

// Event
#[derive(Debug, Clone)]
struct UserAction(String);

#[tokio::main]
async fn main() {
    // 1. Initialize the Bus with channel capacity 4
    let bus = Bus::new(4);

    // 2. Bind a continuous listener using `Bus::on`
    bus.on(from_fn(|event: Guard<UserAction>| async move {
        println!("[Listener] Received action: {}", event.0);
    }));

    // 3. Emit events
    bus.emit(UserAction("Login".to_string())).await;
    bus.emit(UserAction("UpdateProfile".to_string())).await;
    bus.emit(UserAction("Logout".to_string())).await;

    // 4. Graceful shutdown
    bus.close().await;
    println!("Bus closed successfully.");
}
