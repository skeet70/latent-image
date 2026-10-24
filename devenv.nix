{ pkgs, ... }:
let
  xm1Python = pkgs.python3.withPackages (p: [ p.numpy p.unicorn ]);
  xm1Native = import ./build-support/xm1-native.nix { inherit pkgs; };
  unicornX70 = import ./build-support/unicorn-x70.nix { inherit pkgs; };
  fujimod = import ./build-support/fujimod.nix { inherit pkgs; };
in {
  packages = with pkgs; [
    git
    ripgrep
    jq
    bazel_7
    buildifier
    coreutils
    diffutils
    util-linux
    gnumake
    pkg-config
    clang
    lld
    unicorn
    # Internal rules_haskell helpers and the host-only viewer require Python.
    xm1Python
    # Run the audited upstream firmware patcher in its original language.
    nodejs
    libusb1
    usbutils
    # Host-only static firmware analysis of ignored private-data/ inputs:
    # llvm-objdump/llvm-objcopy for quick ARM listings, Ghidra headless for
    # mapped projects. Neither is part of the Bazel graph.
    llvm
    ghidra
    # Host-only annotation of external session recordings (//af-session).
    ffmpeg
    exiftool
  ];

  languages.haskell = {
    enable = true;
    package = import ./build-support/ghc.nix;
    cabal.enable = false;
    stack.enable = false;
    lsp.enable = false;
  };

  env.XM1_VIEWER_PYTHON = "${xm1Python}/bin/python3";
  env.XM1_VIEWER_NATIVE_PACKAGE = "${xm1Native}";
  env.X70_UNICORN_LIBRARY = "${unicornX70}/lib/libunicorn.so.2";
  env.FUJIMOD_EMULATOR = "${fujimod}/bin/fujimod-emulator";

  # No activation hook may enumerate or open a camera.
}
