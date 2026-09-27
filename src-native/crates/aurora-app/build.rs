fn main() {
    // `metadata::BUILT_IN_KEY` is an `option_env!`, which cargo does not track: with a
    // warm target directory, changing the secret leaves the old key — or no key —
    // compiled into the binary, and nothing says so. CI rebuilds from a cache often
    // enough for that to be a real way to ship the wrong one.
    println!("cargo:rerun-if-env-changed=AURORA_TMDB_KEY");

    // Delay-load libmpv, so that a missing or unreadable DLL is something the app can
    // report rather than something the Windows loader kills it for.
    //
    // `libmpv2-sys` links `mpv.lib`, which makes `libmpv-2.dll` a load-time import: the
    // loader resolves it before `main` runs, so `create_backend`'s fallback to
    // `NullBackend` was unreachable and the process simply ended — 0xC0000135 with no
    // window, no log and no message (F-34). Delay-loading moves that decision to the
    // first call, and `MpvBackend::new` makes it explicitly with `LoadLibraryW`.
    //
    // This belongs here rather than in `aurora-player` because `rustc-link-arg` from a
    // library applies to that library's own artifact, not to the binary being linked.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-arg-bins=/DELAYLOAD:libmpv-2.dll");
        println!("cargo:rustc-link-arg-bins=delayimp.lib");
    }

    tauri_build::build();
}
