//! The driver for a machine where libmpv could not be loaded (E30).
//!
//! The app still runs and says why there is no picture, rather than opening an empty
//! window that looks like a broken stream.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::driver::{Driver, Event, Node, Waker};

pub const NO_ENGINE: &str = "this copy of Aurora has no video engine (libmpv could not be loaded)";

#[derive(Default)]
pub struct NoVideo {
    wake: Arc<(Mutex<bool>, Condvar)>,
    why: Option<String>,
}

impl NoVideo {
    /// `why` is what went wrong loading the real one, for the diagnostics page.
    pub fn new(why: Option<String>) -> Self {
        Self {
            wake: Arc::default(),
            why,
        }
    }
}

impl Driver for NoVideo {
    fn set(&mut self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }

    fn set_list(&mut self, _: &str, _: &[String]) -> Result<(), String> {
        Ok(())
    }

    fn command(&mut self, args: &[&str]) -> Result<Node, String> {
        match args.first().copied() {
            Some("loadfile") | Some("screenshot-raw") => Err(NO_ENGINE.into()),
            _ => Ok(Node::None),
        }
    }

    fn get(&mut self, _: &str) -> Option<Node> {
        None
    }

    fn observe(&mut self, _: &str) -> Result<(), String> {
        Ok(())
    }

    fn wait(&mut self, timeout: Duration) -> Option<Event> {
        let (lock, cv) = &*self.wake;
        let woken = lock.lock().unwrap_or_else(|p| p.into_inner());
        let mut woken = cv
            .wait_timeout_while(woken, timeout, |w| !*w)
            .unwrap_or_else(|p| p.into_inner())
            .0;
        *woken = false;
        None
    }

    fn waker(&self) -> Waker {
        let wake = self.wake.clone();
        Arc::new(move || {
            *wake.0.lock().unwrap_or_else(|p| p.into_inner()) = true;
            wake.1.notify_all();
        })
    }

    fn renders_video(&self) -> bool {
        false
    }

    fn describe(&self) -> (String, Option<String>) {
        ("none".into(), self.why.clone())
    }
}
