# GNU GCC 4.6.4 and 4.4.7 with binutils 2.24 for arm-linux-gnueabi: the EXR II
# matching compilers of jitl (justjake)'s x70view (decomp/tools/build_cpu.py
# profile exr2 plus per-file "compiler: 4.4.7" annotations; recipe
# decomp/toolchain/linux/default.nix at 6287653605).
# x70view's three GCC patches only change Darwin/arm64 hosts and are omitted.
# exr2-gcc/host-qsort.c is x70view's stable host qsort.
let
  pkgs = import ./nixpkgs.nix { config = { }; overlays = [ ]; };
  inherit (pkgs) lib;
  stdenv = pkgs.gcc13Stdenv;
  target = "arm-linux-gnueabi";
  hostFlags = "-O2 -std=gnu89 -fcommon -fno-strict-aliasing -fwrapv -Wno-error";
  hostCxxFlags = "-O2 -std=gnu++98 -fno-strict-aliasing -fwrapv -Wno-error";
  configCommit = "00b15927496058d23e6258a28d8996f87cf1f191";
  configGuess = pkgs.fetchurl {
    url = "https://raw.githubusercontent.com/chipp/gnu-config/${configCommit}/config.guess";
    sha256 = "e3d148130e9151735f8b9a8e69a70d06890ece51468a9762eb7ac0feddddcc2f";
  };
  configSub = pkgs.fetchurl {
    url = "https://raw.githubusercontent.com/chipp/gnu-config/${configCommit}/config.sub";
    sha256 = "11c54f55c3ac99e5d2c3dc2bb0bcccbf69f8223cc68f6b2438daa806cf0d16d8";
  };
  common = {
    nativeBuildInputs = [ pkgs.gnumake pkgs.gawk pkgs.bison pkgs.flex ];
    hardeningDisable = [ "all" ];
    enableParallelBuilding = true;
    dontStrip = true;
    strictDeps = true;
    LC_ALL = "C";
    LANG = "C";
    CFLAGS = hostFlags;
    CXXFLAGS = hostCxxFlags;
    MAKEINFO = "true";
    dontUpdateAutotoolsGnuConfigScripts = true;
    meta.platforms = [ "x86_64-linux" ];
    postPatch = ''
      cp ${configGuess} config.guess
      cp ${configSub} config.sub
      chmod +x config.guess config.sub
    '';
  };
  prepareBuild = stableSort: ''
    export CC=${stdenv.cc}/bin/cc CXX=${stdenv.cc}/bin/c++
    export CFLAGS='${hostFlags}' CXXFLAGS='${hostCxxFlags}'
    export CC_FOR_BUILD="$CC" CFLAGS_FOR_BUILD="$CFLAGS"
    export LDFLAGS=""
    sourceDirectory="$PWD"
    mkdir ../build
    cd ../build
    ${lib.optionalString stableSort ''
      "$CC" $CFLAGS -c ${./exr2-gcc/host-qsort.c} -o "$TMPDIR/host-qsort.o"
      export LDFLAGS="$TMPDIR/host-qsort.o"
    ''}
  '';
  binutils = stdenv.mkDerivation (common // {
    pname = "exr2-binutils";
    version = "2.24";
    src = pkgs.fetchurl {
      url = "https://ftp.gnu.org/gnu/binutils/binutils-2.24.tar.bz2";
      sha256 = "e5e8c5be9664e7f7f96e0d09919110ab5ad597794f5b1809871177a0f0f14137";
    };
    configurePhase = ''
      runHook preConfigure
      ${prepareBuild true}
      "$sourceDirectory/configure" --build=${stdenv.buildPlatform.config} \
        --host=${stdenv.hostPlatform.config} --target=${target} --prefix="$out" \
        --disable-nls --disable-werror MAKEINFO=true \
        CC="$CC" CFLAGS="$CFLAGS" LDFLAGS="$LDFLAGS"
      runHook postConfigure
    '';
    buildPhase = ''
      runHook preBuild
      make -j"$NIX_BUILD_CORES" MAKEINFO=true LDFLAGS="$LDFLAGS"
      runHook postBuild
    '';
    installPhase = ''
      runHook preInstall
      make install MAKEINFO=true LDFLAGS="$LDFLAGS"
      runHook postInstall
    '';
  });
  gccHashes = {
    "4.6.4" = "35af16afa0b67af9b8eb15cafb76d2bc5f568540552522f5dc2c88dd45d977e8";
    "4.4.7" = "5ff75116b8f763fa0fb5621af80fc6fb3ea0f1b1a57520874982f03f26cd607f";
  };
  gcc = version: sha256: stdenv.mkDerivation (common // {
    pname = "exr2-gcc";
    inherit version;
    src = pkgs.fetchurl {
      url = "https://ftp.gnu.org/gnu/gcc/gcc-${version}/gcc-${version}.tar.bz2";
      inherit sha256;
    };
    buildInputs = [ pkgs.gmp pkgs.mpfr pkgs.libmpc ];
    configurePhase = ''
      runHook preConfigure
      ${prepareBuild (version == "4.4.7")}
      "$sourceDirectory/configure" --build=${stdenv.buildPlatform.config} \
        --host=${stdenv.hostPlatform.config} --target=${target} --prefix="$out" \
        --enable-languages=c --disable-nls --disable-multilib --disable-libssp --disable-lto \
        --disable-bootstrap --without-headers --with-gnu-as --with-gnu-ld \
        --with-as=${binutils}/bin/${target}-as --with-ld=${binutils}/bin/${target}-ld \
        --with-gmp-include=${lib.getDev pkgs.gmp}/include --with-gmp-lib=${lib.getLib pkgs.gmp}/lib \
        --with-mpfr-include=${lib.getDev pkgs.mpfr}/include --with-mpfr-lib=${lib.getLib pkgs.mpfr}/lib \
        --with-mpc-include=${lib.getDev pkgs.libmpc}/include --with-mpc-lib=${lib.getLib pkgs.libmpc}/lib \
        MAKEINFO=true \
        CC="$CC" CXX="$CXX" CFLAGS="$CFLAGS" CXXFLAGS="$CXXFLAGS" LDFLAGS="''${LDFLAGS:-}"
      runHook postConfigure
    '';
    buildPhase = ''
      runHook preBuild
      make -j"$NIX_BUILD_CORES" all-gcc MAKEINFO=true LDFLAGS="''${LDFLAGS:-}"
      runHook postBuild
    '';
    installPhase = ''
      runHook preInstall
      make install-gcc MAKEINFO=true LDFLAGS="''${LDFLAGS:-}"
      runHook postInstall
    '';
  });
in
assert stdenv.hostPlatform.system == "x86_64-linux";
pkgs.runCommand "exr2-gcc-4.6.4-4.4.7-binutils-2.24" { } ''
  mkdir "$out"
  ln -s ${binutils} "$out/binutils"
  ${lib.concatStringsSep "\n" (lib.mapAttrsToList (version: sha256: ''
    ln -s ${gcc version sha256} "$out/gcc-${version}"
  '') gccHashes)}
''
