{ pkgs ? import ./nixpkgs.nix { config = { }; overlays = [ ]; } }:
# jitl (justjake)'s FujiMod via skeet70's fork, unlicensed (LICENSING.md);
# statically links vendored skeet70/unicorn ba96845e.
let
  rev = "3d90035d7d8d651e126f89f94f30765506e6f53f";
  src = pkgs.applyPatches {
    name = "fujimod-${builtins.substring 0 8 rev}";
    src = pkgs.fetchgit {
      url = "https://github.com/skeet70/fujimod.git";
      inherit rev;
      fetchSubmodules = true;
      hash = "sha256-det+qP0ksHxacoT6ijSLVp1tU4oTsjkgEUG8Ww6OGR8=";
    };
    # uc_context's data starts 8 bytes past a 16-byte boundary, and the
    # optimized Unicorn build reads the CPUARMState copy there with movdqa.
    patches = [ ./fujimod-unicorn-context-align.patch ];
  };
  # Upstream's mise.toml pins Rust 1.99.1; cargo 1.98 rejects the workspace's
  # `default-features = false` overrides of inherited dependencies.
  rustOverlay = import (builtins.fetchTarball {
    url = "https://github.com/oxalica/rust-overlay/archive/b0829a2cc9f7b596583331d78a942c1e5170d8a0.tar.gz";
    sha256 = "sha256-M2rr/yPbTHvgAzCyZ+zHJHflFM0XU7t42Sw2skA3TQM=";
  });
  toolchain = (pkgs.extend rustOverlay).rust-bin.stable."1.99.1".minimal;
  rustPlatform = pkgs.makeRustPlatform {
    cargo = toolchain;
    rustc = toolchain;
  };
in
rustPlatform.buildRustPackage {
  pname = "fujimod-emulator";
  version = "0.1.0-${builtins.substring 0 8 rev}";
  inherit src;
  cargoHash = "sha256-5xQkfvock/4MC7VwU2TPUI26ZK1SYL1dKWRF1GqPMm8=";
  cargoBuildFlags = [ "-p" "fujimod-emulator-cli" "--bin" "fujimod-emulator" ];
  cargoTestFlags = [ "-p" "fujimod-emulator-cpu-unicorn" ];
  CARGO_BUILD_JOBS = 2;
  RUST_TEST_THREADS = 1;
  dontUseCargoParallelTests = true;
  preCheck = ''
    cargo test -p fujimod-emulator-exr2-codecs --all-features --locked -j "$CARGO_BUILD_JOBS" --profile release \
      --target ${pkgs.stdenv.targetPlatform.rust.rustcTargetSpec} --offline
    cargo test -p fujimod-emulator --locked -j "$CARGO_BUILD_JOBS" --profile release \
      --target ${pkgs.stdenv.targetPlatform.rust.rustcTargetSpec} \
      --offline --lib
  '';
  nativeBuildInputs = [ pkgs.cmake pkgs.pkg-config ];
  # The JIT execution-gate tests run their emitted Wasm under Node.
  nativeCheckInputs = [ pkgs.nodejs ];
  dontUseCmakeConfigure = true;
  passthru = { inherit rev src rustPlatform toolchain; };
}
