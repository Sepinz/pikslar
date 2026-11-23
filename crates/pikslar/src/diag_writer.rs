//! The one thread that writes the saving log (`docs/diagnostics.md`), so recording a line never
//! waits on the disk. Lines go through a bounded queue; when it is full a line is dropped and
//! counted, and the count is written once there is room again.

use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How many lines may wait for the disk before new ones are dropped.
pub(crate) const QUEUE: usize = 256;

/// Where the writer thread puts what it is given.
pub(crate) trait Sink: Send + 'static {
    fn line(&mut self, line: &str);
    /// Lines dropped because the queue was full, written as one line.
    fn dropped(&mut self, count: u64);
    /// Lets go of the files and deletes them.
    fn delete(&mut self) -> io::Result<()>;
    /// Why the sink stopped writing, if it did.
    fn problem(&self) -> Option<String>;
}

enum Job {
    Line(String),
    Delete(SyncSender<io::Result<()>>),
    Flush(SyncSender<()>),
}

/// The sending side, shared by every thread that records.
pub(crate) struct Writer {
    tx: SyncSender<Job>,
    dropped: Arc<AtomicU64>,
    problem: Arc<Mutex<Option<String>>>,
}

impl Writer {
    /// Starts the thread. None if it cannot start: recording then does nothing.
    pub fn spawn(sink: impl Sink) -> Option<Writer> {
        let (tx, rx) = sync_channel(QUEUE);
        let dropped = Arc::new(AtomicU64::new(0));
        let problem = Arc::new(Mutex::new(None));
        let (count, said) = (Arc::clone(&dropped), Arc::clone(&problem));
        std::thread::Builder::new()
            .name("diagnostics".into())
            .spawn(move || serve(sink, &rx, &count, &said))
            .ok()?;
        Some(Writer {
            tx,
            dropped,
            problem,
        })
    }

    /// Queues `line` without waiting. A full queue drops it and counts it.
    pub fn send(&self, line: String) {
        if let Err(TrySendError::Full(_)) = self.tx.try_send(Job::Line(line)) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Lines dropped and not yet written as a count.
    #[cfg(test)]
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn problem(&self) -> Option<String> {
        self.problem
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Waits up to `within` for what is queued to be written.
    pub fn flush(&self, within: Duration) -> bool {
        let (tx, rx) = sync_channel(1);
        self.ask(Job::Flush(tx), within, &rx).is_some()
    }

    /// Deletes the log, waiting up to `within` for the writer to let go of it.
    pub fn delete(&self, within: Duration) -> io::Result<()> {
        let (tx, rx) = sync_channel(1);
        self.ask(Job::Delete(tx), within, &rx).unwrap_or_else(|| {
            Err(io::Error::other(
                "the log is busy being written; try again in a moment",
            ))
        })
    }

    fn ask<T>(&self, job: Job, within: Duration, rx: &Receiver<T>) -> Option<T> {
        let end = Instant::now() + within;
        let mut job = job;
        loop {
            match self.tx.try_send(job) {
                Ok(()) => break,
                Err(TrySendError::Full(back)) if Instant::now() < end => {
                    job = back;
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(_) => return None,
            }
        }
        rx.recv_timeout(end.saturating_duration_since(Instant::now()))
            .ok()
    }
}

fn serve(
    mut sink: impl Sink,
    rx: &Receiver<Job>,
    dropped: &AtomicU64,
    problem: &Mutex<Option<String>>,
) {
    while let Ok(job) = rx.recv() {
        let count = dropped.swap(0, Ordering::Relaxed);
        if count > 0 {
            sink.dropped(count);
        }
        match job {
            Job::Line(line) => sink.line(&line),
            Job::Delete(reply) => {
                let _ = reply.send(sink.delete());
            }
            Job::Flush(reply) => {
                let _ = reply.send(());
            }
        }
        *problem.lock().unwrap_or_else(PoisonError::into_inner) = sink.problem();
    }
}
