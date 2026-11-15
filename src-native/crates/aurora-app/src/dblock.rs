//! The library's lock, timed.
//!
//! Every command and every background job reaches the database through one
//! `parking_lot::Mutex<Connection>`, reads included (`services.rs`). That makes the
//! interface exactly as quick as the slowest thing holding it: a page that asks for
//! twenty rows waits behind a refresh writing twenty thousand. Nothing measured it, so
//! "the whole app lags when I click through it" had no number and no culprit.
//!
//! This is a drop-in for that mutex which does: it notes where each lock was taken
//! (`#[track_caller]`), how long the caller waited for it, and how long it was then
//! held, and logs the outliers with their source line. The happy path costs two
//! `Instant::now()` calls.

use std::ops::{Deref, DerefMut};
use std::panic::Location;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, MutexGuard};

/// Held longer than this, and whoever is waiting behind it has visibly waited.
const SLOW_HOLD: Duration = Duration::from_millis(100);
/// Waited longer than this for the lock, and a click was not answered on time.
const SLOW_WAIT: Duration = Duration::from_millis(100);

pub struct TimedMutex<T> {
    inner: Mutex<T>,
}

impl<T> TimedMutex<T> {
    pub const fn new(value: T) -> Self {
        Self {
            inner: Mutex::new(value),
        }
    }

    #[track_caller]
    pub fn lock(&self) -> TimedGuard<'_, T> {
        let at = Location::caller();
        let asked = Instant::now();
        let guard = self.inner.lock();
        let got = Instant::now();
        let waited = got - asked;
        if waited >= SLOW_WAIT {
            tracing::warn!(
                target: "aurora_app::dblock",
                waited_ms = waited.as_millis() as u64,
                at = %at,
                "waited for the library lock"
            );
        }
        TimedGuard { guard, at, got }
    }

    /// For tests that need to see whether the lock is free without hanging on it.
    #[track_caller]
    pub fn try_lock_for(&self, timeout: Duration) -> Option<TimedGuard<'_, T>> {
        let at = Location::caller();
        let guard = self.inner.try_lock_for(timeout)?;
        Some(TimedGuard {
            guard,
            at,
            got: Instant::now(),
        })
    }

    pub fn into_inner(self) -> T {
        self.inner.into_inner()
    }
}

pub struct TimedGuard<'a, T> {
    guard: MutexGuard<'a, T>,
    at: &'static Location<'static>,
    got: Instant,
}

impl<T> Deref for TimedGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.guard
    }
}

impl<T> DerefMut for TimedGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.guard
    }
}

impl<T> Drop for TimedGuard<'_, T> {
    fn drop(&mut self) {
        let held = self.got.elapsed();
        if held >= SLOW_HOLD {
            tracing::warn!(
                target: "aurora_app::dblock",
                held_ms = held.as_millis() as u64,
                at = %self.at,
                "held the library lock"
            );
        }
    }
}
