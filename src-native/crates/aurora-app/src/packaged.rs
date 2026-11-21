//! Whether this copy was installed from the Microsoft Store (docs/RELEASE_2.0.md).
//!
//! A Store copy runs from an MSIX package: its folder under `WindowsApps` is read-only,
//! and Store policy 10.8 forbids an app from updating itself outside the Store. So a
//! packaged copy must never download, stage or swap anything — the Store does all of
//! that — and must not ask GitHub either, because the release it would find there is
//! not the build the Store will hand it.
//!
//! Decided at runtime, from the package identity Windows gives the process, rather than
//! by a build flag: the one binary goes into the installer, the portable zip *and* the
//! MSIX, so the answer cannot be baked in. `GetCurrentPackageFullName` is the documented
//! test — it fails with `APPMODEL_ERROR_NO_PACKAGE` for every unpackaged process.

/// True when running from an MSIX package, which in practice means the Store.
///
/// Computed once: identity is fixed for the life of a process.
pub fn is_packaged() -> bool {
    use std::sync::OnceLock;
    static PACKAGED: OnceLock<bool> = OnceLock::new();
    *PACKAGED.get_or_init(detect)
}

#[cfg(windows)]
fn detect() -> bool {
    use windows::Win32::Foundation::{
        APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS,
    };
    use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;

    // A zero-length buffer is the cheap probe: a packaged process answers "buffer too
    // small" with the length it needs, an unpackaged one answers "no package".
    let mut len = 0u32;
    // SAFETY: a null buffer with a zero length is the documented way to ask for the
    // required size; nothing is written through the pointer.
    let rc = unsafe { GetCurrentPackageFullName(&mut len, windows::core::PWSTR::null()) };
    if rc == APPMODEL_ERROR_NO_PACKAGE {
        return false;
    }
    rc == ERROR_INSUFFICIENT_BUFFER || rc == ERROR_SUCCESS
}

#[cfg(not(windows))]
fn detect() -> bool {
    false
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_test_process_is_not_a_store_package() {
        // cargo test runs an ordinary executable, which has no package identity. If
        // this ever reported true, every copy would stop updating itself.
        assert!(!super::is_packaged());
    }
}
