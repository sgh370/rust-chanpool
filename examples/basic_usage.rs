use rust_chanpool::{ChannelPool, PoolConfig};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a pool configuration
    let config = PoolConfig {
        workers: 4,                                
        queue_capacity: 100,                       
        default_timeout: Some(Duration::from_secs(5)), 
    };

    // Create a channel pool with a job handler
    let pool = ChannelPool::new(
        |n: u32| async move {
            println!("Processing job with input: {}", n);
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok(n * 2)
        },
        config,
    );

    println!("Pool created with capacity: {}", pool.total_capacity());

    // Submit jobs to the pool
    let mut handles = Vec::new();
    for i in 0..10 {
        println!("Submitting job with input: {}", i);
        let handle = pool.submit(i).await?;
        handles.push(handle);
    }

    // Wait for all jobs to complete
    for (i, handle) in handles.into_iter().enumerate() {
        match handle.wait().await {
            Ok(result) => println!("Job {} completed with result: {}", i, result),
            Err(err) => println!("Job {} failed with error: {}", i, err),
        }
    }

    // Shutdown the pool when done
    println!("Shutting down the pool");
    pool.shutdown().await;

    Ok(())
}
