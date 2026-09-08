//! Logging fan-out and the in-app terminal buffer (design §4.9).
//!
//! One [`log::Log`] implementation forwards every record to the platform logger
//! (`env_logger` natively, `eframe::WebLogger` on wasm) **and** pushes the records the user
//! is meant to see into a ring buffer that `app.rs` renders in the terminal bar. Engine and
//! node code therefore only ever calls `log::{info, debug, warn, error}` — no `println!`, no
//! `web_sys::console`.
//!
//! Buffer rule: every record at `Info` or more severe from **any** target, plus every `Debug`
//! record whose target starts with `p2p_transfer` (so iroh/quinn debug noise stays out while
//! this crate's credit, epoch, frame-size and path lines get in).
//!
//! The buffer is user-visible and copyable, so nothing secret may reach it: never log a
//! capability, and never `Debug`-print a value that transitively contains one.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};

/// Lines kept in the terminal ring buffer.
pub const TERMINAL_CAPACITY: usize = 500;

/// Log targets of this crate get their `Debug` records into the terminal buffer.
const CRATE_TARGET: &str = "p2p_transfer";

/// The shared ring buffer behind the in-app terminal view.
pub type TerminalBuffer = Arc<Mutex<VecDeque<String>>>;

static TERMINAL: OnceLock<TerminalBuffer> = OnceLock::new();

/// Handle to the terminal ring buffer. Safe to call before [`init_logging`].
pub fn terminal_buffer() -> TerminalBuffer {
    TERMINAL
        .get_or_init(|| Arc::new(Mutex::new(VecDeque::with_capacity(TERMINAL_CAPACITY))))
        .clone()
}

/// Install the fan-out logger. Idempotent: a second call is a no-op.
pub fn init_logging() {
    #[cfg(not(target_arch = "wasm32"))]
    let (inner, inner_filter): (Box<dyn log::Log + Send + Sync>, log::LevelFilter) = {
        let logger = env_logger::Builder::from_default_env().build();
        let filter = logger.filter();
        (Box::new(logger), filter)
    };
    #[cfg(target_arch = "wasm32")]
    let (inner, inner_filter): (Box<dyn log::Log + Send + Sync>, log::LevelFilter) = (
        Box::new(eframe::WebLogger::new(log::LevelFilter::Debug)),
        log::LevelFilter::Debug,
    );

    let fan_out = FanOut {
        inner,
        buffer: terminal_buffer(),
    };
    // The buffer needs `Debug` records from this crate, so the global max level must not cut
    // them off even when the platform logger asked for less.
    let max_level = inner_filter.max(log::LevelFilter::Debug);
    if log::set_boxed_logger(Box::new(fan_out)).is_ok() {
        log::set_max_level(max_level);
    }
}

struct FanOut {
    inner: Box<dyn log::Log + Send + Sync>,
    buffer: TerminalBuffer,
}

/// Whether this record belongs in the user-visible terminal buffer (design §4.9, m-f).
fn wants_buffer(metadata: &log::Metadata<'_>) -> bool {
    metadata.level() <= log::Level::Info
        || (metadata.level() == log::Level::Debug && metadata.target().starts_with(CRATE_TARGET))
}

impl log::Log for FanOut {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        self.inner.enabled(metadata) || wants_buffer(metadata)
    }

    fn log(&self, record: &log::Record<'_>) {
        // Both loggers installed here re-check `enabled` inside their own `log`, and the max
        // level is forced up to `Debug` for the terminal buffer — so guarding this call would
        // run the same directive match twice for every debug record iroh emits.
        self.inner.log(record);
        if wants_buffer(record.metadata()) {
            // Expand the record's arguments before taking the lock: formatting can be
            // arbitrarily expensive and must not block every other thread that logs.
            let line = format!("[{}] {}", record.level(), record.args());
            if let Ok(mut buffer) = self.buffer.lock() {
                while buffer.len() >= TERMINAL_CAPACITY {
                    buffer.pop_front();
                }
                buffer.push_back(line);
            }
        }
    }

    fn flush(&self) {
        self.inner.flush();
    }
}
