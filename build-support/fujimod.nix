{ pkgs ? import ./nixpkgs.nix { config = { }; overlays = [ ]; } }:
# jitl (justjake)'s FujiMod, unlicensed (LICENSING.md); statically links its
# vendored Unicorn submodule, justjake/unicorn 0c0e055c.
let
  rev = "1d37a8717564c3b457d9f116dc2eb33bee4bd9f0";
  src = pkgs.applyPatches {
    name = "fujimod-${builtins.substring 0 8 rev}";
    src = pkgs.fetchgit {
      url = "https://github.com/justjake/FujiMod";
      inherit rev;
      fetchSubmodules = true;
      hash = "sha256-GAnNLCSDY2f3Ki5xTsY7Sz8xFeM4wHRhkWms/qtgjLM=";
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
  CARGO_BUILD_JOBS = 1;
  RUST_TEST_THREADS = 1;
  preBuild = ''
    cpulimitBin=$(mktemp -d)
    cat > "$cpulimitBin/cargo" <<EOF
#!${pkgs.runtimeShell}
exec ${pkgs.cpulimit}/bin/cpulimit -i -l 75 -- ${toolchain}/bin/cargo "\$@"
EOF
    chmod +x "$cpulimitBin/cargo"
    export PATH="$cpulimitBin:$PATH"
  '';
  nativeBuildInputs = [ pkgs.cmake pkgs.pkg-config ];
  # The JIT execution-gate tests run their emitted Wasm under Node.
  nativeCheckInputs = [ pkgs.nodejs ];
  dontUseCmakeConfigure = true;
  passthru = { inherit rev src rustPlatform toolchain; };
}
