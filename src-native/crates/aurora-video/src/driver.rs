//! The seam between the engine and mpv.
//!
//! The engine never calls mpv itself; it calls a [`Driver`]. On Windows that is
//! libmpv (`libmpv.rs`). In tests it is [`crate::fake::FakeDriver`], which acts out
//! mpv's event sequences, so the engine's ordering, staleness and recovery rules are
//! tested on any machine and without a stream.
//!
//! A `Driver` is used from one thread only, the engine's. The one exception is
//! [`Waker`], which exists to be called from other threads.

use std::sync::Arc;
use std::time::Duration;

/// A value mpv hands back, as a tree. The subset of `mpv_node` the engine reads.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    None,
    Flag(bool),
    Int(i64),
    Double(f64),
    Str(String),
    Array(Vec<Node>),
    Map(Vec<(String, Node)>),
    Bytes(Vec<u8>),
}

impl Node {
    pub fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Node::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Node::Double(d) if d.is_finite() => Some(*d),
            Node::Int(i) => Some(*i as f64),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Node::Int(i) => Some(*i),
            Node::Double(d) if d.is_finite() => Some(*d as i64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Node::Flag(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Node::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> &[Node] {
        match self {
            Node::Array(items) => items,
            _ => &[],
        }
    }

    /// A string that says something: present and not blank.
    pub fn text(&self) -> Option<String> {
        self.as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }
}

/// Why a file ended. mpv's `mpv_end_file_reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    /// It played to the end.
    Eof,
    /// Something else was loaded over it, or it was stopped.
    Stop,
    /// It failed.
    Error,
    /// It was a playlist, and mpv is opening what it pointed at.
    Redirect,
    Quit,
}

/// One thing mpv said.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A file in the playlist began opening.
    StartFile { entry: i64 },
    /// The file was opened and its tracks are known.
    FileLoaded,
    /// Playback (re)started after a load or a seek: a frame is on screen.
    PlaybackRestart,
    /// A file ended. `error` is mpv's description when it failed.
    EndFile {
        entry: i64,
        reason: EndReason,
        error: Option<String>,
        /// On a redirect, the playlist entry mpv is opening instead.
        next: Option<i64>,
    },
    /// An observed property changed.
    Property { name: String, value: Node },
    /// A line of mpv's own log, at warning level or worse.
    Log { prefix: String, text: String },
    /// mpv is going away.
    Shutdown,
}

/// Interrupts [`Driver::wait`] from another thread, so a command queued while the
/// engine is waiting for mpv is seen at once rather than at the next timeout.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

pub trait Driver: Send + 'static {
    /// Set a property (or an option before the first file) from its string form.
    fn set(&mut self, name: &str, value: &str) -> Result<(), String>;

    /// Set a list property from its items, so an item containing a comma stays whole.
    fn set_list(&mut self, name: &str, items: &[String]) -> Result<(), String>;

    /// Run a command and wait for its result.
    fn command(&mut self, args: &[&str]) -> Result<Node, String>;

    /// Read a property. `None` when it has no value now, which for most of them means
    /// "no file loaded" rather than an error.
    fn get(&mut self, name: &str) -> Option<Node>;

    /// Ask to be told whenever `name` changes, as [`Event::Property`].
    fn observe(&mut self, name: &str) -> Result<(), String>;

    /// The next event, waiting up to `timeout`. `None` when the wait ran out, or was cut
    /// short by the [`Waker`].
    fn wait(&mut self, timeout: Duration) -> Option<Event>;

    fn waker(&self) -> Waker;

    /// Whether this driver puts pictures on a screen. A build without libmpv cannot,
    /// and says so (E30).
    fn renders_video(&self) -> bool;

    /// The name and version for diagnostics.
    fn describe(&self) -> (String, Option<String>);
}
