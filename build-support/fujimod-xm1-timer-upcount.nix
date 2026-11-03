{ pkgs ? import ./nixpkgs.nix { config = { }; overlays = [ ]; } }:
# fujimod-xm1.nix against the engine of fujimod-timer-upcount.nix.
import ./fujimod-xm1.nix {
  inherit pkgs;
  fujimod = import ./fujimod-timer-upcount.nix { inherit pkgs; };
}
