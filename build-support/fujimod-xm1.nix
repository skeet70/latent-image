{ pkgs ? import ./nixpkgs.nix { config = { }; overlays = [ ]; } }:
# emulation/fujimod-xm1 built against the FujiMod source of fujimod.nix,
# which the crate's path dependencies expect at ../fujimod.
let
  fujimod = import ./fujimod.nix { inherit pkgs; };
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
in
fujimod.passthru.rustPlatform.buildRustPackage {
  pname = "fujimod-xm1";
  version = "0.1.0";
  inherit src;
  sourceRoot = "fujimod-xm1-workspace/fujimod-xm1";
  cargoLock.lockFile = ../emulation/fujimod-xm1/Cargo.lock;
  nativeBuildInputs = [ pkgs.cmake pkgs.pkg-config ];
  dontUseCmakeConfigure = true;
  # store-path holds $out, so Bazel actions keyed on it change with the store path.
  postInstall = ''
    mkdir -p $out/share/fujimod-xm1
    cp data/xm1-150.json $out/share/fujimod-xm1/
    printf '%s\n' "$out" > $out/share/fujimod-xm1/store-path
  '';
}
