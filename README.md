# rust-chanpool

A Rust library for channel pooling in async applications. It manages reusable task workers with backpressure awareness, allowing for efficient processing of async jobs.

## Author

**Saeed Ghanbari** - [GitHub](https://github.com/sgh370)

## Features

- **Channel pooling**: Efficiently distribute async jobs across a pool of workers
- **Reusable task workers**: Workers process jobs sequentially from their queue
- **Backpressure awareness**: Prevents overwhelming workers with too many jobs
- **Configurable**: Set pool size, job queue capacity, and timeouts
- **Error handling**: Comprehensive error handling for job failures, timeouts, and cancellations
- **Async/await support**: Built on tokio for modern async Rust

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
rust-chanpool = "0.1.0"
tokio = { version = "1.36.0", features = ["full"] }
```

## Quick Start

```rust
use rust_chanpool::{ChannelPool, PoolConfig};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a pool configuration
    let config = PoolConfig {
        workers: 4,                                // Number of worker threads
        queue_capacity: 100,                       // Jobs per worker queue
        default_timeout: Some(Duration::from_secs(5)), // Default job timeout
    };

    // Create a channel pool with a job handler
    let pool = ChannelPool::new(
        |n: u32| async move {
            // Simulate some async work
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok(n * 2) // Return the result
        },
        config,
    );

    // Submit jobs to the pool
    let mut handles = Vec::new();
    for i in 0..10 {
        let handle = pool.submit(i).await?;
        handles.push(handle);
    }

    // Wait for all jobs to complete
    for handle in handles {
        let result = handle.wait().await?;
        println!("Job result: {}", result);
    }

    // Shutdown the pool when done
    pool.shutdown().await;

    Ok(())
}
```

## Running the Examples

The library comes with several examples demonstrating various features:

```bash
# Run the basic example
cargo run --example basic_usage

# Run the advanced example demonstrating backpressure and error handling
cargo run --example advanced_usage
```

## Advanced Usage

### Handling Backpressure

```rust
// Submit a job and handle backpressure
match pool.submit(task).await {
    Ok(handle) => {
        // Process the job handle
        let result = handle.wait().await?;
    },
    Err(PoolError::PoolAtCapacity) => {
        // Handle backpressure - e.g., retry later or drop the task
        println!("Pool is at capacity, try again later");
    },
    Err(e) => return Err(e.into()),
}
```

### Custom Timeouts

```rust
// Submit a job with a custom timeout
let handle = pool.submit_with_timeout(
    task,
    Some(Duration::from_millis(200))
).await?;

// Wait for the result, which will timeout if it takes too long
match handle.wait().await {
    Ok(result) => println!("Job completed with result: {}", result),
    Err(PoolError::Timeout) => println!("Job timed out"),
    Err(e) => println!("Job failed: {}", e),
}
```

### Monitoring Pool Capacity

```rust
// Check available capacity
let available = pool.available_capacity();
let total = pool.total_capacity();
println!("Pool usage: {}/{} slots available", available, total);
```

### Error Handling

The library provides comprehensive error handling through the `PoolError` enum:

```rust
// Different error types
match result {
    Ok(value) => println!("Success: {}", value),
    Err(PoolError::PoolAtCapacity) => println!("Pool is at capacity"),
    Err(PoolError::Timeout) => println!("Job timed out"),
    Err(PoolError::Cancelled) => println!("Job was cancelled"),
    Err(PoolError::WorkerShutDown) => println!("Worker was shut down"),
    Err(PoolError::JobFailed(msg)) => println!("Job failed: {}", msg),
}
```

## Design Principles

- **Efficiency**: Minimize overhead for job submission and processing
- **Reliability**: Comprehensive error handling and timeout support
- **Backpressure**: Prevent resource exhaustion by limiting concurrent jobs
- **Simplicity**: Clean API that's easy to use in async Rust applications

## Running Tests

```bash
# Run the test suite
cargo test
```

## License

This project is licensed under the MIT License.
