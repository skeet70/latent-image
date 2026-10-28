{ pkgs ? import ../../build-support/nixpkgs.nix { config = { }; overlays = [ ]; } }:
# The pinned rustc ships no armv7r-none-eabi core; core and compiler_builtins
# are built from its matching rustLibSrc and installed beside the host std.
let
  inherit (pkgs) rustc cargo;
  target = "armv7r-none-eabi";
  library = pkgs.rustPlatform.rustLibSrc;
  rustHost = "${rustc}/bin/rustc";
in
pkgs.stdenv.mkDerivation {
  pname = "xm1-rust";
  version = rustc.version;
  dontUnpack = true;
  nativeBuildInputs = [ rustc cargo pkgs.coreutils pkgs.findutils ];

  buildPhase = ''
    set -eu
    export HOME="$TMPDIR"
    export CARGO_HOME="$TMPDIR/cargo-home"
    export CARGO_NET_OFFLINE=true
    export CARGO_TARGET_DIR="$TMPDIR/target"
    export RUSTC_BOOTSTRAP=1

    toolchain="$(${rustHost} --print sysroot)"
    sysroot="$TMPDIR/sysroot"
    mkdir -p "$sysroot"
    cp -rs "$toolchain"/. "$sysroot"/
    chmod -R u+w "$sysroot"
    mkdir -p "$sysroot/lib/rustlib/src/rust"
    ln -s ${library} "$sysroot/lib/rustlib/src/rust/library"
    cat > "$TMPDIR/rustc-sysroot" <<EOF
#!${pkgs.runtimeShell}
exec ${rustHost} --sysroot $sysroot "\$@"
EOF
    chmod +x "$TMPDIR/rustc-sysroot"
    export RUSTC="$TMPDIR/rustc-sysroot"

    probe="$TMPDIR/probe"
    mkdir -p "$probe/src" "$probe/.cargo"
    cat > "$probe/Cargo.toml" <<'EOF'
[package]
name = "xm1-rust-sysroot-probe"
version = "0.0.0"
edition = "2021"
[lib]
crate-type = ["rlib"]
[profile.release]
opt-level = "z"
debug = false
panic = "abort"
codegen-units = 1
EOF
    cat > "$probe/src/lib.rs" <<'EOF'
#![no_std]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
EOF
    cat > "$probe/.cargo/config.toml" <<EOF
[source.crates-io]
replace-with = "vendored-sources"
[source.vendored-sources]
directory = "${library}/vendor"
[target.${target}]
rustflags = ["-C", "target-cpu=cortex-r4f", "-C", "relocation-model=static", "-C", "debuginfo=0"]
EOF

    ( cd "$probe"
      cargo build --offline --release -Z build-std=core,compiler_builtins --target ${target}
    )

    mkdir -p "$out/sysroot"
    cp -rs "$toolchain"/. "$out/sysroot"/
    chmod -R u+w "$out/sysroot"
    mkdir -p "$out/sysroot/lib/rustlib/src/rust"
    ln -s ${library} "$out/sysroot/lib/rustlib/src/rust/library"
    mkdir -p "$out/sysroot/lib/rustlib/${target}/lib"
    deps="$TMPDIR/target/${target}/release/deps"
    find "$deps" -maxdepth 1 -name 'libcore-*.rlib' -exec cp {} "$out/sysroot/lib/rustlib/${target}/lib/" \;
    find "$deps" -maxdepth 1 -name 'libcompiler_builtins-*.rlib' -exec cp {} "$out/sysroot/lib/rustlib/${target}/lib/" \;

    mkdir -p "$out/bin" "$out/share/xm1-rust"
    cat > "$out/bin/rustc" <<EOF
#!${pkgs.runtimeShell}
exec ${rustHost} --sysroot $out/sysroot "\$@"
EOF
    chmod +x "$out/bin/rustc"

    ${rustHost} --print target-spec-json --target ${target} -Z unstable-options \
      > "$out/share/xm1-rust/target-spec.json"
    core_rlib="$(find "$out/sysroot/lib/rustlib/${target}/lib" -name 'libcore-*.rlib' | head -n 1)"
    builtins_rlib="$(find "$out/sysroot/lib/rustlib/${target}/lib" -name 'libcompiler_builtins-*.rlib' | head -n 1)"
    {
      echo "rustc_version=$(${rustHost} --version)"
      echo "target=${target}"
      echo "target_cpu=cortex-r4f"
      echo "panic=abort"
      echo "relocation_model=static"
      echo "core_rlib=$core_rlib"
      echo "core_sha256=$(sha256sum "$core_rlib" | cut -d' ' -f1)"
      echo "compiler_builtins_rlib=$builtins_rlib"
      echo "compiler_builtins_sha256=$(sha256sum "$builtins_rlib" | cut -d' ' -f1)"
      echo "core_source=${library}/core"
      echo "compiler_builtins_source=${library}/compiler-builtins/compiler-builtins"
    } > "$out/share/xm1-rust/build-facts.txt"
  '';

  installPhase = "true";
}
