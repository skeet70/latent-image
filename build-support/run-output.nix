# `run-output` on devenv's PATH, compiled from the sources of
# //build-support:run-output with the locked GHC.
let
  pkgs = import ./nixpkgs.nix { config = { }; overlays = [ ]; };
  ghc = import ./ghc.nix;
in
pkgs.runCommand "run-output" { nativeBuildInputs = [ ghc ]; } ''
  mkdir -p $out/bin build/FujiAF
  cp ${../host/FujiAF/RunOutput.hs} build/FujiAF/RunOutput.hs
  cp ${../host/FujiAF/StorageBudget.hs} build/FujiAF/StorageBudget.hs
  ghc -XGHC2021 -Wall -Werror -O2 -threaded -ibuild -outputdir build \
    -o $out/bin/run-output ${./RunOutputMain.hs}
''
