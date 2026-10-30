{ pkgs ? import ./nixpkgs.nix { config = { }; overlays = [ ]; } }:
# fujimod.nix with fujimod-enable-readback.patch; the check phase adds the
# image-input crate's tests.
let
  fujimod = import ./fujimod.nix {
    inherit pkgs;
    extraPatches = [ ./fujimod-enable-readback.patch ];
  };
in
fujimod.overrideAttrs (old: {
  preCheck = old.preCheck + ''
    cargo test -p fujimod-emulator-exr2-isp-input --locked -j "$CARGO_BUILD_JOBS" --profile release \
      --target ${pkgs.stdenv.targetPlatform.rust.rustcTargetSpec} --offline --lib
  '';
})
