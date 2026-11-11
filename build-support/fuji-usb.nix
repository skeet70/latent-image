# The camera transport's USB provider, transport/usb, as a static library. The
# check phase runs its unit tests.
let
  pkgs = import ./nixpkgs.nix { config = { }; overlays = [ ]; };
in
pkgs.rustPlatform.buildRustPackage {
  pname = "fuji-usb";
  version = "0.1.0";
  src = builtins.path {
    path = ../transport/usb;
    name = "fuji-usb-source";
    filter = path: type: builtins.baseNameOf path != "target";
  };
  cargoLock.lockFile = ../transport/usb/Cargo.lock;
  installPhase = ''
    runHook preInstall
    mkdir -p $out/lib
    cp target/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/release/libfuji_usb.a $out/lib/
    runHook postInstall
  '';
}
