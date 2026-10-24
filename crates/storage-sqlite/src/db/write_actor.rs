use super::DbPool;
use crate::errors::StorageError;
use diesel::SqliteConnection;
use std::any::Any;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use wealthfolio_core::errors::{DatabaseError, Error, Result};

struct WriteJobResult {
    value: Box<dyn Any + Send + 'static>,
}

// Type alias for the job to be executed by the writer actor.
// It takes a mutable reference to a SqliteConnection and returns a Result.
// We use core::Result here since that's what callers expect.
type Job = Box<dyn FnOnce(&mut SqliteConnection) -> Result<WriteJobResult> + Send + 'static>;

/// A message for the writer actor: a job, or the request to stop.
///
/// Stopping travels through the same channel as the jobs so that it is observed
/// only after everything already queued has run — the actor drains naturally,
/// with no `select!` and no risk of dropping committed-but-unsent work.
#[allow(clippy::type_complexity)]
enum WriteMessage {
    Job(Job, oneshot::Sender<Result<Box<dyn Any + Send + 'static>>>),
    Shutdown(oneshot::Sender<()>),
}

/// Handle for sending jobs to the writer actor.
#[derive(Clone)]
pub struct WriteHandle {
    // Sender part of the MPSC channel to send jobs.
    // Each job is a boxed closure, and a oneshot sender is used for the reply.
    // The Box<dyn Any + Send> is used for type erasure of the job's return type.
    tx: mpsc::Sender<WriteMessage>,
}

/// Ownership of the writer actor's task.
///
/// The actor holds a pooled connection for the life of its task, so database
/// maintenance cannot proceed until that task has actually finished. Retaining
/// the `JoinHandle` is what makes "wait for it" possible at all.
pub struct WriterTask {
    join: tokio::task::JoinHandle<()>,
}

impl WriterTask {
    /// Waits for the actor's task to finish, after [`WriteHandle::shutdown`].
    pub async fn join(self) {
        if let Err(e) = self.join.await {
            log::warn!("Writer actor task did not exit cleanly: {}", e);
        }
    }
}

impl WriteHandle {
    /// Executes a database job on the writer actor's dedicated connection.
    ///
    /// # Arguments
    /// * `job`: A closure that takes a mutable reference to `SqliteConnection`
    ///   and performs database operations.
    ///
    /// # Returns
    /// A `Result<T>` containing the outcome of the job.
    pub async fn exec<F, T>(&self, job: F) -> Result<T>
    where
        F: FnOnce(&mut SqliteConnection) -> Result<T> + Send + 'static,
        T: Send + 'static + Any, // Add Any bound for T
    {
        self.exec_job(move |conn| {
            job(conn).map(|value| WriteJobResult {
                value: Box::new(value) as Box<dyn Any + Send>,
            })
        })
        .await
    }

    async fn exec_job<F, T>(&self, job: F) -> Result<T>
    where
        F: FnOnce(&mut SqliteConnection) -> Result<WriteJobResult> + Send + 'static,
        T: Send + 'static + Any,
    {
        let (ret_tx, ret_rx) = oneshot::channel();

        // The actor stops during database maintenance, so a closed channel is a
        // normal outcome that callers must be able to surface — not a panic.
        self.tx
            .send(WriteMessage::Job(Box::new(job), ret_tx))
            .await
            .map_err(|_| writer_stopped())?;

        ret_rx
            .await
            .map_err(|_| writer_stopped())?
            .map(|boxed: Box<dyn Any + Send + 'static>| {
                *boxed
                    .downcast::<T>()
                    .expect("a write job returns its own result type")
            })
    }

    /// Executes a database job using the centralized write transaction API.
    pub async fn exec_tx<F, T>(&self, job: F) -> Result<T>
    where
        F: FnOnce(&mut DbWriteTx<'_>) -> Result<T> + Send + 'static,
        T: Send + 'static + Any,
    {
        self.exec_job(move |conn| {
            let mut tx = DbWriteTx { conn };
            let value = job(&mut tx)?;
            Ok(WriteJobResult {
                value: Box::new(value) as Box<dyn Any + Send>,
            })
        })
        .await
    }

    /// Drains queued work, stops the actor and waits for its pooled connection
    /// to be released.
    ///
    /// Until this returns, the actor's connection keeps the database open — and
    /// that connection does not travel through `ServiceContext`, so dropping the
    /// context does not release it.
    pub async fn shutdown(&self) {
        let (ack_tx, ack_rx) = oneshot::channel();
        if self.tx.send(WriteMessage::Shutdown(ack_tx)).await.is_ok() {
            let _ = ack_rx.await;
        }
    }
}

fn writer_stopped() -> Error {
    Error::Database(DatabaseError::ConnectionFailed(
        "The database writer is not running. Database maintenance may be in progress.".to_string(),
    ))
}

pub struct DbWriteTx<'a> {
    conn: &'a mut SqliteConnection,
}

impl<'a> DbWriteTx<'a> {
    pub fn conn(&mut self) -> &mut SqliteConnection {
        self.conn
    }

    pub fn run<T, F>(&mut self, job: F) -> Result<T>
    where
        F: FnOnce(&mut SqliteConnection) -> Result<T>,
    {
        job(self.conn)
    }
}

/// Spawns a writer and detaches its task for tests and disposable pools.
/// Runtimes retain the task through `spawn_writer_with_task` for teardown.
pub fn spawn_writer(pool: DbPool) -> Result<WriteHandle> {
    spawn_writer_with_task(pool).map(|(handle, _task)| handle)
}

/// Spawns a serial database writer and retains ownership of its task.
pub fn spawn_writer_with_task(pool: DbPool) -> Result<(WriteHandle, WriterTask)> {
    fn acquire_writer_connection(pool: &DbPool) -> Result<super::DbConnection> {
        const PER_ATTEMPT_TIMEOUT: Duration = Duration::from_millis(800);
        const RETRY_SLEEP: Duration = Duration::from_millis(200);
        const MAX_TOTAL_WAIT: Duration = Duration::from_secs(8);

        let start = Instant::now();
        let mut attempts: u32 = 0;
        let mut last_err: Option<String> = None;

        while start.elapsed() < MAX_TOTAL_WAIT {
            attempts += 1;
            match pool.get_timeout(PER_ATTEMPT_TIMEOUT) {
                Ok(conn) => return Ok(conn),
                Err(e) => {
                    last_err = Some(e.to_string());
                    log::warn!(
                        "Writer actor init: pool.get_timeout() attempt {} failed ({}), retrying...",
                        attempts,
                        e
                    );
                    std::thread::sleep(RETRY_SLEEP);
                }
            }
        }

        let reason = last_err.unwrap_or_else(|| "unknown pool acquisition error".to_string());
        Err(Error::Database(DatabaseError::ConnectionFailed(format!(
            "Failed to initialize writer connection after {} attempts within {:?}: {}",
            attempts, MAX_TOTAL_WAIT, reason
        ))))
    }

    let mut conn = acquire_writer_connection(&pool)?;

    // Create an MPSC channel for sending jobs to the actor.
    // The channel is bounded; 1024 is an arbitrary size.
    let (tx, mut rx) = mpsc::channel::<WriteMessage>(1024);

    let join = tokio::spawn(async move {
        let mut shutdown_ack: Option<oneshot::Sender<()>> = None;

        // Loop to receive and process jobs.
        while let Some(message) = rx.recv().await {
            let (job, reply_tx) = match message {
                WriteMessage::Job(job, reply_tx) => (job, reply_tx),
                WriteMessage::Shutdown(ack) => {
                    shutdown_ack = Some(ack);
                    break;
                }
            };

            // Execute the job within an immediate database transaction.
            // We wrap the job to return StorageError which implements From<diesel::result::Error>.
            // Then convert back to core::Error at the boundary.
            let result: Result<WriteJobResult> = conn
                .immediate_transaction::<_, StorageError, _>(|c| {
                    // Call the job and convert its error to StorageError if needed
                    job(c).map_err(StorageError::from)
                })
                .map_err(|e: StorageError| e.into());

            let result = result.map(|job_result| job_result.value);

            // Send the result back to the requester.
            // Ignore error if the receiver has dropped (e.g., request timed out or was cancelled).
            let _ = reply_tx.send(result);
        }
        // If rx.recv() returns None, it means every sender (WriteHandle) was
        // dropped, so the actor can terminate.

        // Release the pooled connection before acknowledging, so that a caller
        // awaiting the acknowledgement knows the database handle is gone.
        drop(conn);
        if let Some(ack) = shutdown_ack {
            let _ = ack.send(());
        }
    });

    Ok((WriteHandle { tx }, WriterTask { join }))
}

// Note: DbConnection (PooledConnection) derefs to SqliteConnection.
// The immediate_transaction method is on SqliteConnection via the Connection trait.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DbAccess;
    use tempfile::tempdir;

    fn setup_pool() -> DbPool {
        let app_data = tempdir()
            .expect("tempdir")
            .keep()
            .to_string_lossy()
            .to_string();
        // An explicit path: `get_db_path` resolves to `$DATABASE_URL` when that
        // is set, which would point the test at a real database.
        let access = DbAccess::plaintext(format!("{app_data}/app.db"));
        access.prepare().expect("init db");
        access.run_migrations().expect("migrate db");
        access.create_pool().expect("create pool").as_ref().clone()
    }

    #[tokio::test]
    async fn shutdown_releases_the_pooled_connection_and_fails_later_writes() {
        let pool = setup_pool();
        let (writer, task) = spawn_writer_with_task(pool.clone()).expect("spawn writer");

        writer
            .exec_tx(|tx| tx.run(|_conn| Ok(())))
            .await
            .expect("write before shutdown");

        writer.shutdown().await;
        task.join().await;

        // The actor's connection is what would otherwise keep the database open
        // through maintenance, so it must be back in the pool by now.
        assert_eq!(pool.state().idle_connections, pool.state().connections);

        // A write after shutdown is an error, not a panic: maintenance stops the
        // actor while commands may still be unwinding.
        let result = writer.exec_tx(|tx| tx.run(|_conn| Ok(()))).await;
        assert!(result.is_err(), "writes after shutdown must fail cleanly");
    }

    #[tokio::test]
    async fn shutdown_drains_work_already_queued() {
        let (writer, task) = spawn_writer_with_task(setup_pool()).expect("spawn writer");

        let queued = writer.exec_tx(|tx| {
            tx.run(|_conn| {
                std::thread::sleep(std::time::Duration::from_millis(50));
                Ok(7u8)
            })
        });
        let stopped = writer.shutdown();

        let (queued, ()) = tokio::join!(queued, stopped);
        assert_eq!(queued.expect("queued write must still run"), 7);
        task.join().await;
    }
}
