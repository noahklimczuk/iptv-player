//! Web view processes left behind by an Aurora that is no longer running.
//!
//! "The install and restart button doesn't relaunch the app." It did relaunch it: the
//! new process started, then waited a minute before it could log a line, and then its
//! web view failed to start ("failed to receive message from webview"). The window was
//! shown anyway, and because the window is transparent (the video is drawn behind it)
//! a window with no page in it is invisible. A running app nobody can see looks
//! exactly like one that never came back.
//!
//! The cause was the previous Aurora's WebView2 processes. `restart` starts the new
//! process and then exits, and a WebView2 browser process does not always go with its
//! host: one was measured still alive more than a minute after its Aurora had gone,
//! holding the user-data folder. The next web view to open that folder has to get past
//! it, which took 58 s in the measurement and on the viewer's machine did not work at
//! all. The same happens on an ordinary launch after a crash or a forced close.
//!
//! So before the window is built, ask Windows which processes hold the folder's
//! `lockfile` (Restart Manager knows exactly), and end the ones whose Aurora is gone.
//! Nothing else is touched: not another app's web views, which never hold this folder,
//! and not those of a second Aurora that is still running.

/// What the reaper knows about one process holding the folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    pub pid: u32,
    pub exe: String,
    pub parent: u32,
}

/// Whether `holder` is a web view whose Aurora is gone, given every running process as
/// `(pid, exe name)`.
///
/// Its parent must be a running process named like this one (`our_exe`) for it to be
/// kept. A parent pid that is gone, or that has since been reused by something else,
/// means the Aurora that started it has exited.
pub fn is_orphan(holder: &Holder, running: &[(u32, String)], our_exe: &str) -> bool {
    if !holder.exe.eq_ignore_ascii_case("msedgewebview2.exe") {
        return false;
    }
    !running
        .iter()
        .any(|(pid, exe)| *pid == holder.parent && exe.eq_ignore_ascii_case(our_exe))
}

/// The folder holding this app's WebView2 `lockfile`.
///
/// WebView2 is given a user-data folder -- Tauri's app data folder, unless the
/// environment names another -- and keeps its profile in an `EBWebView` folder *inside*
/// it. The lock is there, one level down from the folder named.
pub fn user_data_dir() -> Option<std::path::PathBuf> {
    let given = match std::env::var_os("WEBVIEW2_USER_DATA_FOLDER") {
        Some(dir) => std::path::PathBuf::from(dir),
        None => {
            std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("tv.aurora.player")
        }
    };
    Some(given.join("EBWebView"))
}

/// End the orphaned web views holding `dir`, and wait briefly for them to go.
/// Returns how many were ended. Best effort: any failure leaves things as they were.
///
/// The second value says what was found, for the log: this runs before the logger
/// exists, and "it ended nothing" is undiagnosable without knowing why.
#[cfg(windows)]
pub fn reap(dir: &std::path::Path) -> (usize, String) {
    let lock = dir.join("lockfile");
    if !lock.exists() {
        return (0, format!("no lock at {}", lock.display()));
    }
    let our_exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "aurora-app.exe".into());

    // A restart starts this process *before* the one it replaces has gone, so at this
    // moment that one's web view still has a living parent and is not an orphan yet.
    // Give the Aurora that launched us a moment to finish exiting first; otherwise its
    // web view is left alone now and orphaned a second later, which is the failure.
    let me = std::process::id();
    let launcher = processes_with_parents()
        .into_iter()
        .find(|(pid, _, _)| *pid == me)
        .map(|(_, _, parent)| parent);
    let started = std::time::Instant::now();
    let mut holders = holders_of(&lock);
    while let Some(launcher) = launcher {
        let waiting = holders.iter().any(|h| h.parent == launcher)
            && processes()
                .iter()
                .any(|(pid, exe)| *pid == launcher && exe.eq_ignore_ascii_case(&our_exe));
        if !waiting || started.elapsed() > std::time::Duration::from_secs(10) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
        holders = holders_of(&lock);
    }
    if holders.is_empty() {
        return (0, "lock not held".into());
    }
    let running = processes();
    let mut ended = 0;
    let mut notes = Vec::new();
    for h in &holders {
        let orphan = is_orphan(h, &running, &our_exe);
        let done = orphan && terminate(h.pid);
        if done {
            ended += 1;
        }
        notes.push(format!(
            "{} {} (parent {}): {}",
            h.exe,
            h.pid,
            h.parent,
            if done {
                "ended"
            } else if orphan {
                "orphaned, could not end"
            } else {
                "its Aurora is running"
            }
        ));
    }
    (ended, notes.join("; "))
}

#[cfg(not(windows))]
pub fn reap(_dir: &std::path::Path) -> (usize, String) {
    (0, "not on Windows".into())
}

/// Wait, up to `max`, for the Aurora that started this process to exit.
///
/// The update chain is: the running app starts a short-lived copy and exits; that copy
/// swaps the files and starts the real one. The running app can take a while to close
/// -- it is finalising recordings and closing a large library -- and its web view goes
/// with it only once it has. Starting the real copy before then is what left a window
/// with no page in it. Returns whether the launcher is gone (or was never an Aurora).
#[cfg(windows)]
pub fn wait_for_launcher(max: std::time::Duration) -> bool {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
    };
    let our_exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "aurora-app.exe".into());
    let all = processes_with_parents();
    let me = std::process::id();
    let Some(parent) = all
        .iter()
        .find(|(pid, _, _)| *pid == me)
        .map(|(_, _, p)| *p)
    else {
        return true;
    };
    if !all
        .iter()
        .any(|(pid, exe, _)| *pid == parent && exe.eq_ignore_ascii_case(&our_exe))
    {
        return true;
    }
    let Ok(handle) = (unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, parent) }) else {
        return true;
    };
    let gone = unsafe { WaitForSingleObject(handle, max.as_millis() as u32) } == WAIT_OBJECT_0;
    unsafe {
        let _ = CloseHandle(handle);
    }
    gone
}

#[cfg(not(windows))]
pub fn wait_for_launcher(_max: std::time::Duration) -> bool {
    true
}

/// Every process holding `file`, according to Restart Manager.
#[cfg(windows)]
fn holders_of(file: &std::path::Path) -> Vec<Holder> {
    use windows::core::{HSTRING, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows::Win32::System::RestartManager::{
        RmEndSession, RmGetList, RmRegisterResources, RmStartSession, CCH_RM_SESSION_KEY,
        RM_PROCESS_INFO,
    };

    let mut session = 0u32;
    let mut key = [0u16; CCH_RM_SESSION_KEY as usize + 1];
    if unsafe { RmStartSession(&mut session, 0, PWSTR(key.as_mut_ptr())) } != ERROR_SUCCESS {
        return Vec::new();
    }
    let path = HSTRING::from(file.as_os_str());
    let mut out = Vec::new();
    'session: {
        let files = [PCWSTR(path.as_ptr())];
        if unsafe { RmRegisterResources(session, Some(&files), None, None) } != ERROR_SUCCESS {
            break 'session;
        }
        let (mut needed, mut count, mut reasons) = (0u32, 0u32, 0u32);
        let first = unsafe { RmGetList(session, &mut needed, &mut count, None, &mut reasons) };
        if first == ERROR_SUCCESS || first != ERROR_MORE_DATA || needed == 0 {
            break 'session;
        }
        let mut infos = vec![RM_PROCESS_INFO::default(); needed as usize];
        count = needed;
        let second = unsafe {
            RmGetList(
                session,
                &mut needed,
                &mut count,
                Some(infos.as_mut_ptr()),
                &mut reasons,
            )
        };
        if second != ERROR_SUCCESS {
            break 'session;
        }
        let running = processes_with_parents();
        for info in &infos[..count as usize] {
            let pid = info.Process.dwProcessId;
            if let Some((_, exe, parent)) = running.iter().find(|(p, _, _)| *p == pid) {
                out.push(Holder {
                    pid,
                    exe: exe.clone(),
                    parent: *parent,
                });
            }
        }
    }
    unsafe {
        let _ = RmEndSession(session);
    }
    out
}

/// Every running process as `(pid, exe name, parent pid)`.
#[cfg(windows)]
fn processes_with_parents() -> Vec<(u32, String, u32)> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let mut out = Vec::new();
    let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return out;
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut ok = unsafe { Process32FirstW(snap, &mut entry) }.is_ok();
    while ok {
        let len = entry
            .szExeFile
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(entry.szExeFile.len());
        let exe = String::from_utf16_lossy(&entry.szExeFile[..len]);
        out.push((entry.th32ProcessID, exe, entry.th32ParentProcessID));
        ok = unsafe { Process32NextW(snap, &mut entry) }.is_ok();
    }
    unsafe {
        let _ = CloseHandle(snap);
    }
    out
}

#[cfg(windows)]
fn processes() -> Vec<(u32, String)> {
    processes_with_parents()
        .into_iter()
        .map(|(pid, exe, _)| (pid, exe))
        .collect()
}

/// End one process and give it up to two seconds to be gone.
#[cfg(windows)]
fn terminate(pid: u32) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    };
    let Ok(handle) = (unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, pid) })
    else {
        return false;
    };
    let ended = unsafe { TerminateProcess(handle, 1) }.is_ok();
    if ended {
        unsafe {
            WaitForSingleObject(handle, 2_000);
        }
    }
    unsafe {
        let _ = CloseHandle(handle);
    }
    ended
}

#[cfg(test)]
mod tests {
    use super::*;

    fn webview(parent: u32) -> Holder {
        Holder {
            pid: 500,
            exe: "msedgewebview2.exe".into(),
            parent,
        }
    }

    #[test]
    fn a_web_view_whose_aurora_has_gone_is_an_orphan() {
        let running = vec![
            (4, "System".to_string()),
            (500, "msedgewebview2.exe".into()),
        ];
        assert!(is_orphan(&webview(49852), &running, "aurora-app.exe"));
    }

    #[test]
    fn a_web_view_of_a_running_aurora_is_left_alone() {
        let running = vec![(26600, "aurora-app.exe".to_string())];
        assert!(!is_orphan(&webview(26600), &running, "aurora-app.exe"));
        assert!(
            !is_orphan(&webview(26600), &running, "AURORA-APP.EXE"),
            "exe names compare without case, as Windows does"
        );
    }

    #[test]
    fn a_parent_pid_reused_by_something_else_still_means_aurora_is_gone() {
        let running = vec![(49852, "notepad.exe".to_string())];
        assert!(is_orphan(&webview(49852), &running, "aurora-app.exe"));
    }

    #[test]
    fn nothing_but_a_web_view_is_ever_ended() {
        let holder = Holder {
            pid: 7,
            exe: "explorer.exe".into(),
            parent: 1,
        };
        assert!(!is_orphan(&holder, &[], "aurora-app.exe"));
    }
}
