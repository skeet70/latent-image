# One locked package set for devenv and Bazel; no ad-hoc Cabal resolution.
let
  pkgs = import ./nixpkgs.nix { config = {}; overlays = []; };
in
pkgs.haskellPackages.ghcWithPackages (packages: [
  packages.aeson
  packages.attoparsec
  packages.cryptohash-sha256
])
