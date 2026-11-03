{ pkgs ? import ../../build-support/nixpkgs.nix { config = { }; overlays = [ ]; } }:
let
  heapSource = builtins.path {
    path = ./.;
    name = "general-heap-host-source";
    filter = path: type: type == "directory" || builtins.elem (builtins.baseNameOf path)
      [ "Cargo.toml" "Cargo.lock" "src.rs" "backend.rs" "tests.rs" "replay.rs" ];
  };
  tapeSource = builtins.path {
    path = ../../emulation/fujimod-xm1/src;
    name = "admitted-heap-tape-source";
    filter = path: type: type == "directory" || builtins.elem (builtins.baseNameOf path)
      [ "heap_state.rs" "heap_replay_state.rs" ];
  };
  workspace = pkgs.runCommand "heap-bridge-replay-workspace" { } ''
    mkdir -p "$out/replacement" "$out/emulation/fujimod-xm1"
    cp -r ${heapSource} "$out/replacement/general_heap"
    cp -r ${tapeSource} "$out/emulation/fujimod-xm1/src"
  '';
in
pkgs.rustPlatform.buildRustPackage {
  pname = "xm1-heap-bridge-replay";
  version = "0.1.0";
  src = workspace;
  sourceRoot = "heap-bridge-replay-workspace/replacement/general_heap";
  cargoLock.lockFile = ./Cargo.lock;
  buildFeatures = [ "host-replay" ];
  cargoBuildFlags = [ "--bin" "heap-bridge-replay" ];
  cargoInstallFlags = [ "--bin" "heap-bridge-replay" ];
  CARGO_BUILD_JOBS = 2;
  RUSTC_BOOTSTRAP = "1";
  doCheck = false;
}
