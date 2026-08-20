//! High-resolution application timing.
//!
//! The application deliberately avoids using the GUI frame loop or the
//! midir-provided timestamp as its authoritative time source. Instead, a
//! monotonic high-resolution clock is sampled at the moment a message is
//! received (i.e. inside the real-time MIDI callback) and carried along with
//! the event as it travels to the GUI. This keeps display timing decoupled
//! from rendering and gives sub-millisecond fidelity.

use std::sync::Arc;
use std::time::Instant;

/// Produces high-resolution, monotonic application timestamps.
///
/// Implementations must be cheap to call inside a real-time MIDI callback.
pub trait Clock: Send + Sync {
    /// Returns microseconds elapsed since the application's monotonic start.
    fn now_micros(&self) -> u64;
}

/// Default production clock backed by [`std::time::Instant`].
///
/// On virtually all modern platforms this uses a monotonic high-resolution
/// timer (e.g. `QueryPerformanceCounter` on Windows, `mach_absolute_time` on
/// macOS, `clock_gettime(CLOCK_MONOTONIC)` on Linux). It is not affected by
/// wall-clock adjustments.
#[derive(Clone, Debug)]
pub struct SystemClock {
    start: Instant,
}

impl SystemClock {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now_micros(&self) -> u64 {
        self.start.elapsed().as_micros() as u64
    }
}

/// Thread-safe handle to a [`Clock`], shared between the MIDI callback thread
/// and the GUI thread.
pub type SharedClock = Arc<dyn Clock>;
