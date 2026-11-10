# `host-build` on devenv's PATH, compiled from the sources of
# //emulation:host-build with the locked GHC.
let
  pkgs = import ./nixpkgs.nix { config = { }; overlays = [ ]; };
  ghc = import ./ghc.nix;
  sources = builtins.path {
    path = ../emulation;
    name = "host-build-source";
    filter = path: type:
      let name = builtins.baseNameOf path; in
      if type == "directory" then name == "Emulation"
      else builtins.elem name [ "HostBuildRun.hs" "HostAdmission.hs" ];
  };
in
pkgs.runCommand "host-build" { nativeBuildInputs = [ ghc ]; } ''
  mkdir -p $out/bin build/FujiAF
  cp ${../host/FujiAF/StorageBudget.hs} build/FujiAF/StorageBudget.hs
  ghc -XGHC2021 -Wall -Werror -O2 -threaded -i${sources} -ibuild -outputdir build \
    -o $out/bin/host-build ${sources}/HostBuildRun.hs
''
