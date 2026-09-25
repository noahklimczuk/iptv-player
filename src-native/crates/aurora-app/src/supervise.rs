//! Keeping the background loops alive, and saying so when they nearly were not.
//!
//! Three threads do work nobody asks for: the player heartbeat every 250 ms, the DVR
//! scheduler every ten seconds, and the update check once after launch. Each was a
//! bare `loop { … }` inside a `spawn`, with no `catch_unwind` and no panic hook
//! anywhere in the process.
//!
//! A panic in any of them ended that thread, permanently, and said nothing. In a
//! release build `windows_subsystem = "windows"` means there is no console for the
//! default hook to print to, so the evidence went nowhere at all. What a viewer would
//! see is an OSD frozen on whatever it last showed, failover that stopped working, or
//! — worse, because there is no symptom until the programme is gone — a DVR that
//! silently never records anything again.
//!
//! Two things fix it. A panic hook so the payload and its location reach the log file,
//! and a wrapper that catches a panicking tick and keeps the loop running.

use std::panic::{catch_unwind, AssertUnwindSafe};

/// Send panics to the log, wherever the log happens to be.
///
/// The default hook writes to stderr, which a release build does not have. This one
/// goes through `tracing`, so it lands in `aurora.log` beside the library — the one
/// place someone reporting a bug can be asked to look.
pub fn log_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "an unknown location".to_string());
        tracing::error!("panic at {location}: {}", panic_message(info.payload()),);
        // Still call the old hook: in a debug build it prints the backtrace, which is
        // what a developer actually wants, and nothing here is a reason to lose it.
        previous(info);
    }));
}

/// The `&str` or `String` a panic carried, or a stand-in.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "a panic with no message".to_string()
    }
}

/// Run one iteration of a background loop, surviving a panic inside it.
///
/// Returns whether the body completed. The caller keeps looping either way: a tick
/// that panicked is one lost tick, and the alternative — the feature being over for
/// the rest of the session, quietly — is much worse. `AssertUnwindSafe` is the honest
/// claim here rather than a shortcut: every one of these bodies works through an
/// `Arc<Mutex<…>>`, and `parking_lot`'s mutexes do not poison, so state that was
/// half-written when the panic happened is state the next tick reads and corrects.
pub fn supervised<F: FnOnce()>(name: &str, body: F) -> bool {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(()) => true,
        Err(payload) => {
            tracing::error!(
                "the {name} thread panicked and was restarted: {}",
                panic_message(&*payload)
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// The bug: a panicking tick used to end its thread, so the feature stopped for
    /// the rest of the session with nothing said about it.
    #[test]
    fn a_panicking_tick_does_not_end_the_loop() {
        let ticks = AtomicUsize::new(0);
        // Quiet: this test panics on purpose and the default hook would print a
        // backtrace for each one.
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));

        for i in 0..5 {
            supervised("test", || {
                ticks.fetch_add(1, Ordering::SeqCst);
                if i == 2 {
                    panic!("the provider sent something impossible");
                }
            });
        }

        std::panic::set_hook(previous);
        assert_eq!(
            ticks.load(Ordering::SeqCst),
            5,
            "the loop stopped at the panicking tick"
        );
    }

    #[test]
    fn it_reports_whether_the_body_finished() {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));

        assert!(supervised("test", || {}));
        assert!(!supervised("test", || panic!("nope")));

        std::panic::set_hook(previous);
    }

    #[test]
    fn a_panic_message_is_readable_whatever_it_carried() {
        assert_eq!(panic_message(&"a str"), "a str");
        assert_eq!(panic_message(&String::from("a String")), "a String");
        assert_eq!(panic_message(&42u8), "a panic with no message");
    }
}
