{ pkgs }:
# jitl's Unicorn 2.1.4 fork (embedder branch) with the optional uc_embedder_*
# exports that emulation/xm1-upstream/native/src/ffi.rs loads.
pkgs.unicorn.overrideAttrs (old: {
  pname = "unicorn-x70";
  version = "2.1.4-embedder-ecf0e25a";
  src = pkgs.fetchFromGitHub {
    owner = "justjake";
    repo = "unicorn";
    rev = "ecf0e25a2a4c308655e30e1d3198d148c4fc0819";
    hash = "sha256-8qHXX4gz0nhinW+m5jHlNK7SBOsnB2l8SM8PS8H2Kik=";
  };
  cmakeFlags = (old.cmakeFlags or [ ]) ++ [
    "-DUNICORN_ARCH=arm"
    "-DUNICORN_BUILD_TESTS=OFF"
  ];
})
