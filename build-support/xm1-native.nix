{ pkgs }:
pkgs.rustPlatform.buildRustPackage {
  pname = "xm1-viewer-native";
  version = "0.1.0";
  src = builtins.path {
    path = ../emulation/xm1-upstream/native;
    name = "xm1-native-source";
    filter = path: type:
      let name = builtins.baseNameOf path;
      in name != "target" && name != "__pycache__" && name != ".git"
        && !(pkgs.lib.hasSuffix ".so" name) && !(pkgs.lib.hasSuffix ".pyc" name);
  };
  cargoLock.lockFile = ../emulation/xm1-upstream/native/Cargo.lock;
  nativeBuildInputs = [ pkgs.python3 ];
  PYO3_PYTHON = "${pkgs.python3}/bin/python3";
  doCheck = true;
  X70_UNICORN_LIBRARY = "${import ./unicorn-x70.nix { inherit pkgs; }}/lib/libunicorn.so.2";
  installPhase = ''
    runHook preInstall
    mkdir -p "$out/x70_native"
    cp python/x70_native/*.py python/x70_native/*.pyi python/x70_native/py.typed "$out/x70_native/"
    cp target/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/release/lib_native.so "$out/x70_native/_native.abi3.so"
    runHook postInstall
  '';
}
