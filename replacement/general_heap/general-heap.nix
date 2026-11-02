{ pkgs ? import ../../build-support/nixpkgs.nix { config = { }; overlays = [ ]; }
, recipeArena ? false
, hostTests ? false
}:
let
  toolchain = if hostTests then pkgs.rustc else import ../toolchain/xm1-rust.nix { inherit pkgs; };
  dependencies = pkgs.rustPlatform.importCargoLock { lockFile = ./Cargo.lock; };
  vendor = pkgs.runCommand "general-heap-vendor" { } ''
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
  features = pkgs.lib.concatStringsSep "," (
    pkgs.lib.optional recipeArena "recipe-arena"
    ++ pkgs.lib.optional hostTests "host"
  );
in
pkgs.stdenv.mkDerivation {
  pname = "xm1-general-heap" + pkgs.lib.optionalString hostTests "-host-tests";
  version = "0.1.0";
  src = builtins.path {
    path = ./.;
    name = "xm1-general-heap-source";
    filter = path: type: type == "directory" || builtins.elem (builtins.baseNameOf path)
      [ "Cargo.toml" "Cargo.lock" "src.rs" "backend.rs" "tests.rs" "replay.rs" ];
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
    mkdir -p .cargo
    cat > .cargo/config.toml <<EOF
    [source.crates-io]
    replace-with = "vendored-sources"
    [source.vendored-sources]
    directory = "${vendor}"
    EOF
    ${if hostTests then ''
      cargo test --frozen --lib --jobs 2 --features ${features}
    '' else ''
      export RUSTFLAGS="-C target-cpu=cortex-r4f -C relocation-model=static -C panic=abort -C codegen-units=1 -C debuginfo=0"
      cargo build --frozen --release --lib --jobs 2 --target armv7r-none-eabi \
        --no-default-features ${pkgs.lib.optionalString (features != "") "--features ${features}"} \
        -Z build-std=core,alloc,compiler_builtins -Z build-std-features=compiler-builtins-mem
    ''}
    runHook postBuild
  '';
  installPhase = if hostTests then ''
    mkdir -p "$out/share/general-heap"
    cp Cargo.lock "$out/share/general-heap/"
  '' else ''
    mkdir -p "$out/lib" "$out/share/general-heap"
    cp "$CARGO_TARGET_DIR/armv7r-none-eabi/release/libxm1_general_heap.a" "$out/lib/"
    cp "$CARGO_TARGET_DIR/armv7r-none-eabi/release/libxm1_general_heap.rlib" "$out/lib/"
    cp Cargo.lock "$out/share/general-heap/"
    cp ${toolchain}/share/xm1-rust/build-facts.txt "$out/share/general-heap/toolchain-facts.txt"
  '';
}
