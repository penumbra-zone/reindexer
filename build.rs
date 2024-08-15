use std::env;
use std::process::Command;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    eprintln!("{}", out_dir);

    // Build Go static library
    Command::new("go")
        .args(&["build", "-buildmode=c-archive", "-o"])
        .arg(&format!("{}/libcometbft.a", out_dir))
        .arg("./go/cometbft.go")
        .status()
        .unwrap();

    // Link the Go static library
    println!("cargo:rustc-link-search=native={}", out_dir);
    println!("cargo:rustc-link-lib=static=cometbft");
}
