fn main() {
    // Hangar Mobile's web app, compiled in by remote/assets.rs. Watched while
    // it exists, so a new `pnpm mobile:build` is embedded by the next release
    // build; not while it does not, since Cargo reruns a build script on every
    // build for a path that is missing.
    if std::path::Path::new("../dist-mobile").is_dir() {
        println!("cargo:rerun-if-changed=../dist-mobile");
    } else if std::env::var("PROFILE").as_deref() == Ok("release") {
        println!(
            "cargo:warning=dist-mobile/ is missing: this build serves a placeholder instead of Hangar Mobile (run `pnpm mobile:build` first)"
        );
    }
    tauri_build::build()
}
