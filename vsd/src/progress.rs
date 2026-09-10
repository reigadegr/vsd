//! Progress rendering, speed calculation, and status reporting utilities.

use colored::Colorize;
use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio::task::JoinHandle;

/// The snapshot state of a progress indicator used for callbacks.
pub struct ProgressState {
    /// The label or identifier of the progress item (e.g. stream name/id).
    pub label: String,
    /// The number of parts downloaded so far.
    pub downloaded_parts: usize,
    /// The total number of parts expected.
    pub total_parts: usize,
    /// The total number of bytes downloaded so far.
    pub downloaded_bytes: usize,
    /// The estimated total size of the resource in bytes.
    pub estimated_bytes: usize,
    /// The current download speed in bits per second (bps).
    pub speed_bps: f64,
    /// The estimated time of arrival (ETA) in seconds.
    pub eta_seconds: usize,
    /// The progress percentage completed (0.0 to 100.0).
    pub percent: f32,
}

struct ProgressInner {
    counter: usize,
    id: String,
    session_counter: usize,
    session_bytes: usize,
    total: usize,
    timer: Instant,
    total_bytes: usize,
}

impl ProgressInner {
    fn state(&self) -> ProgressState {
        let estimated_bytes = if self.counter > 0 {
            ((self.total_bytes as f64 / self.counter as f64) * self.total as f64) as usize
        } else {
            0
        };

        let percent = if self.total > 0 {
            (self.counter as f64 / self.total as f64 * 100.0) as f32
        } else {
            100.0
        };

        let elapsed_secs = self.timer.elapsed().as_secs_f64();

        let speed_bps = if self.session_counter > 0 {
            self.session_bytes as f64 / elapsed_secs
        } else {
            0.0
        };

        let rate = if self.session_counter > 0 {
            self.session_counter as f64 / elapsed_secs
        } else {
            0.0
        };

        let eta_seconds = if rate > 0.0 {
            (self.total.saturating_sub(self.counter) as f64 / rate) as usize
        } else {
            0
        };

        ProgressState {
            label: self.id.clone(),
            downloaded_parts: self.counter,
            total_parts: self.total,
            downloaded_bytes: self.total_bytes,
            estimated_bytes,
            speed_bps,
            eta_seconds,
            percent,
        }
    }
}

/// A trait that allows registering callbacks to receive progress updates.
pub trait ProgressCallback: Send + Sync {
    /// Triggered periodically to report the current progress snapshot state.
    fn on_progress(&self, state: &ProgressState);

    /// Triggered when the operation is successfully completed.
    fn on_finish(&self, state: &ProgressState) {
        self.on_progress(state);
    }
}

/// Thread-safe progress manager tracking downloaded parts, bytes, speed, and ETA.
#[derive(Clone)]
pub(crate) struct Progress {
    inner: Arc<Mutex<ProgressInner>>,
    callback: Option<Arc<dyn ProgressCallback>>,
}

impl Progress {
    /// Creates a new progress tracker with a label ID and estimated total steps/parts.
    pub fn new(id: &str, total: usize, callback: Option<Arc<dyn ProgressCallback>>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ProgressInner {
                counter: 0,
                id: id.to_owned(),
                session_counter: 0,
                session_bytes: 0,
                total,
                timer: Instant::now(),
                total_bytes: 0,
            })),
            callback,
        }
    }

    /// Dynamically updates the total parts expected.
    pub fn update_total(&self, total: usize) {
        let mut inner = self.inner.lock().unwrap();
        inner.total = total;
    }

    /// Bumps the downloaded parts counter and updates speed/byte statistics.
    pub fn update(&self, size: usize) {
        let mut inner = self.inner.lock().unwrap();
        inner.counter += 1;
        inner.session_counter += 1;
        inner.total_bytes += size;
        inner.session_bytes += size;

        if inner.counter > inner.total {
            inner.total = inner.counter;
        }
    }

    /// Skips a chunk of bytes, incrementing completed parts without affecting the current download speed/session statistics.
    pub fn skip(&self, size: usize) {
        let mut inner = self.inner.lock().unwrap();
        inner.counter += 1;
        inner.total_bytes += size;

        if inner.counter > inner.total {
            inner.total = inner.counter;
        }
    }

    /// Finishes the progress reporting, triggering the final callback or printing the final status.
    pub fn finish(&self) {
        let inner = self.inner.lock().unwrap();
        if let Some(cb) = &self.callback {
            cb.on_finish(&inner.state());
        } else {
            Self::render(&inner);
            eprintln!();
        }
    }

    /// Spawns a background task that periodically updates and prints the progress bar status.
    pub fn spawn(&self) -> JoinHandle<()> {
        let inner = self.inner.clone();
        let callback = self.callback.clone();

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                let done = {
                    let inner = inner.lock().unwrap();
                    if let Some(cb) = &callback {
                        cb.on_progress(&inner.state());
                    } else {
                        Self::render(&inner);
                    }
                    inner.counter >= inner.total
                };
                if done {
                    break;
                }
            }
        })
    }

    fn render(inner: &ProgressInner) {
        if inner.counter == 0 {
            return;
        }

        let state = inner.state();
        let stderr = io::stderr();
        let mut handle = stderr.lock();
        write!(
            handle,
            "\r\x1B[2K{}#({}) {}/~{}{} PT:{} DL:{} ETA:{}{}",
            "[".magenta(),
            state.label,
            ByteSize(state.downloaded_bytes),
            ByteSize(state.estimated_bytes),
            format!("({:.0}%)", state.percent).cyan(),
            format!("{}/{}", state.downloaded_parts, state.total_parts).cyan(),
            ByteSize(state.speed_bps as usize).to_string().green(),
            Eta(state.eta_seconds).to_string().yellow(),
            "]".magenta(),
        )
        .unwrap();
        handle.flush().unwrap();
    }
}

/// Helper struct for printing human-readable byte sizes.
pub struct ByteSize(pub usize);

impl std::fmt::Display for ByteSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const KIB: f64 = 1024.0;
        const MIB: f64 = KIB * 1024.0;
        const GIB: f64 = MIB * 1024.0;

        let bytes = self.0 as f64;

        if bytes >= GIB {
            write!(f, "{:.1}GiB", bytes / GIB)
        } else if bytes >= MIB {
            write!(f, "{:.1}MiB", bytes / MIB)
        } else if bytes >= KIB {
            write!(f, "{:.1}KiB", bytes / KIB)
        } else {
            write!(f, "{}B", self.0)
        }
    }
}

/// Helper struct for printing human-readable estimated time of arrival (ETA).
pub struct Eta(pub usize);

impl std::fmt::Display for Eta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let total_seconds = self.0;
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;

        if hours > 0 {
            write!(f, "{hours}h{minutes}m{seconds}s")
        } else if minutes > 0 {
            write!(f, "{minutes}m{seconds}s")
        } else {
            write!(f, "{seconds}s")
        }
    }
}
