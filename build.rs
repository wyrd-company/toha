// ---
// relationships:
//   implements: architecture
// ---
//! Sets `TOHA_VERSION` from the git tags of the toha repository.
#[cfg(feature = "cli")]
#[path = "build/version.rs"]
mod version;

#[cfg(feature = "cli")]
use std::{env, path::PathBuf};

#[cfg(feature = "cli")]
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build/version.rs");
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let fallback = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let resolution = version::resolve(&root, &fallback);
    for path in &resolution.watched {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    if let Some(warning) = &resolution.warning {
        println!("cargo:warning={warning}");
    }
    println!("cargo:rustc-env=TOHA_VERSION={}", resolution.version);
}

#[cfg(not(feature = "cli"))]
fn main() {
    println!("cargo:rustc-env=TOHA_VERSION={}", env!("CARGO_PKG_VERSION"));
}
