{ pkgs ? import ../../build-support/nixpkgs.nix { config = { }; overlays = [ ]; }, backend }:
let
  admittedBackend = assert builtins.elem backend [ "rlsf" "talc" "buddy" ]; backend;
  toolchain = import ../toolchain/xm1-rust.nix { inherit pkgs; };
  dependencies = pkgs.rustPlatform.importCargoLock { lockFile = ./Cargo.lock; };
  vendor = pkgs.runCommand "heap-replay-vendor" { } ''
    mkdir -p "$out"
    for crate in ${dependencies}/* ${pkgs.rustPlatform.rustLibSrc}/vendor/*; do
      if [ -f "$crate/Cargo.toml" ]; then
        name="$(basename "$crate")"
        if [ ! -e "$out/$name" ]; then
          ln -s "$crate" "$out/$name"
        fi
      fi
    done
  '';
in
pkgs.stdenv.mkDerivation {
  pname = "xm1-heap-replay-${admittedBackend}";
  version = "0.1.0";
  src = pkgs.lib.fileset.toSource {
    root = ./.;
    fileset = pkgs.lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./src.rs ./observations.rs ];
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
    export RUSTC_BOOTSTRAP=1
    export RUSTC="${toolchain}/bin/rustc"
    export RUSTFLAGS="-C target-cpu=cortex-r4f -C relocation-model=static -C panic=abort -C codegen-units=1 -C debuginfo=0"
    mkdir -p .cargo
    cat > .cargo/config.toml <<EOF
    [source.crates-io]
    replace-with = "vendored-sources"
    [source.vendored-sources]
    directory = "${vendor}"
    EOF
    cargo build --frozen --release --lib --jobs 2 --target armv7r-none-eabi \
      --no-default-features --features ${admittedBackend} \
      -Z build-std=core,alloc,compiler_builtins -Z build-std-features=compiler-builtins-mem
    runHook postBuild
  '';
  installPhase = ''
    mkdir -p "$out/lib" "$out/share/heap-replay"
    cp "$CARGO_TARGET_DIR/armv7r-none-eabi/release/libxm1_heap_replay.a" "$out/lib/"
    cp Cargo.lock "$out/share/heap-replay/"
    cp ${toolchain}/share/xm1-rust/build-facts.txt "$out/share/heap-replay/toolchain-facts.txt"
  '';
}
