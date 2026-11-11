let
  pkgs = import ./nixpkgs.nix { config = {}; overlays = []; };
  archive = pkgs.fetchCrate {
    pname = "libm";
    version = "0.2.16";
    unpack = false;
    sha256 = "b6d2cec3eae94f9f509c767b45932f1ada8350c4bdb85af2fcab4a3c14807981";
  };
in pkgs.runCommand "libm-source-0.2.16" {
  nativeBuildInputs = [ pkgs.gnutar pkgs.gzip ];
} ''
  mkdir -p "$out"
  tar -xzf ${archive} --strip-components=1 -C "$out"
''
