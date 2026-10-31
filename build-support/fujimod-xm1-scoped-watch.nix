{ pkgs ? import ./nixpkgs.nix { config = { }; overlays = [ ]; } }:
# fujimod-xm1.nix with the scoped-watch feature against the engine of fujimod-scoped-watch.nix.
import ./fujimod-xm1.nix {
  inherit pkgs;
  fujimod = import ./fujimod-scoped-watch.nix { inherit pkgs; };
  features = [ "scoped-watch" ];
}
