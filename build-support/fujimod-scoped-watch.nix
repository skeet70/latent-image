{ pkgs ? import ./nixpkgs.nix { config = { }; overlays = [ ]; } }:
# fujimod.nix with fujimod-scoped-watch.patch; its regressions run in the
# emulator crate's existing `--lib` check.
import ./fujimod.nix {
  inherit pkgs;
  extraPatches = [ ./fujimod-scoped-watch.patch ];
}
