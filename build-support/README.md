# Bazel and Nix integration

Bazel owns the project build graph and host tests. The project structure is Haskell/Rust, with C limited to technically necessary boundaries. The preserved viewer's Rust extension currently builds through the locked Nix package. Nix/devenv provides pinned compilers, libraries and validation tools. The initial supported build host is Linux x86_64 with Nix; this platform is not a firmware target.

## Pinned inputs

- `.bazelversion` selects Bazel 7.6.0, matching the `bazel_7` package in `devenv.nix`.
- `MODULE.bazel` selects Haskell rules 1.0 and Nixpkgs rules 0.14.0, plus C/shell/platform support. `MODULE.bazel.lock` records resolved module metadata and extension inputs. The Nix C/POSIX modules come from integrity-checked subdirectories of the upstream release archive.
- `nixpkgs.nix` reads the `nixpkgs-src` revision and NAR hash directly from `devenv.lock`. It imports that source with no user overlays through the Nixpkgs rules. `ghc.nix` selects GHC 9.10.3 with Aeson 2.2.4.1 and Attoparsec 0.14.4 from that exact package set; both devenv and Bazel use it. `repositories.bzl` also declares GCC, Unicorn, headers-only libusb, jq, and check-jsonschema repositories from this input. The libusb header target adds no runtime link or initialization; [the transport provider](../transport/README.md#native-provider) binds its dynamically loaded runtime separately.
- `rules-haskell-nix.patch` changes two Nix integration dependencies in the released Haskell module from development-only to ordinary dependencies. This gives the non-root module's Nix macros the repository visibility they need. It changes no compiler or runtime code.

Bazel 7 is deliberate: the released Haskell/POSIX integration still uses APIs removed in Bazel 8. Upgrade the rules and Bazel together after validating the Nix toolchains, Haskell binary, C library discovery, and tests. Do not replace the pinned toolchains with compilers selected implicitly from the host `PATH`.

## Commands

```sh
devenv shell -- bazel build //...
devenv shell -- bazel test //...
devenv shell -- bazel run //transport:camera -- --help
devenv shell -- bazel run //emulation:unicorn-info
```

The initial acquisition can download pinned Bazel modules and Nix store paths. After acquisition, check that the module lock is current and rerun tests without Bazel repository downloads:

```sh
devenv shell -- bazel test --lockfile_mode=error --repository_disable_download --nocache_test_results //...
```

The C import uses Unicorn's `libunicorn.so.2` runtime name so Bazel's runtime symlinks satisfy its SONAME in both `bazel run` and sandboxed tests. No `LD_LIBRARY_PATH` override is required. Upstream Haskell rule helpers and the schema checker use Python internally; project-authored tools use Haskell/Rust, with C limited to necessary boundaries under [current principles](../docs/principles.md#implementation-and-build). JSON parsing uses the shared strict Haskell parser, including duplicate-key rejection, and canonical review bindings retain sorted ASCII JSON semantics.

## Extending the graph

Add dependencies through pinned module/Nix inputs and explicit target declarations. Update `.bazelversion` and the devenv Bazel package together; regenerate the module lock, format with buildifier, and validate all targets. Record both lockfile hashes and effective build flags with experiment artifacts. Host checks do not verify firmware architecture, target ABI, or camera behavior.

Nix-backed actions rely on the Nix store and current host platform. Remote workers need equivalent declared toolchain support before remote execution is enabled. Private camera data and workspace tool metadata are excluded from wildcard package discovery by `.bazelignore`; source and redistributable fixtures remain visible. Keep private artifacts out of shared caches unless an explicitly private cache scope is established.
