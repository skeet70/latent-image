# Bazel actions keyed on the file change with the store path.
let
  pkgs = import ./nixpkgs.nix { config = {}; overlays = []; };
in pkgs.runCommand "libusb-runtime" {} ''
  mkdir -p "$out/share/libusb-runtime"
  readlink -f ${pkgs.libusb1.out}/lib/libusb-1.0.so > "$out/share/libusb-runtime/store-path"
''
