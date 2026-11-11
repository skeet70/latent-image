{ pkgs ? import ./nixpkgs.nix { config = { }; overlays = [ ]; }
, fujimod ? import ./fujimod.nix { inherit pkgs; }
, features ? [ ]
, nativeOnly ? false
}:
# emulation/fujimod-xm1 built against the source of `fujimod`, which the
# crate's path dependencies expect at ../fujimod. `deps` is compiled from the
# crate's manifest and lock alone and includes FujiMod's path crates and their
# vendored Unicorn.
let
  craneSource = builtins.fetchTarball {
    url = "https://github.com/ipetkov/crane/archive/32daa78aa882b7a43c508d1d5a5ad18a7968d731.tar.gz";
    sha256 = "06y3dvs9ms4c70r2ia50k77650yrihc09jibqbqrzly23dn42s4m";
  };
  craneLib = (import craneSource { inherit pkgs; }).overrideToolchain fujimod.passthru.toolchain;
  crate = builtins.path {
    path = ../emulation/fujimod-xm1;
    name = "fujimod-xm1-source";
    filter = path: type: builtins.baseNameOf path != "target";
  };
  checkpointModel = builtins.path {
    path = ../emulation/fujimod-xm1;
    name = "fujimod-xm1-checkpoint-model";
    filter = path: type: !(builtins.elem (builtins.baseNameOf path) [
      "target" "tests" "ram_push.rs" "composed_startup.rs" "toggle.rs"
    ]);
  };
  cargoLock = ../emulation/fujimod-xm1/Cargo.lock;
  nativeTarget = "fujimod-xm1-native-case";
  manifest = builtins.fromTOML (builtins.readFile ../emulation/fujimod-xm1/Cargo.toml);
  # Cargo builds declared binaries when an integration test is selected.
  nativeManifest = (pkgs.formats.toml { }).generate "fujimod-xm1-native-Cargo.toml" (manifest // {
    package = manifest.package // { autobins = false; };
    bin = builtins.filter (binary: binary.name == nativeTarget) manifest.bin;
  });
  binaryFlags = pkgs.lib.optionalString nativeOnly " --bin ${nativeTarget}";
  testFlags = pkgs.lib.optionalString nativeOnly " --bin ${nativeTarget} --test native_case";
  target = pkgs.stdenv.targetPlatform.rust.rustcTargetSpec;
  # `deps` and the package must unpack to the same directory: path-dependency
  # package ids and Unicorn's CMake paths include it.
  workspace = crateSource: pkgs.runCommand "fujimod-xm1-workspace" { } ''
    mkdir -p $out
    ln -s ${fujimod.src} $out/fujimod
    cp -r ${crateSource} $out/fujimod-xm1
  '';
  featureFlag = pkgs.lib.optionalString (features != [ ]) " --features ${pkgs.lib.concatStringsSep "," features}";
  cargoFlags = "--profile release --target ${target} --offline --locked${featureFlag}";
  common = {
    pname = "fujimod-xm1";
    version = "0.1.0";
    sourceRoot = "fujimod-xm1-workspace/fujimod-xm1";
    inherit cargoLock;
    cargoVendorDir = craneLib.vendorCargoDeps { inherit cargoLock; };
    # apple-cf, a macOS-only FujiMod dependency, builds its Swift bridge with
    # SwiftPM. The bridge autolinks the SDK's Swift overlays; ld64 binds its
    # libobjc imports only when libobjc is also linked explicitly.
    nativeBuildInputs = [ pkgs.cmake pkgs.pkg-config pkgs.jq ]
      ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ pkgs.swift pkgs.swiftpm ];
    # nand_image.rs links zlib for retained-identical committed NAND exports.
    buildInputs = [ pkgs.zlib ];
    dontUseCmakeConfigure = true;
  } // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isDarwin {
    NIX_LDFLAGS = "-L${pkgs.apple-sdk.sdkroot}/usr/lib/swift -lobjc";
  };
  deps = craneLib.buildDepsOnly (common // {
    dummySrc = workspace (craneLib.mkDummySrc { src = crate; inherit cargoLock; });
    buildPhaseCargoCommand = ''
      cargo build ${cargoFlags}
      cargo test --no-run ${cargoFlags}
    '';
    doCheck = false;
  });
in
# //emulation:fujimod-xm1-tests runs every exported test harness; the crate
# has no doctests.
craneLib.mkCargoDerivation (common // {
  src = workspace crate;
  pname = common.pname + pkgs.lib.optionalString nativeOnly "-native";
  postPatch = pkgs.lib.optionalString nativeOnly ''
    cp ${nativeManifest} Cargo.toml
  '';
  outputs = [ "out" "tests" ];
  cargoArtifacts = deps;
  doInstallCargoArtifacts = false;
  XM1_CHECKPOINT_MODEL_ID = builtins.hashString "sha256" (builtins.toJSON {
    engine = "${fujimod}";
    owned = "${checkpointModel}";
    build = builtins.readFile ./fujimod-xm1.nix;
    inherit features target;
  });
  buildPhaseCargoCommand = ''
    cargo build ${cargoFlags}${binaryFlags} --message-format json-render-diagnostics > build-artifacts.json
    cargo test --no-run ${cargoFlags}${testFlags} --message-format json-render-diagnostics > test-artifacts.json
  '';
  # store-path holds $out, so Bazel actions keyed on it change with the store path.
  installPhaseCommand = ''
    mkdir -p $out/bin $out/share/fujimod-xm1 $tests/libexec/fujimod-xm1-tests
    jq -r 'select(.reason == "compiler-artifact" and .target.kind == ["bin"] and .profile.test == false) | .executable' build-artifacts.json \
      | while read -r executable; do cp "$executable" $out/bin/; done
    cp data/xm1-150.json $out/share/fujimod-xm1/
    printf '%s\n' "$out" > $out/share/fujimod-xm1/store-path
    jq -r 'select(.reason == "compiler-artifact" and .profile.test == true and .executable != null) | .executable' test-artifacts.json \
      | while read -r executable; do
          cp "$executable" "$tests/libexec/fujimod-xm1-tests/"
          basename "$executable" >> "$tests/libexec/fujimod-xm1-tests/manifest"
        done
    test -s $tests/libexec/fujimod-xm1-tests/manifest
  '';
})
