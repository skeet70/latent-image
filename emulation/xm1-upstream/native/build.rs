use std::process::Command;

fn main() {
    let compiler = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let output = Command::new(compiler)
        .arg("--version")
        .output()
        .expect("rustc version");
    assert!(output.status.success(), "rustc version command failed");
    println!(
        "cargo:rustc-env=X70_RUSTC_VERSION={}",
        String::from_utf8(output.stdout).unwrap().trim()
    );
    println!(
        "cargo:rustc-env=X70_TARGET={}",
        std::env::var("TARGET").unwrap()
    );
    println!("cargo:rerun-if-changed=build.rs");
}
