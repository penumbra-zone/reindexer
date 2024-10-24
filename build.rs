//! The `penumbra-indexer` binary requires a CometBFT C archive
//! for linking. That build-time dependency must be built via go.
//! By default, cargo will try to use `go` on PATH to perform
//! the build. If you wish to build the C archive yourself,
//! you can provide a fullpath to it via the env var
//!
//!   PENUMBRA_REINDEXER_STATIC_LIB
//!
//! Currently the cargo cache is invalidated if this `build.rs` file
//! changes. Makes sense, maybe that's default.

use std::env;
use std::path::PathBuf;
use std::process::Command;

// Build the C archive for cometbft via golang.
//
// Assumes that a sufficient golang development environment exists,
// including `go` on the PATH. If you wish to prebuild the C archive
// and pass it into the cargo build, set `PENUMBRA_REINDEXER_STATIC_LIB` instead.
fn build_cometbft_c_archive() -> PathBuf {
    // Cargo will automatically set `OUT_DIR` depending on the target and profile.
    let cargo_out_dir = PathBuf::from(env::var("OUT_DIR").expect("cargo build should set OUT_DIR"));

    // Set the golang source directory to `go` within the crate root.
    let go_source_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("cargo build should set CARGO_MANIFEST_DIR"),
    )
    .join("go");

    // Build Go static library
    let archive_filepath = cargo_out_dir.join("libcometbft.a");
    let status = Command::new("go")
        .args(&[
            "build",
            "-buildmode=c-archive",
            "-o",
            archive_filepath
                .as_os_str()
                .to_str()
                .expect("failed to convert c-archive filepath to str"),
        ])
        .current_dir(&go_source_dir)
        .arg("cometbft.go")
        .status()
        .expect("failed to run go build command; make sure go is installed and on PATH");
    assert!(
        status.success(),
        "failed to build c-archive of cometbft code"
    );
    archive_filepath
}

fn main() {
    // Check if a prebuilt static lib is provided.
    let static_lib = match env::var("PENUMBRA_REINDEXER_STATIC_LIB") {
        Ok(p) => {
            let p = PathBuf::from(&p);
            // Check that the path actually exists, otherwise build failures will be confusing
            assert!(
                p.exists(),
                "static lib for cometbft not found: {}",
                &p.display()
            );
            p
        }
        // Fall back to building the required static lib automatically, via local `go`.
        Err(_e) => build_cometbft_c_archive(),
    };

    let static_lib_dir = static_lib
        .parent()
        .expect("failed to find parent directory of archive");

    // Link the Go static library
    println!(
        "cargo:rustc-link-search=native={}",
        static_lib_dir.display()
    );
    println!("cargo:rustc-link-lib=static=cometbft");

    // Rerun the build if the static lib changed, but only if was provided out of band.
    // Otherwise, we'll bust the build cache based on timestamp of the `go` directory.
    println!("cargo::rerun-if-env-changed=PENUMBRA_REINDEXER_STATIC_LIB");
    if let Ok(_) = env::var("PENUMBRA_REINDEXER_STATIC_LIB") {
        println!("cargo::rerun-if-changed={}", static_lib_dir.display());
    }
    // Rerun the build when the Go code changes
    println!("cargo::rerun-if-changed=go");
}
