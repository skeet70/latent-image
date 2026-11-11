{ pkgs ? import ./nixpkgs.nix { config = { }; overlays = [ ]; }
, fujimod ? import ./fujimod.nix { inherit pkgs; }
}:
# replacement/develop/tools/checkpoint-low-prefix as a third directory of the
# fujimod-xm1 workspace. It restores the SDK's dependency artifacts into the
# SDK's own target directory and compiles fujimod-xm1 with the SDK's checkpoint
# model identity, so a checkpoint key computed here equals the SDK's.
let
  sdk = import ./fujimod-xm1.nix { inherit pkgs fujimod; };
  craneSource = builtins.fetchTarball {
    url = "https://github.com/ipetkov/crane/archive/32daa78aa882b7a43c508d1d5a5ad18a7968d731.tar.gz";
    sha256 = "06y3dvs9ms4c70r2ia50k77650yrihc09jibqbqrzly23dn42s4m";
  };
  craneLib = (import craneSource { inherit pkgs; }).overrideToolchain fujimod.passthru.toolchain;
  source = name: path: builtins.path {
    inherit name path;
    filter = path: type: builtins.baseNameOf path != "target";
  };
  workspace = pkgs.runCommand "fujimod-xm1-workspace" { } ''
    mkdir -p $out
    ln -s ${fujimod.src} $out/fujimod
    cp -r ${source "fujimod-xm1-source" ../emulation/fujimod-xm1} $out/fujimod-xm1
    cp -r ${source "xm1-checkpoint-low-prefix-source" ../replacement/develop/tools/checkpoint-low-prefix} $out/xm1-checkpoint-low-prefix
  '';
  target = pkgs.stdenv.targetPlatform.rust.rustcTargetSpec;
in
craneLib.mkCargoDerivation {
  pname = "xm1-checkpoint-low-prefix";
  version = "0.1.0";
  src = workspace;
  sourceRoot = "fujimod-xm1-workspace/xm1-checkpoint-low-prefix";
  cargoLock = ../replacement/develop/tools/checkpoint-low-prefix/Cargo.lock;
  inherit (sdk) cargoVendorDir cargoArtifacts XM1_CHECKPOINT_MODEL_ID;
  CARGO_TARGET_DIR = "../fujimod-xm1/target";
  CARGO_BUILD_JOBS = 2;
  postUnpack = ''
    chmod -R u+w fujimod-xm1-workspace/fujimod-xm1
  '';
  nativeBuildInputs = [ pkgs.cmake pkgs.pkg-config ];
  buildInputs = [ pkgs.zlib ];
  dontUseCmakeConfigure = true;
  doInstallCargoArtifacts = false;
  buildPhaseCargoCommand = ''
    cargo build --profile release --target ${target} --offline --locked
  '';
  installPhaseCommand = ''
    mkdir -p $out/bin $out/share/xm1-checkpoint-low-prefix
    cp ../fujimod-xm1/target/${target}/release/xm1-checkpoint-low-prefix $out/bin/
    printf '%s\n' "$XM1_CHECKPOINT_MODEL_ID" > $out/share/xm1-checkpoint-low-prefix/checkpoint-model-id
    printf '%s\n' "${sdk}" > $out/share/xm1-checkpoint-low-prefix/sdk-store-path
  '';
}
