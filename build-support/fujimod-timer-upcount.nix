{ pkgs ? import ./nixpkgs.nix { config = { }; overlays = [ ]; } }:
# fujimod.nix with fujimod-timer-upcount.patch (kernel counter rises in mode
# 0x23, E-XM1-TIMER-IRQ-LAWS-150); the check phase adds the timer crate's tests.
let
  fujimod = import ./fujimod.nix {
    inherit pkgs;
    extraPatches = [ ./fujimod-timer-upcount.patch ];
  };
in
fujimod.overrideAttrs (old: {
  preCheck = old.preCheck + ''
    cargo test -p fujimod-emulator-exr2-system --locked -j "$CARGO_BUILD_JOBS" --profile release \
      --target ${pkgs.stdenv.targetPlatform.rust.rustcTargetSpec} --offline --lib
  '';
})
