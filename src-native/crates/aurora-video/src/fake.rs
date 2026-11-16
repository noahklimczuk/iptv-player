//! A stand-in for mpv, for tests here and in the host.
//!
//! It acts out the part of mpv the engine depends on: `loadfile` replaces the current
//! entry (ending it with `stop`) and starts a new one with a fresh id; `stop` ends the
//! current one. Everything after that — the file loading, the picture appearing, a
//! failure, a property changing — is said by the test through [`FakeMpv`], in whatever
//! order and timing the test is about.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::driver::{Driver, EndReason, Event, Node, Waker};

#[derive(Default)]
struct State {
    events: VecDeque<Event>,
    woken: bool,
    next_entry: i64,
    current: Option<i64>,
    /// Every `set`, in order: (name, value). Lists are joined with `|`.
    sets: Vec<(String, String)>,
    commands: Vec<Vec<String>>,
    props: HashMap<String, Node>,
    observed: Vec<String>,
    /// When set, `loadfile` fails with this.
    refuse_load: Option<String>,
    /// When false, `loadfile` returns no entry id, as an old mpv would.
    report_entry: bool,
    screenshot: Option<Node>,
}

#[derive(Clone, Default)]
pub struct FakeMpv {
    inner: Arc<(Mutex<State>, Condvar)>,
}

impl FakeMpv {
    pub fn new() -> Self {
        let f = Self::default();
        f.lock().report_entry = true;
        f.lock().next_entry = 1;
        f
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.inner.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The driver to hand to [`crate::Engine::start`]; this handle keeps working.
    pub fn driver(&self) -> FakeDriver {
        FakeDriver { mpv: self.clone() }
    }

    /// Queue an event, as if mpv had said it.
    pub fn say(&self, ev: Event) {
        self.lock().events.push_back(ev);
        self.inner.1.notify_all();
    }

    /// The entry mpv is on now.
    pub fn current(&self) -> Option<i64> {
        self.lock().current
    }

    /// The current file opened.
    pub fn loaded(&self) {
        self.say(Event::FileLoaded);
    }

    /// The first frame is up.
    pub fn restarted(&self) {
        self.say(Event::PlaybackRestart);
    }

    /// A property changed, and reads as `value` from now on.
    pub fn prop(&self, name: &str, value: Node) {
        self.lock().props.insert(name.into(), value.clone());
        self.say(Event::Property {
            name: name.into(),
            value,
        });
    }

    /// A property reads as `value`, without an event (for those the engine polls).
    pub fn holds(&self, name: &str, value: Node) {
        self.lock().props.insert(name.into(), value);
    }

    /// The current file fails.
    pub fn fail(&self, error: &str) {
        let entry = self.lock().current.take();
        if let Some(entry) = entry {
            self.say(Event::EndFile {
                entry,
                reason: EndReason::Error,
                error: Some(error.into()),
                next: None,
            });
        }
    }

    pub fn log(&self, text: &str) {
        self.say(Event::Log {
            prefix: "stream".into(),
            text: text.into(),
        });
    }

    /// One video track and one audio track, as a typical file has.
    pub fn video_tracks() -> Node {
        Node::Array(vec![
            track(1, "video", None, None, true),
            track(1, "audio", Some("eng"), Some("Main"), true),
            track(2, "audio", Some("fra"), None, false),
            track(1, "sub", Some("eng"), Some("SDH"), false),
        ])
    }

    /// Opened, tracks known, first frame up: a file that is playing.
    pub fn plays(&self) {
        self.loaded();
        self.prop("track-list", Self::video_tracks());
        self.restarted();
    }

    pub fn refuse_loads(&self, why: Option<&str>) {
        self.lock().refuse_load = why.map(str::to_string);
    }

    pub fn report_entries(&self, yes: bool) {
        self.lock().report_entry = yes;
    }

    pub fn screenshot(&self, node: Node) {
        self.lock().screenshot = Some(node);
    }

    pub fn sets(&self) -> Vec<(String, String)> {
        self.lock().sets.clone()
    }

    /// The last value `name` was set to.
    pub fn last_set(&self, name: &str) -> Option<String> {
        self.lock()
            .sets
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
    }

    pub fn commands(&self) -> Vec<Vec<String>> {
        self.lock().commands.clone()
    }

    pub fn observed(&self) -> Vec<String> {
        self.lock().observed.clone()
    }

    /// Wait until `f` holds of the commands sent, or two seconds pass.
    pub fn wait_for_command(&self, f: impl Fn(&[String]) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if self.lock().commands.iter().any(|c| f(c)) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }
}

pub fn track(id: i64, kind: &str, lang: Option<&str>, title: Option<&str>, selected: bool) -> Node {
    let mut m = vec![
        ("id".to_string(), Node::Int(id)),
        ("type".to_string(), Node::Str(kind.into())),
        ("selected".to_string(), Node::Flag(selected)),
        ("default".to_string(), Node::Flag(selected)),
    ];
    if let Some(l) = lang {
        m.push(("lang".into(), Node::Str(l.into())));
    }
    if let Some(t) = title {
        m.push(("title".into(), Node::Str(t.into())));
    }
    Node::Map(m)
}

pub struct FakeDriver {
    mpv: FakeMpv,
}

impl Driver for FakeDriver {
    fn set(&mut self, name: &str, value: &str) -> Result<(), String> {
        let mut s = self.mpv.lock();
        s.sets.push((name.into(), value.into()));
        s.props.insert(name.into(), Node::Str(value.into()));
        Ok(())
    }

    fn set_list(&mut self, name: &str, items: &[String]) -> Result<(), String> {
        self.mpv.lock().sets.push((name.into(), items.join("|")));
        Ok(())
    }

    fn command(&mut self, args: &[&str]) -> Result<Node, String> {
        let mut s = self.mpv.lock();
        s.commands
            .push(args.iter().map(|a| a.to_string()).collect());
        match args.first().copied() {
            Some("loadfile") => {
                if let Some(why) = s.refuse_load.clone() {
                    return Err(why);
                }
                if let Some(old) = s.current.take() {
                    s.events.push_back(Event::EndFile {
                        entry: old,
                        reason: EndReason::Stop,
                        error: None,
                        next: None,
                    });
                }
                let id = s.next_entry;
                s.next_entry += 1;
                s.current = Some(id);
                s.events.push_back(Event::StartFile { entry: id });
                drop(s);
                self.mpv.inner.1.notify_all();
                let reported = self.mpv.lock().report_entry;
                Ok(if reported {
                    Node::Map(vec![("playlist_entry_id".into(), Node::Int(id))])
                } else {
                    Node::None
                })
            }
            Some("stop") => {
                if let Some(old) = s.current.take() {
                    s.events.push_back(Event::EndFile {
                        entry: old,
                        reason: EndReason::Stop,
                        error: None,
                        next: None,
                    });
                }
                Ok(Node::None)
            }
            Some("screenshot-raw") => s.screenshot.clone().ok_or_else(|| "no picture".into()),
            _ => Ok(Node::None),
        }
    }

    fn get(&mut self, name: &str) -> Option<Node> {
        self.mpv.lock().props.get(name).cloned()
    }

    fn observe(&mut self, name: &str) -> Result<(), String> {
        self.mpv.lock().observed.push(name.into());
        Ok(())
    }

    fn wait(&mut self, timeout: Duration) -> Option<Event> {
        let (lock, cv) = &*self.mpv.inner;
        let mut s = lock.lock().unwrap_or_else(|p| p.into_inner());
        if s.events.is_empty() && !s.woken && !timeout.is_zero() {
            s = cv
                .wait_timeout_while(s, timeout, |s| s.events.is_empty() && !s.woken)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
        s.woken = false;
        s.events.pop_front()
    }

    fn waker(&self) -> Waker {
        let inner = self.mpv.inner.clone();
        Arc::new(move || {
            inner.0.lock().unwrap_or_else(|p| p.into_inner()).woken = true;
            inner.1.notify_all();
        })
    }

    fn renders_video(&self) -> bool {
        true
    }

    fn describe(&self) -> (String, Option<String>) {
        ("fake".into(), None)
    }
}
