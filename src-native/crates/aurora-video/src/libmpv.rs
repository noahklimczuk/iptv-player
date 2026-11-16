//! The real driver: libmpv through its C API.
//!
//! Direct rather than through a wrapper, because the engine needs exactly the parts
//! wrappers leave out: `mpv_wakeup` (so a queued command interrupts the wait at once),
//! the playlist entry ids on file events (so a stale event can be told from a current
//! one), and the log stream (so a failure can say "403" rather than "loading failed").
//!
//! The binary links libmpv with `/DELAYLOAD`, so the DLL is only looked for on the first
//! call into it. [`LibMpv::open`] asks for it by name first: calling a delay-loaded
//! symbol whose DLL is missing is a crash, not an error.

use std::ffi::{c_char, c_void, CStr, CString};
use std::ptr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use libmpv2_sys as sys;

use crate::driver::{Driver, EndReason, Event, Node, Waker};

pub const DLL: &str = "libmpv-2.dll";

/// How an instance is set up. The main player draws into a window; a preview instance
/// (thumbnails) draws nowhere.
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// The child window to draw into.
    pub wid: Option<isize>,
    /// No video output at all: decoding only, for frame grabs.
    pub headless: bool,
}

/// Whether the DLL can be loaded, asked before any mpv symbol is touched.
pub fn loadable() -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::System::LibraryLoader::LoadLibraryW;
    let wide: Vec<u16> = DLL.encode_utf16().chain(std::iter::once(0)).collect();
    // Safety: a null-terminated wide string that outlives the call. The handle is kept:
    // the process is about to use the library for the rest of its life.
    unsafe { LoadLibraryW(PCWSTR(wide.as_ptr())).is_ok() }
}

fn err(code: i32) -> String {
    // Safety: mpv_error_string returns a static string for any code.
    unsafe { CStr::from_ptr(sys::mpv_error_string(code)) }
        .to_string_lossy()
        .into_owned()
}

fn cstr(s: &str) -> Result<CString, String> {
    CString::new(s).map_err(|_| format!("{s:?} contains a NUL"))
}

/// The handle, shared with the waker so it can tell a live handle from a destroyed one.
type Handle = Arc<Mutex<usize>>;

pub struct LibMpv {
    h: *mut sys::mpv_handle,
    shared: Handle,
}

// Safety: mpv handles may be used from any thread. This one is used from the engine
// thread only, apart from `mpv_wakeup`, which mpv documents as thread-safe and which the
// waker only calls while the handle is alive (see `Drop`).
unsafe impl Send for LibMpv {}

impl LibMpv {
    pub fn open(cfg: &Config) -> Result<Self, String> {
        if !loadable() {
            return Err(format!("{DLL} could not be loaded"));
        }
        // Safety: plain C calls on a handle we own until `Drop`.
        unsafe {
            let h = sys::mpv_create();
            if h.is_null() {
                return Err("mpv could not be created".into());
            }
            let me = Self {
                h,
                shared: Arc::new(Mutex::new(h as usize)),
            };
            let mut options: Vec<(&str, String)> = vec![
                // Nothing of the viewer's own mpv set-up applies inside Aurora.
                ("config", "no".into()),
                ("load-scripts", "no".into()),
                ("ytdl", "no".into()),
                ("terminal", "no".into()),
                // Aurora draws every control; mpv draws none and takes no input.
                ("osc", "no".into()),
                ("osd-level", "0".into()),
                ("input-default-bindings", "no".into()),
                ("input-vo-keyboard", "no".into()),
                ("input-cursor", "no".into()),
                ("cursor-autohide", "no".into()),
                // Stay up between files, and hold the last frame at the end of a film.
                ("idle", "yes".into()),
                ("keep-open", "yes".into()),
                ("force-window", "no".into()),
                ("volume-max", "200".into()),
                ("network-timeout", "12".into()),
                // A dropped HTTP connection is reopened by FFmpeg before mpv ever sees
                // an error; most "the channel blipped" moments end here.
                (
                    "stream-lavf-o",
                    "reconnect=1,reconnect_streamed=1,reconnect_delay_max=4".into(),
                ),
                ("audio-client-name", "Aurora TV".into()),
                ("hwdec", "auto-safe".into()),
            ];
            if cfg.headless {
                options.push(("vo", "null".into()));
                options.push(("ao", "null".into()));
                options.push(("hwdec", "no".into()));
            } else {
                options.push(("vo", "gpu-next".into()));
                options.push(("gpu-api", "d3d11".into()));
            }
            if let Some(wid) = cfg.wid {
                options.push(("wid", wid.to_string()));
            }
            for (name, value) in &options {
                let (n, v) = (cstr(name)?, cstr(value)?);
                let rc = sys::mpv_set_option_string(h, n.as_ptr(), v.as_ptr());
                if rc < 0 {
                    tracing::warn!(option = name, value, "mpv refused an option: {}", err(rc));
                }
            }
            let rc = sys::mpv_initialize(h);
            if rc < 0 {
                return Err(format!("mpv would not start: {}", err(rc)));
            }
            let level = cstr("warn")?;
            sys::mpv_request_log_messages(h, level.as_ptr());
            Ok(me)
        }
    }

    pub fn version() -> Option<String> {
        // Safety: a static query, valid once the DLL is loaded.
        let v = unsafe { sys::mpv_client_api_version() } as u64;
        Some(format!("client API {}.{}", v >> 16, v & 0xffff))
    }
}

impl Drop for LibMpv {
    fn drop(&mut self) {
        // Close the door on the waker first: after this it will not touch the handle.
        *self.shared.lock().unwrap_or_else(|p| p.into_inner()) = 0;
        // Safety: we own the handle, and nothing else will use it again.
        unsafe { sys::mpv_terminate_destroy(self.h) };
    }
}

/// Read an mpv node into an owned [`Node`].
///
/// # Safety
/// `n` must point at a node mpv filled in, still alive.
unsafe fn read(n: *const sys::mpv_node) -> Node {
    let n = &*n;
    match n.format {
        sys::mpv_format_MPV_FORMAT_STRING | sys::mpv_format_MPV_FORMAT_OSD_STRING => {
            if n.u.string.is_null() {
                Node::None
            } else {
                Node::Str(CStr::from_ptr(n.u.string).to_string_lossy().into_owned())
            }
        }
        sys::mpv_format_MPV_FORMAT_FLAG => Node::Flag(n.u.flag != 0),
        sys::mpv_format_MPV_FORMAT_INT64 => Node::Int(n.u.int64),
        sys::mpv_format_MPV_FORMAT_DOUBLE => Node::Double(n.u.double_),
        sys::mpv_format_MPV_FORMAT_NODE_ARRAY | sys::mpv_format_MPV_FORMAT_NODE_MAP => {
            let list = n.u.list;
            if list.is_null() {
                return Node::None;
            }
            let list = &*list;
            let count = usize::try_from(list.num).unwrap_or(0);
            let map = n.format == sys::mpv_format_MPV_FORMAT_NODE_MAP;
            let mut items = Vec::with_capacity(count);
            let mut entries = Vec::with_capacity(if map { count } else { 0 });
            for i in 0..count {
                let value = read(list.values.add(i));
                if map {
                    let key = *list.keys.add(i);
                    let key = if key.is_null() {
                        String::new()
                    } else {
                        CStr::from_ptr(key).to_string_lossy().into_owned()
                    };
                    entries.push((key, value));
                } else {
                    items.push(value);
                }
            }
            if map {
                Node::Map(entries)
            } else {
                Node::Array(items)
            }
        }
        sys::mpv_format_MPV_FORMAT_BYTE_ARRAY => {
            let ba = n.u.ba;
            if ba.is_null() || (*ba).data.is_null() {
                return Node::Bytes(Vec::new());
            }
            Node::Bytes(std::slice::from_raw_parts((*ba).data as *const u8, (*ba).size).to_vec())
        }
        _ => Node::None,
    }
}

impl Driver for LibMpv {
    fn set(&mut self, name: &str, value: &str) -> Result<(), String> {
        let (n, v) = (cstr(name)?, cstr(value)?);
        // Safety: live handle, strings outlive the call.
        let rc = unsafe { sys::mpv_set_property_string(self.h, n.as_ptr(), v.as_ptr()) };
        if rc < 0 {
            Err(err(rc))
        } else {
            Ok(())
        }
    }

    fn set_list(&mut self, name: &str, items: &[String]) -> Result<(), String> {
        let n = cstr(name)?;
        let strings = items
            .iter()
            .map(|s| cstr(s))
            .collect::<Result<Vec<_>, _>>()?;
        let mut values: Vec<sys::mpv_node> = strings
            .iter()
            .map(|s| sys::mpv_node {
                u: sys::mpv_node__bindgen_ty_1 {
                    string: s.as_ptr() as *mut c_char,
                },
                format: sys::mpv_format_MPV_FORMAT_STRING,
            })
            .collect();
        let mut list = sys::mpv_node_list {
            num: values.len() as i32,
            values: if values.is_empty() {
                ptr::null_mut()
            } else {
                values.as_mut_ptr()
            },
            keys: ptr::null_mut(),
        };
        let mut node = sys::mpv_node {
            u: sys::mpv_node__bindgen_ty_1 { list: &mut list },
            format: sys::mpv_format_MPV_FORMAT_NODE_ARRAY,
        };
        // Safety: every pointer in `node` refers to locals that outlive the call; mpv
        // copies what it keeps.
        let rc = unsafe {
            sys::mpv_set_property(
                self.h,
                n.as_ptr(),
                sys::mpv_format_MPV_FORMAT_NODE,
                &mut node as *mut _ as *mut c_void,
            )
        };
        if rc < 0 {
            Err(err(rc))
        } else {
            Ok(())
        }
    }

    fn command(&mut self, args: &[&str]) -> Result<Node, String> {
        let owned = args
            .iter()
            .map(|a| cstr(a))
            .collect::<Result<Vec<_>, _>>()?;
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|a| a.as_ptr()).collect();
        ptrs.push(ptr::null());
        // Safety: zeroed is a valid empty node (format NONE).
        let mut result: sys::mpv_node = unsafe { std::mem::zeroed() };
        // Safety: a null-terminated argument array of strings that outlive the call.
        let rc = unsafe { sys::mpv_command_ret(self.h, ptrs.as_mut_ptr(), &mut result) };
        if rc < 0 {
            return Err(err(rc));
        }
        // Safety: mpv filled `result`; it is read, then freed exactly once.
        unsafe {
            let out = read(&result);
            sys::mpv_free_node_contents(&mut result);
            Ok(out)
        }
    }

    fn get(&mut self, name: &str) -> Option<Node> {
        let n = cstr(name).ok()?;
        // Safety: as in `command`.
        unsafe {
            let mut node: sys::mpv_node = std::mem::zeroed();
            let rc = sys::mpv_get_property(
                self.h,
                n.as_ptr(),
                sys::mpv_format_MPV_FORMAT_NODE,
                &mut node as *mut _ as *mut c_void,
            );
            if rc < 0 {
                return None;
            }
            let out = read(&node);
            sys::mpv_free_node_contents(&mut node);
            Some(out)
        }
    }

    fn observe(&mut self, name: &str) -> Result<(), String> {
        let n = cstr(name)?;
        // Safety: live handle; mpv copies the name.
        let rc = unsafe {
            sys::mpv_observe_property(self.h, 0, n.as_ptr(), sys::mpv_format_MPV_FORMAT_NODE)
        };
        if rc < 0 {
            Err(err(rc))
        } else {
            Ok(())
        }
    }

    fn wait(&mut self, timeout: Duration) -> Option<Event> {
        let mut timeout = timeout.as_secs_f64();
        loop {
            // Safety: live handle. The event and everything it points at stay valid
            // until the next `mpv_wait_event` on this handle, and are copied out first.
            unsafe {
                let ev = &*sys::mpv_wait_event(self.h, timeout);
                // Anything after the first event is fetched without waiting.
                timeout = 0.0;
                match ev.event_id {
                    sys::mpv_event_id_MPV_EVENT_NONE => return None,
                    sys::mpv_event_id_MPV_EVENT_SHUTDOWN => return Some(Event::Shutdown),
                    sys::mpv_event_id_MPV_EVENT_LOG_MESSAGE => {
                        let m = &*(ev.data as *const sys::mpv_event_log_message);
                        let text = |p: *const c_char| {
                            if p.is_null() {
                                String::new()
                            } else {
                                CStr::from_ptr(p).to_string_lossy().into_owned()
                            }
                        };
                        return Some(Event::Log {
                            prefix: text(m.prefix),
                            text: text(m.text),
                        });
                    }
                    sys::mpv_event_id_MPV_EVENT_START_FILE => {
                        let s = &*(ev.data as *const sys::mpv_event_start_file);
                        return Some(Event::StartFile {
                            entry: s.playlist_entry_id,
                        });
                    }
                    sys::mpv_event_id_MPV_EVENT_END_FILE => {
                        let e = &*(ev.data as *const sys::mpv_event_end_file);
                        let reason = match e.reason {
                            sys::mpv_end_file_reason_MPV_END_FILE_REASON_EOF => EndReason::Eof,
                            sys::mpv_end_file_reason_MPV_END_FILE_REASON_ERROR => EndReason::Error,
                            sys::mpv_end_file_reason_MPV_END_FILE_REASON_REDIRECT => {
                                EndReason::Redirect
                            }
                            sys::mpv_end_file_reason_MPV_END_FILE_REASON_QUIT => EndReason::Quit,
                            _ => EndReason::Stop,
                        };
                        return Some(Event::EndFile {
                            entry: e.playlist_entry_id,
                            reason,
                            error: (reason == EndReason::Error).then(|| err(e.error)),
                            next: (e.playlist_insert_num_entries > 0)
                                .then_some(e.playlist_insert_id),
                        });
                    }
                    sys::mpv_event_id_MPV_EVENT_FILE_LOADED => return Some(Event::FileLoaded),
                    sys::mpv_event_id_MPV_EVENT_PLAYBACK_RESTART => {
                        return Some(Event::PlaybackRestart)
                    }
                    sys::mpv_event_id_MPV_EVENT_PROPERTY_CHANGE => {
                        let p = &*(ev.data as *const sys::mpv_event_property);
                        if p.name.is_null() {
                            continue;
                        }
                        let name = CStr::from_ptr(p.name).to_string_lossy().into_owned();
                        let value =
                            if p.format == sys::mpv_format_MPV_FORMAT_NODE && !p.data.is_null() {
                                read(p.data as *const sys::mpv_node)
                            } else {
                                Node::None
                            };
                        return Some(Event::Property { name, value });
                    }
                    // Nothing the engine uses; look at the next one.
                    _ => continue,
                }
            }
        }
    }

    fn waker(&self) -> Waker {
        let shared = self.shared.clone();
        Arc::new(move || {
            let h = shared.lock().unwrap_or_else(|p| p.into_inner());
            if *h != 0 {
                // Safety: the handle is alive while this lock shows it, and `Drop`
                // clears it under the same lock before destroying it.
                unsafe { sys::mpv_wakeup(*h as *mut sys::mpv_handle) };
            }
        })
    }

    fn renders_video(&self) -> bool {
        true
    }

    fn describe(&self) -> (String, Option<String>) {
        ("libmpv".into(), Self::version())
    }
}
