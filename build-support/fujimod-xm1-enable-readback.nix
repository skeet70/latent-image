{ pkgs ? import ./nixpkgs.nix { config = { }; overlays = [ ]; } }:
# fujimod-xm1.nix against the engine of fujimod-enable-readback.nix.
import ./fujimod-xm1.nix {
  inherit pkgs;
  fujimod = import ./fujimod-enable-readback.nix { inherit pkgs; };
}
