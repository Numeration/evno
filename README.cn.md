# evno: 高性能、类型安全的异步事件总线

[English](./README.md) | **简体中文**

[![Crates.io](https://img.shields.io/crates/v/evno.svg?style=flat-square)](https://crates.io/crates/evno)
[![Docs.rs](https://docs.rs/evno/badge.svg?style=flat-square)](https://docs.rs/evno)

`evno` 是一个基于 Tokio 运行时的高性能、类型安全的异步事件总线库。它利用 [gyre](https://crates.io/crates/gyre) 提供的低延迟、多播环形缓冲区，结合 [acty](https://crates.io/crates/acty) 的 Actor 模型，提供了一个具备结构化并发和中间件能力的事件分发系统。

## 核心概念与特性

`evno` 的设计目标是提供一个高性能且具备可靠生命周期管理能力的事件系统。

### 1. 零丢失启动保证

`evno` 确保事件在所有正在启动的监听器完成订阅注册之前不会开始投递。这意味着您不必担心由于异步启动时序导致的瞬时事件丢失，确保 Listener 始终从事件流的起始点开始接收。

### 2. 结构化并发与任务生命周期

每个通过 `Bus::bind` 启动的 Listener 都运行在一个独立的异步任务中，其生命周期管理严格。

| 方法                                                  | 描述 |
|:----------------------------------------------------| :--- |
| `Bus::bind` / `Bus::on` / `Bus::once` / `Bus::many` | 启动一个新的事件监听任务。 |
| `SubscribeHandle`                                   | 提供了对任务的主动取消 (`cancel()`) 和等待结束 (`.await`) 的能力。 |
| `CancellationToken`                                 | 嵌入到 `Listener` 的 `handle` 方法中，允许 Listener 内部逻辑进行条件性自取消。 |

### 3. 类型安全与事件处理链 (Chain/Step)

`evno` 允许您通过 `Chain` 和 `Step` Trait 构建事件处理管道（中间件）。`Step` 负责将事件从类型 `E_in` 转换为类型 `E_out`，实现上下文注入、日志记录或数据标准化等功能。

*   **类型安全:** 管道的输入和输出类型在编译时确定，确保下游监听器接收到的是期望的、经过处理的事件类型。
*   **链式构建:** 使用 `chain.prepend(Step)` 可以在现有链条前增加新的处理步骤。

### 4. 优雅停机 (Drain/Close)

`Bus` 实例是可克隆的，所有克隆共享底层的事件系统和生命周期状态。

| 方法 | 语义 | 行为 |
| :--- | :--- | :--- |
| `bus.drain()` | **全局强制排空**。消耗 `self`。 | 阻塞直到所有 `Bus` 克隆被丢弃，且所有 Listener 任务运行完毕。 |
| `bus.close()` | **条件优雅关闭**。消耗 `self`。 | 如果当前 `Bus` 实例是**最后一个**引用，则执行完整的 `drain()`。否则，仅丢弃当前引用并立即返回。 |

**最佳实践:** 在应用退出时，对持有 `Bus` 引用的对象调用 `close()`。系统只会在最后一个引用释放时执行全局排空。

---

## 快速上手与教程

我们将通过一系列示例代码，演示 `evno` 的核心用法。

**添加依赖:**

在你的 `Cargo.toml` 中添加 `evno` 和 `tokio`。

```toml
[dependencies]
evno = "1"
tokio = { version = "1", features = ["full"] }
```

### 1. 基础事件分发

定义一个事件，启动一个持续监听的 Listener，并发送事件。

```rust
// main.rs
use evno::{Bus, Emit, Close, Guard, from_fn};

// 1. 定义事件
#[derive(Debug, Clone)]
struct UserAction(String);

#[tokio::main]
async fn main() {
    // 初始化 Bus，容量为 4
    let bus = Bus::new(4);

    // 2. 绑定持续监听器 (Bus::on 是 Bus::bind 的别名)
    bus.on(from_fn(|event: Guard<UserAction>| async move {
        println!("[Listener A] Received action: {}", event.0);
    }));
    
    // 3. 绑定第二个监听器，它们会接收到相同的事件
    bus.on(from_fn(|event: Guard<UserAction>| async move {
        // Guard<E> 是事件的所有权封装，当其被 Drop 时，总线会释放底层资源。
        println!("[Listener B] Confirming: {}", event.0);
    }));

    // 4. 发送事件
    bus.emit(UserAction("Login".to_string())).await;
    bus.emit(UserAction("UpdateProfile".to_string())).await;

    // 5. 优雅关闭，等待所有事件处理完成
    bus.close().await;
    println!("Bus closed successfully, all listeners finished.");
}
```

### 2. 限制次数的监听器与主动取消

`Bus` 提供了 `once` (监听一次) 和 `many` (监听 N 次) 方法，以及通过 `SubscribeHandle` 进行的主动取消。

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

    // 1. 监听一次 (once)
    let counter_clone = counter.clone();
    bus.once(from_fn(move |_event: Guard<CounterEvent>| {
        let c = counter_clone.clone();
        async move { c.fetch_add(1, Ordering::SeqCst); }
    }));
    
    // 2. 监听三次 (many)
    let counter_clone = counter.clone();
    let handle_many = bus.many(3, from_fn(move |_event: Guard<CounterEvent>| {
        let c = counter_clone.clone();
        async move { c.fetch_add(1, Ordering::SeqCst); }
    }));
    
    // 3. 发送 5 个事件
    for i in 0..5 {
        bus.emit(CounterEvent(i)).await;
    }

    // 等待 many 监听器完成（它会在收到第三个事件后自动退出）
    handle_many.await.unwrap();
    assert_eq!(counter.load(Ordering::SeqCst), 4);

    // 4. 演示主动取消
    let handle_cancel = bus.on(from_fn(move |_event: Guard<CounterEvent>| async move {
        unreachable!("This task should have been cancelled.");
    }));
    
    // 立即取消任务
    let join_handle = handle_cancel.cancel();
    // 等待任务确认退出
    assert!(join_handle.await.is_ok());

    bus.close().await;
}
```

### 3. 中间件：类型安全的上下文注入

使用 `Chain` 和 `Step` 实现事件管道，在事件到达 `Bus` 之前注入上下文数据。

```rust
use evno::{Bus, Chain, Close, Emit, Event, Guard, Step, from_fn};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

// 1. 原始事件类型
#[derive(Debug, Clone, PartialEq)]
struct OriginalEvent(String);

// 2. 注入的上下文
#[derive(Debug, Clone, PartialEq)]
struct RequestContext { request_id: u64 }

// 3. 变换后的事件类型
#[derive(Debug, Clone, PartialEq)]
struct ContextualEvent<E>(E, RequestContext);

// 4. 定义 Step：请求 ID 注入器
#[derive(Clone)]
struct RequestInjector(Arc<AtomicU64>);

impl Step for RequestInjector {
    // 定义输出类型：输入 E -> 输出 ContextualEvent<E>
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

    // 5. 构建事件链: Bus -> RequestInjector
    // 所有的 OriginalEvent 都会先经过 RequestInjector 处理
    let chain = Chain::from(bus.clone()).prepend(RequestInjector(counter));

    // 6. 绑定监听器：必须监听最终的类型 ContextualEvent<OriginalEvent>
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

    // 7. 通过 Chain 发送原始事件
    chain.emit(OriginalEvent("First request".to_string())).await;
    chain.emit(OriginalEvent("Second request".to_string())).await;

    // 8. 优雅关闭
    chain.close().await;
    bus.close().await;
}
```

### 4. 使用 `to_emitter` 获取 Typed Emitter

您可以使用 `to_emitter::<E>()` 获取一个特定事件类型的发送端，这在封装发送逻辑或与其他系统集成时非常方便。如果从 `Chain` 获取，返回的 Emitter 会自动应用链上的所有 Step 逻辑。

```rust
use evno::{Bus, Chain, Emit, TypedEmit, Close};
// 沿用上一个例子的 RequestInjector 和 OriginalEvent 定义

#[tokio::main]
async fn main() {
    let bus = Bus::new(4);
    let counter = Arc::new(AtomicU64::new(200));

    let chain = Chain::from(bus.clone()).prepend(RequestInjector(counter));

    // 1. 从 Chain 获取 Typed Emitter
    // 这个 emitter 发送的事件会自动经过 RequestInjector
    let chained_emitter = chain.to_emitter::<OriginalEvent>();
    
    chained_emitter.emit(OriginalEvent("Action via Typed Emitter".to_string())).await;

    // 2. 也可以从 Bus 获取原生 Typed Emitter (绕过 Chain)
    let raw_emitter = bus.to_emitter::<OriginalEvent>();
    raw_emitter.emit(OriginalEvent("Action via Raw Emitter".to_string())).await;
    // 注意：如果直接通过 bus.to_emitter 发送，事件将不会被 RequestInjector 处理

    bus.close().await;
}
```

---

## API 概览

| Trait / Struct | 描述 |
| :--- | :--- |
| `Bus` | 核心事件总线结构，用于事件分发和生命周期管理。 |
| `Emit` | 通用发送 Trait，允许发送任何实现了 `Event` 的类型 (`bus.emit(E)`)。 |
| `TypedEmit` | 特定类型发送 Trait，用于类型固定的 Emitter。 |
| `Drain` / `Close` | 定义总线的优雅停机和资源清理机制。 |
| `Listener` | 用户实现事件处理逻辑的 Trait，包括 `begin`, `handle`, `after` 三个生命周期钩子。 |
| `Guard<E>` | 事件数据的封装类型，表示事件的所有权，其 `Drop` 行为控制底层资源释放。 |
| `SubscribeHandle` | 监听器任务的句柄，用于取消或等待完成。 |
| `Chain` | 事件处理管道结构，用于组合多个 `Step`。 |
| `Step` | 事件转换逻辑的定义 Trait，实现事件的类型变换。 |

## 许可证

本项目采用 [Apache 2.0 许可证](./LICENSE)。