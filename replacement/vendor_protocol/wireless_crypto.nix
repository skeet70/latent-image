{ pkgs ? import ../../build-support/nixpkgs.nix { config = { }; overlays = [ ]; }
, host ? false
}:
let
  toolchain = import ../toolchain/xm1-rust.nix { inherit pkgs; };
  vendor = pkgs.rustPlatform.importCargoLock { lockFile = ./wireless_crypto/Cargo.lock; };
  targetDirectory = if host then "release" else "armv7r-none-eabi/release";
in
pkgs.stdenv.mkDerivation {
  pname = "xm1-wireless-crypto-${if host then "host" else "arm"}";
  version = "0.1.0";
  src = builtins.path {
    path = ./wireless_crypto;
    name = "xm1-wireless-crypto-source";
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
    export CARGO_TARGET_ARMV7R_NONE_EABI_RUSTFLAGS="-C target-cpu=cortex-r4f -C target-feature=+strict-align -C relocation-model=static -C panic=abort -C debuginfo=0 -C jump-tables=no"
    mkdir -p .cargo
    cat > .cargo/config.toml <<EOF
    [source.crates-io]
    replace-with = "vendored-sources"
    [source.vendored-sources]
    directory = "${vendor}"
    EOF
    cargo build --frozen --release --lib --jobs 2 ${pkgs.lib.optionalString (!host) "--target armv7r-none-eabi"}
    runHook postBuild
  '';
  installPhase = ''
    runHook preInstall
    mkdir -p "$out/lib/deps" "$out/share/wireless-crypto"
    release="$CARGO_TARGET_DIR/${targetDirectory}"
    cp "$release/libxm1_wireless_crypto.rlib" "$out/lib/"
    for library in "$release/deps/"*.rlib; do
      case "$library" in
        */libxm1_wireless_crypto-*) ;;
        *) cp "$library" "$out/lib/deps/" ;;
      esac
    done
    cp Cargo.lock "$out/share/wireless-crypto/"
    cp ${toolchain}/share/xm1-rust/build-facts.txt "$out/share/wireless-crypto/toolchain-facts.txt"
    runHook postInstall
  '';
}
