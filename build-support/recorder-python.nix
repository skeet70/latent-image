let
  pkgs = import ./nixpkgs.nix { config = {}; overlays = []; };
  runtime = pkgs.python3.withPackages (p: [ p.numpy p.unicorn ]);
  runtimeHeader = pkgs.writeText "recorder-python-runtime.h" ''
    #define AFPY_DECLARED_ENV "${runtime}"
    #define AFPY_DECLARED_BASE "${pkgs.python3}"
    #define AFPY_DECLARED_ABI "${pkgs.python3.libPrefix}"
    #define AFPY_DECLARED_VERSION "${pkgs.python3.version}"
  '';
in pkgs.runCommand "xm1-recorder-python-runtime" {} ''
  mkdir -p "$out"
  ln -s ${runtime}/include "$out/include"
  ln -s ${runtime}/lib "$out/lib"
  ln -s ${runtime}/bin "$out/bin"
  cp ${runtimeHeader} "$out/recorder-python-runtime.h"
''
