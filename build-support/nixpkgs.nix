# Consume the same source revision/NAR hash as devenv without user overlays.
let
  lock = builtins.fromJSON (builtins.readFile ../devenv.lock);
  source = builtins.fetchTree lock.nodes.nixpkgs-src.locked;
in
import source.outPath
