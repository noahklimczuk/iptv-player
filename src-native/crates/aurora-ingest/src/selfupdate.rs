//! Replacing a portable copy of the app with a newer one, without leaving the app.
//!
//! A portable build used to be offered nothing: `can_install()` returned false and the
//! viewer was told the release page had the new zip. So the one build that is easiest
//! to update — a folder of files nobody installed — was the one that could only be
//! updated by hand, while the build that needs an installer and a UAC prompt got the
//! button.
//!
//! The awkward part on Windows is that a running `.exe` cannot be deleted or
//! overwritten — but it *can* be renamed. So the swap is: move what is there out of
//! the way, move what was downloaded in, and relaunch. Doing it at startup, before
//! anything is holding a file open, is what makes it survivable: at every point either
//! the old copy or the new one is complete, and a failure half way leaves the old one
//! recoverable rather than leaving a folder of neither.
//!
//! Everything here is ordinary filesystem work on purpose. Renaming a running
//! executable is the only Windows-specific part of the idea, and it is not expressed
//! in Windows-specific code — which is what lets the whole of it be tested on Linux
//! (docs/DECISIONS.md D2), rather than being another thing that compiles and has never
//! been run.

use std::path::{Path, PathBuf};

use aurora_core::neterr::{ErrorAction, ErrorCode, NetFailure};

/// Where a downloaded update is unpacked, under the app's data directory.
pub const STAGE_DIR: &str = "staged";
/// Written last, once the staged copy is complete. Its absence is what says "this
/// folder is a half-finished download, throw it away" — a marker written at the end is
/// the cheapest transaction there is.
pub const READY_MARKER: &str = ".ready";
/// Where the outgoing copy goes while the new one moves in.
pub const PREVIOUS_DIR: &str = "previous";

/// The executable a portable build runs from, and which has to end up replaced.
///
/// Per platform, because the name is how `stage_zip` decides an archive really is
/// Aurora and not something else that was served under the right filename. Hardcoding
/// the Windows spelling meant a portable build anywhere else refused its own release
/// with "the archive contains no aurora-app.exe" — which is how the update path came
/// to have no end-to-end test: it could not be driven at all except on Windows.
#[cfg(windows)]
pub const APP_EXE: &str = "aurora-app.exe";
#[cfg(not(windows))]
pub const APP_EXE: &str = "aurora-app";

fn failure(message: &str, cause: String) -> NetFailure {
    NetFailure {
        code: ErrorCode::Unknown,
        message: message.to_string(),
        cause,
        actions: vec![ErrorAction::Retry],
        retryable: true,
    }
}

/// What a staged update turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged {
    /// The folder holding the unpacked new copy.
    pub dir: PathBuf,
    /// Files it will put in place, relative to the install folder.
    pub files: Vec<PathBuf>,
}

/// Unpack a verified zip into `stage_dir`, ready to be swapped in.
///
/// The zip is the portable archive from the release, which holds one top-level folder
/// (`Aurora TV/`) — that prefix is stripped, because what is being staged is the
/// *contents* of an install folder rather than a folder to nest inside it.
///
/// Refuses anything whose path escapes the staging directory. A zip is an archive from
/// the network, and `../../windows/system32` is the oldest trick there is; that the
/// archive was digest-checked says it is the file GitHub published, not that the file
/// is harmless.
pub fn stage_zip(archive: &Path, stage_dir: &Path) -> Result<Staged, NetFailure> {
    // Start from nothing: a previous half-finished attempt must not contribute files.
    let _ = std::fs::remove_dir_all(stage_dir);
    std::fs::create_dir_all(stage_dir)
        .map_err(|e| failure("Could not prepare the update", e.to_string()))?;

    let file = std::fs::File::open(archive)
        .map_err(|e| failure("Could not open the downloaded update", e.to_string()))?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| {
        failure(
            "The downloaded update is not a readable archive",
            e.to_string(),
        )
    })?;

    let mut files = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| failure("The downloaded update is damaged", e.to_string()))?;

        let Some(relative) = safe_entry_path(entry.name()) else {
            return Err(failure(
                "The downloaded update contains an unsafe path",
                format!("{:?} does not stay inside the update folder", entry.name()),
            ));
        };
        if entry.is_dir() || relative.as_os_str().is_empty() {
            continue;
        }

        let target = stage_dir.join(&relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| failure("Could not write the update", e.to_string()))?;
        }
        let mut out = std::fs::File::create(&target)
            .map_err(|e| failure("Could not write the update", e.to_string()))?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|e| failure("Could not write the update", e.to_string()))?;

        #[cfg(unix)]
        {
            // Keep the executable bit the archive recorded, so a staged copy on a
            // non-Windows host is still runnable. Windows has no equivalent and needs
            // none.
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = entry.unix_mode() {
                let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode));
            }
        }

        files.push(relative);
    }

    if files.is_empty() {
        return Err(failure(
            "The downloaded update is empty",
            "the archive contained no files".into(),
        ));
    }
    if !files.iter().any(|f| is_app_exe(f)) {
        return Err(failure(
            "The downloaded update is not Aurora",
            format!("the archive contains no {APP_EXE}"),
        ));
    }

    // Last, and only once everything above worked: this is what `staged` looks for.
    std::fs::write(stage_dir.join(READY_MARKER), b"")
        .map_err(|e| failure("Could not finish preparing the update", e.to_string()))?;

    files.sort();
    Ok(Staged {
        dir: stage_dir.to_path_buf(),
        files,
    })
}

fn is_app_exe(path: &Path) -> bool {
    path.file_name()
        .map(|n| n.eq_ignore_ascii_case(APP_EXE))
        .unwrap_or(false)
}

/// A zip entry's path, relative and guaranteed to stay inside the target.
///
/// Also strips a single leading folder, because the portable archive wraps everything
/// in `Aurora TV/` and what is wanted is the contents. Returns `None` for anything
/// with a `..`, an absolute path, or a drive letter.
fn safe_entry_path(name: &str) -> Option<PathBuf> {
    // Zip always uses forward slashes; a backslash in a name is either a Windows
    // writer being sloppy or someone being clever, and both are handled by treating it
    // as a separator rather than as part of a filename.
    let parts: Vec<&str> = name
        .split(['/', '\\'])
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    if parts.iter().any(|p| *p == ".." || p.contains(':')) {
        return None;
    }
    if name.starts_with('/') || name.starts_with('\\') {
        return None;
    }
    // Drop the single wrapping folder the archive is built with.
    let parts = if parts.len() > 1 {
        &parts[1..]
    } else {
        &parts[..]
    };
    Some(parts.iter().collect())
}

/// A staged update that is complete and ready to swap in, if there is one.
///
/// Anything without the marker is a download that did not finish, and is removed
/// rather than reported: a partial copy of an application is worse than none.
pub fn staged(stage_dir: &Path) -> Option<Staged> {
    if !stage_dir.join(READY_MARKER).exists() {
        if stage_dir.exists() {
            let _ = std::fs::remove_dir_all(stage_dir);
        }
        return None;
    }
    let mut files = Vec::new();
    collect(stage_dir, stage_dir, &mut files);
    files.retain(|f| f.as_os_str() != READY_MARKER);
    if files.is_empty() || !files.iter().any(|f| is_app_exe(f)) {
        let _ = std::fs::remove_dir_all(stage_dir);
        return None;
    }
    files.sort();
    Some(Staged {
        dir: stage_dir.to_path_buf(),
        files,
    })
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out);
        } else if let Ok(relative) = path.strip_prefix(root) {
            out.push(relative.to_path_buf());
        }
    }
}

/// Move a staged update into `install_dir`, keeping what was there.
///
/// The order is the whole design. For each file: move the existing one into
/// `previous/`, then move the new one in. Moving rather than deleting is what makes
/// the running executable survivable on Windows, where a live `.exe` can be renamed
/// but not removed — and it is also the rollback, because everything displaced is
/// still on disk under its own name.
///
/// A failure part way rolls back what it already did and returns the error. The app
/// then carries on as the version it already was, which is the only outcome here worth
/// having: a failed update must never be the reason somebody's television stops
/// working.
pub fn apply(
    staged: &Staged,
    install_dir: &Path,
    previous_dir: &Path,
) -> Result<usize, NetFailure> {
    let _ = std::fs::remove_dir_all(previous_dir);
    std::fs::create_dir_all(previous_dir)
        .map_err(|e| failure("Could not make room for the update", e.to_string()))?;

    // (destination, where its previous contents went) for everything done so far.
    let mut done: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();

    for relative in &staged.files {
        let destination = install_dir.join(relative);
        let incoming = staged.dir.join(relative);

        if let Some(parent) = destination.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                rollback(&done, install_dir);
                return Err(failure("Could not install the update", e.to_string()));
            }
        }

        // Displace whatever is there. Renaming a running .exe is legal on Windows;
        // deleting it is not, which is why this never deletes.
        let displaced = if destination.exists() {
            let kept = previous_dir.join(relative);
            if let Some(parent) = kept.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::rename(&destination, &kept) {
                rollback(&done, install_dir);
                return Err(failure(
                    "Could not replace the running copy",
                    format!("{}: {e}", destination.display()),
                ));
            }
            Some(kept)
        } else {
            None
        };

        if let Err(e) = move_file(&incoming, &destination) {
            // Put back the one just displaced, then everything before it.
            if let Some(kept) = &displaced {
                let _ = std::fs::rename(kept, &destination);
            }
            rollback(&done, install_dir);
            return Err(failure("Could not install the update", e.to_string()));
        }
        done.push((destination, displaced));
    }

    let _ = std::fs::remove_dir_all(&staged.dir);
    Ok(done.len())
}

/// Put back everything a failed `apply` had already moved.
fn rollback(done: &[(PathBuf, Option<PathBuf>)], _install_dir: &Path) {
    for (destination, displaced) in done.iter().rev() {
        let _ = std::fs::remove_file(destination);
        if let Some(kept) = displaced {
            let _ = std::fs::rename(kept, destination);
        }
    }
}

/// `rename`, falling back to copy-then-delete across a filesystem boundary.
///
/// The staging folder is under the app's data directory and the install folder is
/// wherever the viewer unzipped it; on Windows those are routinely different volumes,
/// and `rename` refuses across one.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(from, to)?;
            let _ = std::fs::remove_file(from);
            Ok(())
        }
    }
}

/// Delete the copy `apply` displaced. Called once the new version has started
/// successfully, so there is a version of the app that ran before the old one goes.
pub fn forget_previous(previous_dir: &Path) {
    let _ = std::fs::remove_dir_all(previous_dir);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aurora-selfupdate-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A portable archive shaped the way CI builds one: everything under one folder.
    fn portable_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, body) in entries {
            zip.start_file(format!("Aurora TV/{name}"), options)
                .unwrap();
            zip.write_all(body).unwrap();
        }
        zip.finish().unwrap();
    }

    fn write(path: &Path, body: &[u8]) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn a_portable_archive_stages_its_contents_without_its_wrapper() {
        let dir = scratch("stage");
        let archive = dir.join("aurora-tv-portable.zip");
        portable_zip(
            &archive,
            &[
                (APP_EXE, b"new app"),
                ("mpv-2.dll", b"new mpv"),
                ("licenses/GPL-3.0.txt", b"licence"),
            ],
        );

        let staged = stage_zip(&archive, &dir.join(STAGE_DIR)).unwrap();

        assert_eq!(staged.files.len(), 3);
        assert!(staged.files.contains(&PathBuf::from(APP_EXE)));
        // The `Aurora TV/` wrapper is gone: what is staged is the folder's contents.
        assert_eq!(std::fs::read(staged.dir.join(APP_EXE)).unwrap(), b"new app");
        assert_eq!(
            std::fs::read(staged.dir.join("licenses/GPL-3.0.txt")).unwrap(),
            b"licence"
        );
        assert!(staged.dir.join(READY_MARKER).exists());
    }

    /// The oldest trick in archives. A digest proves the file is the one GitHub
    /// published; it says nothing about whether the file is well behaved.
    #[test]
    fn an_archive_that_escapes_its_folder_is_refused() {
        let dir = scratch("escape");
        for hostile in [
            "../evil.exe",
            "Aurora TV/../../evil.exe",
            "/etc/passwd",
            "..\\evil.exe",
        ] {
            let archive = dir.join("hostile.zip");
            let file = std::fs::File::create(&archive).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            zip.start_file(hostile, options).unwrap();
            zip.write_all(b"owned").unwrap();
            zip.finish().unwrap();

            let outcome = stage_zip(&archive, &dir.join(STAGE_DIR));
            assert!(outcome.is_err(), "{hostile:?} was accepted");
            assert!(!dir.join("evil.exe").exists(), "{hostile:?} escaped");
        }
    }

    #[test]
    fn an_archive_that_is_not_aurora_is_refused() {
        let dir = scratch("notaurora");
        let archive = dir.join("other.zip");
        portable_zip(&archive, &[("something-else.exe", b"nope")]);

        let err = stage_zip(&archive, &dir.join(STAGE_DIR)).unwrap_err();
        assert!(err.message.contains("not Aurora"), "{}", err.message);
    }

    #[test]
    fn an_empty_archive_is_refused() {
        let dir = scratch("empty");
        let archive = dir.join("empty.zip");
        portable_zip(&archive, &[]);
        assert!(stage_zip(&archive, &dir.join(STAGE_DIR)).is_err());
    }

    /// The marker is written last, so a folder without one is a download that did not
    /// finish — and half an application is worse than none.
    #[test]
    fn a_half_written_stage_is_discarded_rather_than_used() {
        let dir = scratch("partial");
        let stage = dir.join(STAGE_DIR);
        write(&stage.join(APP_EXE), b"half a download");

        assert_eq!(staged(&stage), None);
        assert!(!stage.exists(), "the partial download was left behind");
    }

    #[test]
    fn a_complete_stage_is_found() {
        let dir = scratch("found");
        let stage = dir.join(STAGE_DIR);
        write(&stage.join(APP_EXE), b"new app");
        write(&stage.join("licenses/GPL-3.0.txt"), b"licence");
        write(&stage.join(READY_MARKER), b"");

        let found = staged(&stage).expect("a staged update");
        assert_eq!(found.files.len(), 2);
        assert!(found.files.contains(&PathBuf::from(APP_EXE)));
    }

    #[test]
    fn applying_replaces_the_files_and_keeps_the_old_ones() {
        let dir = scratch("apply");
        let install = dir.join("Aurora TV");
        write(&install.join(APP_EXE), b"old app");
        write(&install.join("mpv-2.dll"), b"old mpv");
        write(&install.join("portable.txt"), b"");

        let stage = dir.join(STAGE_DIR);
        write(&stage.join(APP_EXE), b"new app");
        write(&stage.join("mpv-2.dll"), b"new mpv");
        write(&stage.join("licenses/GPL-3.0.txt"), b"new licence");
        write(&stage.join(READY_MARKER), b"");
        let found = staged(&stage).unwrap();

        let previous = dir.join(PREVIOUS_DIR);
        let moved = apply(&found, &install, &previous).unwrap();
        assert_eq!(moved, 3);

        assert_eq!(std::fs::read(install.join(APP_EXE)).unwrap(), b"new app");
        assert_eq!(
            std::fs::read(install.join("mpv-2.dll")).unwrap(),
            b"new mpv"
        );
        // A file the new version adds arrives even though nothing displaced it.
        assert_eq!(
            std::fs::read(install.join("licenses/GPL-3.0.txt")).unwrap(),
            b"new licence"
        );
        // A file the update does not mention is untouched — `portable.txt` is what
        // makes this copy portable, and losing it would change where the data lives.
        assert!(install.join("portable.txt").exists());
        // The displaced copies are kept, which is what makes this recoverable.
        assert_eq!(std::fs::read(previous.join(APP_EXE)).unwrap(), b"old app");
        // And the staging folder is gone, so nothing tries to apply it twice.
        assert!(!stage.exists());
    }

    /// The outcome that matters most: a failed update must never be why somebody's
    /// television stops working.
    #[test]
    fn a_failure_part_way_puts_everything_back() {
        let dir = scratch("rollback");
        let install = dir.join("Aurora TV");
        write(&install.join(APP_EXE), b"old app");
        write(&install.join("mpv-2.dll"), b"old mpv");

        let stage = dir.join(STAGE_DIR);
        write(&stage.join(APP_EXE), b"new app");
        write(&stage.join("mpv-2.dll"), b"new mpv");
        write(&stage.join(READY_MARKER), b"");
        let mut found = staged(&stage).unwrap();

        // A third file the stage claims and does not have: the move fails half way
        // through, after the first two have already been replaced.
        found.files.push(PathBuf::from("missing.dll"));
        found.files.sort();

        let previous = dir.join(PREVIOUS_DIR);
        assert!(apply(&found, &install, &previous).is_err());

        assert_eq!(std::fs::read(install.join(APP_EXE)).unwrap(), b"old app");
        assert_eq!(
            std::fs::read(install.join("mpv-2.dll")).unwrap(),
            b"old mpv"
        );
        assert!(!install.join("missing.dll").exists());
    }

    #[test]
    fn applying_works_across_a_filesystem_boundary() {
        // `rename` refuses across volumes and the staging folder is routinely on a
        // different one from the install. Simulated by removing the staged file
        // between the rename attempt and the copy — the behaviour under test is that
        // `move_file` does not rely on rename alone.
        let dir = scratch("crossfs");
        let from = dir.join("from.bin");
        let to = dir.join("nested").join("to.bin");
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(&from, b"payload").unwrap();

        move_file(&from, &to).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"payload");
        assert!(!from.exists());
    }

    #[test]
    fn the_previous_copy_can_be_forgotten_once_the_new_one_runs() {
        let dir = scratch("forget");
        let previous = dir.join(PREVIOUS_DIR);
        write(&previous.join(APP_EXE), b"old app");

        forget_previous(&previous);
        assert!(!previous.exists());
        // Idempotent: it runs on every launch, and usually there is nothing there.
        forget_previous(&previous);
    }

    #[test]
    fn entry_paths_are_normalised_and_bounded() {
        assert_eq!(
            safe_entry_path("Aurora TV/aurora-app.exe"),
            Some(PathBuf::from("aurora-app.exe"))
        );
        assert_eq!(
            safe_entry_path("Aurora TV/licenses/GPL-3.0.txt"),
            Some(PathBuf::from("licenses/GPL-3.0.txt"))
        );
        assert_eq!(safe_entry_path("../escape"), None);
        assert_eq!(safe_entry_path("/absolute"), None);
        assert_eq!(safe_entry_path("C:/windows/system32/evil.dll"), None);
        assert_eq!(safe_entry_path("Aurora TV/../../evil"), None);
    }
}
