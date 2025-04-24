//! # rust-chanpool
//!
//! `rust-chanpool` is a library for channel pooling in async Rust applications.
//! It manages reusable task workers with backpressure awareness, allowing for
//! efficient processing of async jobs.
//!
//! ## Features
//!
//! - Channel pooling for async jobs
//! - Management of reusable task workers
//! - Backpressure awareness to prevent overwhelming workers
//! - Configurable pool size and job queue capacity
//!
//! ## Author
//!
//! Saeed Ghanbari (https://github.com/sgh370)

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use futures::future::Future;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::timeout;

mod worker;
pub use worker::Worker;

/// Errors that can occur when using the channel pool.
#[derive(Error, Debug)]
pub enum PoolError {
    /// Error when the pool is at capacity and cannot accept more jobs
    #[error("pool is at capacity")]
    PoolAtCapacity,

    /// Error when a job is submitted but the worker has been shut down
    #[error("worker is shut down")]
    WorkerShutDown,

    /// Error when a job times out
    #[error("job timed out")]
    Timeout,

    /// Error when a job is cancelled
    #[error("job was cancelled")]
    Cancelled,

    /// Error when a job fails with a specific error
    #[error("job failed: {0}")]
    JobFailed(String),
}

/// Configuration for the channel pool.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// The number of workers in the pool
    pub workers: usize,
    /// The capacity of each worker's job queue
    pub queue_capacity: usize,
    /// The default timeout for jobs
    pub default_timeout: Option<Duration>,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            workers: num_cpus::get(),
            queue_capacity: 100,
            default_timeout: None,
        }
    }
}

/// A job that can be submitted to the pool.
#[derive(Debug)]
pub struct Job<T, R> {
    /// The task to be executed
    pub task: T,
    /// The response channel to send the result back
    pub response_tx: oneshot::Sender<Result<R, PoolError>>,
    /// The timeout for this job
    pub timeout: Option<Duration>,
}

impl<T, R> fmt::Display for Job<T, R>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Job({:?})", self.task)
    }
}

/// A handle to a job that has been submitted to the pool.
#[derive(Debug)]
pub struct JobHandle<R> {
    /// The receiver for the job result
    pub rx: oneshot::Receiver<Result<R, PoolError>>,
    /// The timeout for this job
    pub timeout: Option<Duration>,
}

impl<R> JobHandle<R> {
    /// Wait for the job to complete with an optional timeout.
    pub async fn wait(self) -> Result<R, PoolError> {
        let rx = self.rx;
        
        match self.timeout {
            Some(timeout_duration) => {
                match timeout(timeout_duration, rx).await {
                    Ok(result) => result.map_err(|_| PoolError::Cancelled)?.map_err(|e| e),
                    Err(_) => Err(PoolError::Timeout),
                }
            },
            None => rx.await.map_err(|_| PoolError::Cancelled)?.map_err(|e| e),
        }
    }
}

/// The main channel pool that manages workers and distributes jobs.
#[derive(Debug)]
pub struct ChannelPool<T, R, F, Fut>
where
    T: Send + std::fmt::Debug + 'static,
    R: Send + 'static,
    F: Fn(T) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<R, PoolError>> + Send + 'static,
{
    /// The worker handles
    workers: Vec<JoinHandle<()>>,
    /// The job senders for each worker
    job_txs: Vec<mpsc::Sender<Job<T, R>>>,
    /// The semaphore to limit concurrent jobs
    semaphore: Arc<Semaphore>,
    /// The configuration for the pool
    config: PoolConfig,
    /// The job handler function
    _job_handler: Arc<F>,
    /// Phantom data for unused type parameters
    _phantom: std::marker::PhantomData<Fut>,
}

impl<T, R, F, Fut> ChannelPool<T, R, F, Fut>
where
    T: Send + std::fmt::Debug + 'static,
    R: Send + 'static,
    F: Fn(T) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<R, PoolError>> + Send + 'static,
{
    /// Create a new channel pool with the given job handler and configuration.
    pub fn new(job_handler: F, config: PoolConfig) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.workers * config.queue_capacity));
        let mut workers = Vec::with_capacity(config.workers);
        let mut job_txs = Vec::with_capacity(config.workers);

        let job_handler = Arc::new(job_handler);

        for id in 0..config.workers {
            let (job_tx, job_rx) = mpsc::channel(config.queue_capacity);
            let worker_semaphore = semaphore.clone();
            let worker_handler = job_handler.clone();

            let worker = Worker::new(id, job_rx, worker_handler, worker_semaphore);
            let handle = tokio::spawn(async move {
                worker.run().await;
            });

            workers.push(handle);
            job_txs.push(job_tx);
        }

        Self {
            workers,
            job_txs,
            semaphore,
            config,
            _job_handler: job_handler,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Submit a job to the pool and get a handle to await its result.
    pub async fn submit(&self, task: T) -> Result<JobHandle<R>, PoolError> {
        self.submit_with_timeout(task, self.config.default_timeout).await
    }

    /// Submit a job to the pool with a specific timeout and get a handle to await its result.
    pub async fn submit_with_timeout(
        &self,
        task: T,
        timeout: Option<Duration>,
    ) -> Result<JobHandle<R>, PoolError> {
        // Create a channel for the response
        let (response_tx, response_rx) = oneshot::channel();

        // Create the job
        let job = Job {
            task,
            response_tx,
            timeout,
        };

        // Select a worker using round-robin
        let worker_idx = fastrand::usize(..self.job_txs.len());
        let job_tx = &self.job_txs[worker_idx];

        // Check if the channel is full (would block)
        if job_tx.capacity() == 0 {
            return Err(PoolError::PoolAtCapacity);
        }

        // Try to acquire a permit from the semaphore to ensure we don't exceed capacity
        let permit = match self.semaphore.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => return Err(PoolError::PoolAtCapacity),
        };

        // Send the job to the worker
        if job_tx.send(job).await.is_err() {
            // Drop the permit if we can't send the job
            drop(permit);
            return Err(PoolError::WorkerShutDown);
        }

        // The permit will be dropped when the job completes
        tokio::spawn(async move {
            let _ = permit;
        });

        Ok(JobHandle {
            rx: response_rx,
            timeout,
        })
    }

    /// Shut down the pool and wait for all workers to complete.
    pub async fn shutdown(self) {
        drop(self.job_txs);
        for handle in self.workers {
            let _ = handle.await;
        }
    }

    /// Get the current number of available slots in the pool.
    pub fn available_capacity(&self) -> usize {
        self.semaphore.available_permits()
    }

    /// Get the total capacity of the pool.
    pub fn total_capacity(&self) -> usize {
        self.config.workers * self.config.queue_capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_basic_job_submission() {
        let config = PoolConfig {
            workers: 2,
            queue_capacity: 10,
            default_timeout: None,
        };

        let pool = ChannelPool::new(|n: u32| async move { Ok(n * 2) }, config);

        let handle = pool.submit(21).await.unwrap();
        let result = handle.wait().await.unwrap();

        assert_eq!(result, 42);

        pool.shutdown().await;
    }

    #[tokio::test]
    async fn test_backpressure() {
        let config = PoolConfig {
            workers: 1,
            queue_capacity: 1,
            default_timeout: None,
        };

        let pool = ChannelPool::new(
            |n: u32| async move {
                sleep(Duration::from_millis(500)).await;
                Ok(n)
            },
            config,
        );

        // The semaphore capacity is workers * queue_capacity = 1 * 1 = 1
        let handle1 = pool.submit(1).await.unwrap();
        
        // Try to submit a second job - this might succeed or fail depending on timing
        let result2 = pool.submit(2).await;
        
        // Submit third job - should fail due to backpressure
        let result3 = pool.submit(3).await;
        assert!(matches!(result3, Err(PoolError::PoolAtCapacity)));

        // Wait for the first job to complete
        let _ = handle1.wait().await.unwrap();
        
        // If the second job was accepted, wait for it to complete
        if let Ok(handle2) = result2 {
            let _ = handle2.wait().await.unwrap();
        }
        
        // Now we should be able to submit another job
        let handle3 = pool.submit(3).await.unwrap();
        let result = handle3.wait().await.unwrap();
        assert_eq!(result, 3);

        pool.shutdown().await;
    }

    #[tokio::test]
    async fn test_timeout() {
        let config = PoolConfig {
            workers: 1,
            queue_capacity: 1,
            default_timeout: Some(Duration::from_millis(50)),
        };

        let pool = ChannelPool::new(
            |_: u32| async move {
                sleep(Duration::from_millis(200)).await;
                Ok(42)
            },
            config,
        );

        let handle = pool.submit(1).await.unwrap();
        let result = handle.wait().await;
        
        assert!(matches!(result, Err(PoolError::Timeout)));

        pool.shutdown().await;
    }

    #[tokio::test]
    async fn test_job_error_handling() {
        let config = PoolConfig {
            workers: 1,
            queue_capacity: 1,
            default_timeout: None,
        };

        let pool = ChannelPool::new(
            |n: i32| async move {
                if n < 0 {
                    Err(PoolError::JobFailed("negative input".to_string()))
                } else {
                    Ok(n * 2)
                }
            },
            config,
        );

        // Test successful job
        let handle1 = pool.submit(5).await.unwrap();
        let result1 = handle1.wait().await.unwrap();
        assert_eq!(result1, 10);

        // Test failed job
        let handle2 = pool.submit(-5).await.unwrap();
        let result2 = handle2.wait().await;
        assert!(matches!(result2, Err(PoolError::JobFailed(msg)) if msg == "negative input"));

        pool.shutdown().await;
    }

    #[tokio::test]
    async fn test_parallel_processing() {
        let counter = Arc::new(AtomicUsize::new(0));
        let config = PoolConfig {
            workers: 4,
            queue_capacity: 10,
            default_timeout: None,
        };

        let counter_clone = counter.clone();
        let pool = ChannelPool::new(
            move |_: u32| {
                let counter = counter_clone.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    sleep(Duration::from_millis(50)).await;
                    Ok(counter.load(Ordering::SeqCst))
                }
            },
            config,
        );

        // Submit multiple jobs in parallel
        let mut handles = Vec::new();
        for _ in 0..10 {
            let handle = pool.submit(1).await.unwrap();
            handles.push(handle);
        }

        // Wait for all jobs to complete
        let mut results = Vec::new();
        for handle in handles {
            results.push(handle.wait().await.unwrap());
        }

        // Verify that all jobs were processed
        assert_eq!(counter.load(Ordering::SeqCst), 10);
        
        // Verify that jobs were processed in parallel (results should not be sequential 1,2,3,...)
        let sequential = (1..=10).collect::<Vec<_>>();
        assert_ne!(results, sequential);

        pool.shutdown().await;
    }
}
