//! The target triple the binary was built for, which `hangar-node version`
//! prints: the desktop picks the binary to install on a node from it.

fn main() {
    let target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=HANGAR_NODE_TARGET={target}");
    println!("cargo:rerun-if-changed=build.rs");
}
