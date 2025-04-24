use std::fmt;
use std::sync::Arc;

use futures::future::Future;
use tokio::sync::{mpsc, Semaphore};
use tracing::{debug, info, trace};

use crate::Job;
use crate::PoolError;

/// A worker that processes jobs from a channel.
#[derive(Debug)]
pub struct Worker<T, R, F, Fut>
where
    T: Send + std::fmt::Debug + 'static,
    R: Send + 'static,
    F: Fn(T) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<R, PoolError>> + Send + 'static,
{
    /// The worker's unique identifier
    id: usize,
    /// The channel to receive jobs from
    job_rx: mpsc::Receiver<Job<T, R>>,
    /// The handler function to process jobs
    handler: Arc<F>,
    /// The semaphore to track job slots (kept for future use)
    _semaphore: Arc<Semaphore>,
}

impl<T, R, F, Fut> Worker<T, R, F, Fut>
where
    T: Send + std::fmt::Debug + 'static,
    R: Send + 'static,
    F: Fn(T) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<R, PoolError>> + Send + 'static,
{
    /// Create a new worker with the given ID, job receiver, handler, and semaphore.
    pub fn new(
        id: usize,
        job_rx: mpsc::Receiver<Job<T, R>>,
        handler: Arc<F>,
        semaphore: Arc<Semaphore>,
    ) -> Self {
        Self {
            id,
            job_rx,
            handler,
            _semaphore: semaphore,
        }
    }

    /// Run the worker, processing jobs until the channel is closed.
    pub async fn run(mut self) {
        info!("Worker {} started", self.id);

        while let Some(job) = self.job_rx.recv().await {
            trace!("Worker {} received job: {}", self.id, job);
            
            // Process the job
            let handler = self.handler.clone();
            let task = job.task;
            let response_tx = job.response_tx;

            // Spawn a task to handle the job
            tokio::spawn(async move {
                let result = handler(task).await;
                
                // Send the result back
                if let Err(_) = response_tx.send(result) {
                    debug!("Failed to send job result: receiver dropped");
                }
            });
        }

        info!("Worker {} shutting down", self.id);
    }
}

impl<T, R, F, Fut> fmt::Display for Worker<T, R, F, Fut>
where
    T: Send + std::fmt::Debug + 'static,
    R: Send + 'static,
    F: Fn(T) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<R, PoolError>> + Send + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Worker({})", self.id)
    }
}
