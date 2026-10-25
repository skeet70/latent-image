# Bazel actions keyed on the files change with the store paths.
let
  pkgs = import ./nixpkgs.nix { config = {}; overlays = []; };
in pkgs.runCommand "ram-push-toolchain-paths" {} ''
  mkdir -p "$out/share/ram-push-toolchain-paths"
  readlink -f ${pkgs.bash}/bin/bash > "$out/share/ram-push-toolchain-paths/bash"
  readlink -f ${pkgs.clang}/bin/clang > "$out/share/ram-push-toolchain-paths/clang"
  readlink -f ${pkgs.lld}/bin/ld.lld > "$out/share/ram-push-toolchain-paths/ld.lld"
  readlink -f ${pkgs.llvm}/bin/llvm-nm > "$out/share/ram-push-toolchain-paths/llvm-nm"
  readlink -f ${pkgs.llvm}/bin/llvm-objcopy > "$out/share/ram-push-toolchain-paths/llvm-objcopy"
  readlink -f ${pkgs.llvm}/bin/llvm-objdump > "$out/share/ram-push-toolchain-paths/llvm-objdump"
  readlink -f ${pkgs.coreutils}/bin/sha256sum > "$out/share/ram-push-toolchain-paths/sha256sum"
''
