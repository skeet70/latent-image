# Consume the same source revision/NAR hash as devenv without user overlays.
# fetchTarball, unlike fetchTree, needs no experimental Nix feature.
let
  lock = builtins.fromJSON (builtins.readFile ../devenv.lock);
  locked = lock.nodes.nixpkgs-src.locked;
  source = builtins.fetchTarball {
    url = "https://github.com/${locked.owner}/${locked.repo}/archive/${locked.rev}.tar.gz";
    sha256 = locked.narHash;
  };
in
import source
