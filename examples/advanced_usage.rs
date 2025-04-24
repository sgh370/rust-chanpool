use rust_chanpool::{ChannelPool, PoolConfig, PoolError};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a pool with limited capacity to demonstrate backpressure
    let config = PoolConfig {
        workers: 2,
        queue_capacity: 3,  // Small queue to demonstrate backpressure
        default_timeout: Some(Duration::from_millis(500)),
    };

    // Create a counter to track completed jobs
    let completed_jobs = Arc::new(AtomicUsize::new(0));
    let completed_jobs_clone = completed_jobs.clone();

    // Create a channel pool with a job handler that can fail
    let pool = ChannelPool::new(
        move |input: i32| {
            let counter = completed_jobs_clone.clone();
            
            async move {
                println!("Starting job with input: {}", input);
                
                // Simulate different job behaviors based on input value
                match input {
                    // Successful fast job
                    n if n >= 0 && n < 5 => {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        counter.fetch_add(1, Ordering::SeqCst);
                        Ok(n * 2)
                    },
                    // Successful slow job
                    n if n >= 5 && n < 10 => {
                        tokio::time::sleep(Duration::from_millis(600)).await;
                        counter.fetch_add(1, Ordering::SeqCst);
                        Ok(n * 2)
                    },
                    // Job that returns an error
                    n if n < 0 => {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        Err(PoolError::JobFailed(format!("Negative input not allowed: {}", n)))
                    },
                    // Default case
                    _ => {
                        tokio::time::sleep(Duration::from_millis(200)).await;
                        counter.fetch_add(1, Ordering::SeqCst);
                        Ok(input * 3)
                    }
                }
            }
        },
        config,
    );

    println!("Pool created with total capacity: {}", pool.total_capacity());
    println!("Current available capacity: {}", pool.available_capacity());

    // Demonstrate submitting jobs with different behaviors
    println!("\n--- Submitting a mix of jobs ---");
    
    // Submit a batch of jobs that should succeed quickly
    let mut handles = Vec::new();
    for i in 0..4 {
        match pool.submit(i).await {
            Ok(handle) => {
                println!("Successfully submitted job with input: {}", i);
                handles.push((i, handle));
            },
            Err(err) => println!("Failed to submit job with input {}: {}", i, err),
        }
    }
    
    // Submit a job that will return an error
    match pool.submit(-1).await {
        Ok(handle) => {
            println!("Successfully submitted job with input: -1");
            handles.push((-1, handle));
        },
        Err(err) => println!("Failed to submit job with input -1: {}", err),
    }
    
    // Submit a job that will timeout
    match pool.submit(7).await {
        Ok(handle) => {
            println!("Successfully submitted job with input: 7");
            handles.push((7, handle));
        },
        Err(err) => println!("Failed to submit job with input 7: {}", err),
    }
    
    // Try to submit more jobs than capacity to demonstrate backpressure
    println!("\n--- Demonstrating backpressure ---");
    for i in 10..15 {
        match pool.submit(i).await {
            Ok(handle) => {
                println!("Successfully submitted job with input: {}", i);
                handles.push((i, handle));
            },
            Err(err) => println!("Failed to submit job with input {}: {}", i, err),
        }
    }

    // Wait for some jobs to complete to free up capacity
    println!("\n--- Waiting for some jobs to complete ---");
    tokio::time::sleep(Duration::from_millis(300)).await;
    println!("Current available capacity: {}", pool.available_capacity());
    
    // Try submitting again after some jobs have completed
    println!("\n--- Submitting after some capacity freed up ---");
    for i in 15..18 {
        match pool.submit(i).await {
            Ok(handle) => {
                println!("Successfully submitted job with input: {}", i);
                handles.push((i, handle));
            },
            Err(err) => println!("Failed to submit job with input {}: {}", i, err),
        }
    }

    // Demonstrate custom timeout
    println!("\n--- Demonstrating custom timeout ---");
    match pool.submit_with_timeout(8, Some(Duration::from_millis(700))).await {
        Ok(handle) => {
            println!("Successfully submitted job with input 8 and custom timeout");
            handles.push((8, handle));
        },
        Err(err) => println!("Failed to submit job with input 8: {}", err),
    }

    // Wait for all jobs to complete and check results
    println!("\n--- Job Results ---");
    for (input, handle) in handles {
        match handle.wait().await {
            Ok(result) => println!("Job with input {} completed successfully with result: {}", input, result),
            Err(err) => println!("Job with input {} failed with error: {}", input, err),
        }
    }

    // Print statistics
    println!("\n--- Statistics ---");
    println!("Total jobs completed: {}", completed_jobs.load(Ordering::SeqCst));
    println!("Final available capacity: {}", pool.available_capacity());

    // Shutdown the pool
    println!("\n--- Shutting down the pool ---");
    pool.shutdown().await;
    println!("Pool shutdown complete");

    Ok(())
}
