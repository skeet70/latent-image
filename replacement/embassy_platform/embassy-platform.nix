{ pkgs ? import ../../build-support/nixpkgs.nix { config = { }; overlays = [ ]; }
, features ? [ "counter-up" ]
, hostTests ? false
}:
# Target builds produce the staticlib; hostTests runs the crate's host unit tests instead.
let
  toolchain = import ../toolchain/xm1-rust.nix { inherit pkgs; };
  vendor = pkgs.rustPlatform.importCargoLock { lockFile = ./Cargo.lock; };
  featureFlags = "--no-default-features --features ${builtins.concatStringsSep "," features}";
in
pkgs.stdenv.mkDerivation {
  pname = if hostTests then "xm1-embassy-platform-tests" else "xm1-embassy-platform";
  version = "0.1.0";
  src = builtins.path {
    path = ./.;
    name = "xm1-embassy-platform-source";
    filter = path: type:
      let name = builtins.baseNameOf path; in
      type == "directory" && name == "src" || builtins.elem name [ "Cargo.toml" "Cargo.lock" ] || pkgs.lib.hasSuffix ".rs" name;
  };
  nativeBuildInputs = [ pkgs.cargo toolchain ];
  doCheck = false;
  dontConfigure = true;
  dontFixup = true;
  buildPhase = ''
    runHook preBuild
    export HOME="$TMPDIR"
    export CARGO_HOME="$TMPDIR/cargo-home"
    export CARGO_TARGET_DIR="$TMPDIR/target"
    export CARGO_NET_OFFLINE=true
    export RUSTC="${toolchain}/bin/rustc"
    export CARGO_TARGET_ARMV7R_NONE_EABI_RUSTFLAGS="-C target-cpu=cortex-r4f -C relocation-model=static -C panic=abort -C debuginfo=0"
    mkdir -p .cargo
    cat > .cargo/config.toml <<EOF
    [source.crates-io]
    replace-with = "vendored-sources"
    [source.vendored-sources]
    directory = "${vendor}"
    EOF
    ${if hostTests then ''
      cargo test --frozen --lib --jobs 2 ${featureFlags} 2>&1 | tee test.log
    '' else ''
      cargo build --frozen --release --lib --jobs 2 --target armv7r-none-eabi ${featureFlags}
    ''}
    runHook postBuild
  '';
  installPhase = if hostTests then ''
    mkdir -p "$out"
    cp test.log "$out/test.log"
  '' else ''
    mkdir -p "$out/lib" "$out/share/embassy-platform"
    cp "$CARGO_TARGET_DIR/armv7r-none-eabi/release/libxm1_embassy_platform.a" "$out/lib/"
    cp Cargo.lock "$out/share/embassy-platform/"
    cp ${toolchain}/share/xm1-rust/build-facts.txt "$out/share/embassy-platform/toolchain-facts.txt"
  '';
}
