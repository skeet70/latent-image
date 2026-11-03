{ pkgs ? import ../../build-support/nixpkgs.nix { config = { }; overlays = [ ]; } }:
# Host proc-macro and build-script crates compile against the host std that
# xm1-rust keeps in its sysroot; target crates use its prebuilt core.
let
  toolchain = import ../toolchain/xm1-rust.nix { inherit pkgs; };
  vendor = pkgs.rustPlatform.importCargoLock { lockFile = ./Cargo.lock; };
in
pkgs.stdenv.mkDerivation {
  pname = "xm1-embassy-trial";
  version = "0.1.0";
  src = builtins.path {
    path = ./.;
    name = "xm1-embassy-trial-source";
    filter = path: type: builtins.elem (builtins.baseNameOf path) [ "Cargo.toml" "Cargo.lock" "src.rs" ];
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
    cargo build --frozen --release --lib --jobs 2 --target armv7r-none-eabi
    runHook postBuild
  '';
  installPhase = ''
    mkdir -p "$out/lib" "$out/share/embassy-trial"
    cp "$CARGO_TARGET_DIR/armv7r-none-eabi/release/libxm1_embassy_trial.a" "$out/lib/"
    cp "$(find "$CARGO_TARGET_DIR/armv7r-none-eabi/release/build" -path '*aarch32-rt-*/out/link.x')" "$out/lib/aarch32-rt-link.x"
    find "$CARGO_TARGET_DIR/release/deps" -maxdepth 1 -name 'lib*.so' -printf '%f\n' \
      | sed 's/^lib\(.*\)-[0-9a-f]*\.so$/\1/' | sort > "$out/share/embassy-trial/host-proc-macros.txt"
    cp Cargo.lock "$out/share/embassy-trial/"
    cp ${toolchain}/share/xm1-rust/build-facts.txt "$out/share/embassy-trial/toolchain-facts.txt"
  '';
}
