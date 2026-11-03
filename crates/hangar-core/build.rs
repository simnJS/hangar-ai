fn main() {
    // Hangar Mobile's web app, compiled in by remote/assets.rs from the
    // repository's dist-mobile/, two levels up from this crate. Watched while
    // it exists, so a new `pnpm mobile:build` is embedded by the next release
    // build; not while it does not, since Cargo reruns a build script on every
    // build for a path that is missing.
    //
    // What src-tauri/build.rs did besides, `tauri_build::build()`, was the
    // window's: its configuration, icons and Windows manifest belong to the
    // app crate now.
    if std::path::Path::new("../../dist-mobile").is_dir() {
        println!("cargo:rerun-if-changed=../../dist-mobile");
    } else if std::env::var("PROFILE").as_deref() == Ok("release") {
        println!(
            "cargo:warning=dist-mobile/ is missing: this build serves a placeholder instead of Hangar Mobile (run `pnpm mobile:build` first)"
        );
    }
    // Without a rerun-if line of its own, Cargo would rerun this script
    // whenever any file of the crate changes.
    println!("cargo:rerun-if-changed=build.rs");
}
