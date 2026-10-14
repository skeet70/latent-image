"""Import the locked Nix toolchains and host libraries into Bazel."""

load("@rules_haskell//haskell:nixpkgs.bzl", "haskell_register_ghc_nixpkgs")
load("@rules_nixpkgs_cc//:cc.bzl", "nixpkgs_cc_configure")
load("@rules_nixpkgs_core//:nixpkgs.bzl", "nixpkgs_local_repository", "nixpkgs_package")

_UNICORN_BUILD = """
load("@rules_cc//cc:defs.bzl", "cc_import", "cc_library")
package(default_visibility = ["//visibility:public"])
# Preserve the SONAME in Bazel's runtime symlinks as well as at link time.
cc_import(name = "shared", shared_library = "lib/libunicorn.so.2")
cc_library(
    name = "unicorn",
    hdrs = glob(["include/unicorn/*.h"]),
    includes = ["include"],
    deps = [":shared"],
)
"""

_LIBUSB_HEADERS_BUILD = """
load("@rules_cc//cc:defs.bzl", "cc_library")
package(default_visibility = ["//visibility:public"])
# Camera operations open the runtime by path, as does the transport test that
# exercises the lister. Do not link libusb or run its initialization anywhere
# else.
cc_library(
    name = "headers",
    hdrs = glob(["include/libusb-1.0/*.h"]),
    includes = ["include/libusb-1.0"],
)
"""

# Exported for its store path alone; nothing depends on these bytes at link time.
_LIBUSB_RUNTIME_BUILD = """
exports_files(["lib/libusb-1.0.so"], visibility = ["//visibility:public"])
"""

def _lab_dependencies_impl(_ctx):
    nixpkgs_local_repository(
        name = "lab_nixpkgs",
        nix_file = "//build-support:nixpkgs.nix",
        nix_file_deps = ["//:devenv.lock"],
    )
    nixpkgs_cc_configure(
        name = "lab_cc",
        repository = "@lab_nixpkgs",
        register = False,
    )
    haskell_register_ghc_nixpkgs(
        name = "lab_ghc",
        version = "9.10.3",
        attribute_path = "",
        nix_file = "//build-support:ghc.nix",
        nix_file_deps = ["//build-support:nixpkgs.nix", "//:devenv.lock"],
        repository = "@lab_nixpkgs",
        register = False,
    )
    nixpkgs_package(
        name = "lab_unicorn",
        attribute_path = "unicorn",
        repository = "@lab_nixpkgs",
        build_file_content = _UNICORN_BUILD,
    )
    nixpkgs_package(
        name = "lab_libusb",
        attribute_path = "libusb1.dev",
        repository = "@lab_nixpkgs",
        build_file_content = _LIBUSB_HEADERS_BUILD,
    )
    nixpkgs_package(
        name = "lab_libusb_runtime",
        attribute_path = "libusb1.out",
        repository = "@lab_nixpkgs",
        build_file_content = _LIBUSB_RUNTIME_BUILD,
    )
    for name, attribute, binary in [
        ("lab_jsonschema", "check-jsonschema", "check-jsonschema"),
        ("lab_jq", "jq", "jq"),
        ("lab_exiftool", "exiftool", "exiftool"),
    ]:
        nixpkgs_package(
            name = name,
            attribute_path = attribute,
            repository = "@lab_nixpkgs",
            build_file_content = 'exports_files(["bin/%s"], visibility = ["//visibility:public"])' % binary,
        )
    # //af-session reads external session recordings on the host.
    nixpkgs_package(
        name = "lab_ffmpeg",
        attribute_path = "ffmpeg",
        repository = "@lab_nixpkgs",
        build_file_content = 'exports_files(["bin/ffmpeg", "bin/ffprobe"], visibility = ["//visibility:public"])',
    )

lab_dependencies = module_extension(implementation = _lab_dependencies_impl)
