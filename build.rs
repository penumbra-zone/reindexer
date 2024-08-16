use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let go_dir = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("go");

    // Build Go static library
    Command::new("go")
        .args(&["build", "-buildmode=c-archive", "-o"])
        .arg(&format!("{}/libcometbft.a", out_dir))
        .arg("cometbft.go")
        .current_dir(&go_dir)
        .status()
        .unwrap();

    // Link the Go static library
    println!("cargo:rustc-link-search=native={}", out_dir);
    println!("cargo:rustc-link-lib=static=cometbft");
    // Rerun the build when the Go changes
    println!("cargo::rerun-if-changed=go");
}
