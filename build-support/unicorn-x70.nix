{ pkgs }:
# jitl's Unicorn 2.1.4 fork with the optional uc_x70_* exports that
# emulation/xm1-upstream/native/src/ffi.rs loads. f1246785 is the last
# commit exporting that prefix; the branch renamed it to uc_embedder_*.
pkgs.unicorn.overrideAttrs (old: {
  pname = "unicorn-x70";
  version = "2.1.4-x70-f1246785";
  src = pkgs.fetchFromGitHub {
    owner = "justjake";
    repo = "unicorn";
    rev = "f12467851d10175cd2856089c39c7e561c9cb25e";
    hash = "sha256-pszHgW8yMt65+7SveenlQ+bjbLkooIYgWur1gdO+tRw=";
  };
  cmakeFlags = (old.cmakeFlags or [ ]) ++ [
    "-DUNICORN_ARCH=arm"
    "-DUNICORN_BUILD_TESTS=OFF"
  ];
})
