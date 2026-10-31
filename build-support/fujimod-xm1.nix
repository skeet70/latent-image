{ pkgs ? import ./nixpkgs.nix { config = { }; overlays = [ ]; }
, fujimod ? import ./fujimod.nix { inherit pkgs; }
, features ? [ ]
}:
# emulation/fujimod-xm1 built against the source of `fujimod`, which the
# crate's path dependencies expect at ../fujimod.
let
  crate = builtins.path {
    path = ../emulation/fujimod-xm1;
    name = "fujimod-xm1-source";
    filter = path: type: builtins.baseNameOf path != "target";
  };
  src = pkgs.runCommand "fujimod-xm1-workspace" { } ''
    mkdir -p $out
    ln -s ${fujimod.src} $out/fujimod
    cp -r ${crate} $out/fujimod-xm1
  '';
  featureFlag = pkgs.lib.optionalString (features != [ ]) " --features ${pkgs.lib.concatStringsSep "," features}";
in
fujimod.passthru.rustPlatform.buildRustPackage ({
  pname = "fujimod-xm1";
  version = "0.1.0";
  inherit src;
  sourceRoot = "fujimod-xm1-workspace/fujimod-xm1";
  cargoLock.lockFile = ../emulation/fujimod-xm1/Cargo.lock;
  CARGO_BUILD_JOBS = 2;
  RUST_TEST_THREADS = 1;
  dontUseCargoParallelTests = true;
  nativeBuildInputs = [ pkgs.cmake pkgs.pkg-config pkgs.jq ];
  nativeCheckInputs = [ fujimod ];
  # nand_image.rs links zlib for retained-identical committed NAND exports.
  buildInputs = [ pkgs.zlib ];
  dontUseCmakeConfigure = true;
  preCheck = ''
    cargo test --no-run --message-format=json -j "$CARGO_BUILD_JOBS" --profile release \
      --target ${pkgs.stdenv.targetPlatform.rust.rustcTargetSpec} --offline${featureFlag} > test-artifacts.json
  '';
  # store-path holds $out, so Bazel actions keyed on it change with the store path.
  postInstall = ''
    mkdir -p $out/share/fujimod-xm1
    cp data/xm1-150.json $out/share/fujimod-xm1/
    printf '%s\n' "$out" > $out/share/fujimod-xm1/store-path
    mkdir -p $out/libexec/fujimod-xm1-tests
    ${pkgs.jq}/bin/jq -r 'select(.reason == "compiler-artifact" and .profile.test == true and .executable != null) | .executable' test-artifacts.json \
      | while read -r executable; do
          cp "$executable" "$out/libexec/fujimod-xm1-tests/"
          basename "$executable" >> "$out/libexec/fujimod-xm1-tests/manifest"
        done
    test -s $out/libexec/fujimod-xm1-tests/manifest
  '';
} // pkgs.lib.optionalAttrs (features != [ ]) {
  buildFeatures = features;
  checkFeatures = features;
})
