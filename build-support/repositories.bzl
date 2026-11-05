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

# Headers only: the embedding loads libpython from the declared base interpreter at run time.
_RECORDER_PYTHON_BUILD = """
load("@rules_cc//cc:defs.bzl", "cc_library")
package(default_visibility = ["//visibility:public"])
_headers = glob(["include/python*/Python.h"])
_header = _headers[0] if len(_headers) == 1 else fail("Recorder embedding requires exactly one Python.h")
cc_library(
    name = "headers",
    hdrs = ["recorder-python-runtime.h"] + glob(["include/python*/**/*.h"]),
    includes = [".", _header[:-9]],
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
exports_files(["share/libusb-runtime/store-path"], visibility = ["//visibility:public"])
"""

_FUJIMOD_NIX_DEPS = [
    "//:devenv.lock",
    "//build-support:fujimod-classified-watch.patch",
    "//build-support:fujimod-composite-cpu.patch",
    "//build-support:fujimod-composite-watch.patch",
    "//build-support:fujimod-fetch-attribution.patch",
    "//build-support:fujimod-rprofile-byte-exclusives.patch",
    "//build-support:fujimod-timer-upcount.patch",
    "//build-support:fujimod-unicorn-context-align.patch",
    "//build-support:fujimod-wifi-unanswered-cmd53.patch",
    "//build-support:fujimod.nix",
    "//build-support:nixpkgs.nix",
]

# Bazel invalidation dependencies for the crate copied by fujimod-xm1.nix.
_FUJIMOD_XM1_CRATE = [
    "//emulation:fujimod-xm1/Cargo.lock",
    "//emulation:fujimod-xm1/Cargo.toml",
    "//emulation:fujimod-xm1/data/xm1-150.json",
    "//emulation:fujimod-xm1/src/adapters.rs",
    "//emulation:fujimod-xm1/src/adjustment.rs",
    "//emulation:fujimod-xm1/src/application_observation.rs",
    "//emulation:fujimod-xm1/src/application_observation/capture.rs",
    "//emulation:fujimod-xm1/src/application_observation/checks.rs",
    "//emulation:fujimod-xm1/src/application_observation/controls.rs",
    "//emulation:fujimod-xm1/src/application_observation/planes.rs",
    "//emulation:fujimod-xm1/src/application_observation/readiness.rs",
    "//emulation:fujimod-xm1/src/attach_relations.rs",
    "//emulation:fujimod-xm1/src/attach_state.rs",
    "//emulation:fujimod-xm1/src/bridge.rs",
    "//emulation:fujimod-xm1/src/composed_catalog.rs",
    "//emulation:fujimod-xm1/src/composed_mutations.rs",
    "//emulation:fujimod-xm1/src/composed_receipt.rs",
    "//emulation:fujimod-xm1/src/composed_startup.rs",
    "//emulation:fujimod-xm1/src/ff80.rs",
    "//emulation:fujimod-xm1/src/frame_capture.rs",
    "//emulation:fujimod-xm1/src/guest.rs",
    "//emulation:fujimod-xm1/src/handoff.rs",
    "//emulation:fujimod-xm1/src/heap_accesses.rs",
    "//emulation:fujimod-xm1/src/heap_cutover.rs",
    "//emulation:fujimod-xm1/src/heap_cutover_check.rs",
    "//emulation:fujimod-xm1/src/heap_cutover_contract.rs",
    "//emulation:fujimod-xm1/src/heap_cutover_guard.rs",
    "//emulation:fujimod-xm1/src/heap_observe.rs",
    "//emulation:fujimod-xm1/src/heap_replay.rs",
    "//emulation:fujimod-xm1/src/heap_replay_check.rs",
    "//emulation:fujimod-xm1/src/heap_replay_contract.rs",
    "//emulation:fujimod-xm1/src/heap_replay_runtime.rs",
    "//emulation:fujimod-xm1/src/heap_replay_state.rs",
    "//emulation:fujimod-xm1/src/heap_state.rs",
    "//emulation:fujimod-xm1/src/kernel_install.rs",
    "//emulation:fujimod-xm1/src/continuation_view.rs",
    "//emulation:fujimod-xm1/src/loader_continuations.rs",
    "//emulation:fujimod-xm1/src/loader_continuations_check.rs",
    "//emulation:fujimod-xm1/src/loader_orchestration_owned.rs",
    "//emulation:fujimod-xm1/src/loader_reset_check.rs",
    "//emulation:fujimod-xm1/src/loader_shared_check.rs",
    "//emulation:fujimod-xm1/src/loader_shared_controls.rs",
    "//emulation:fujimod-xm1/src/loader_shared_policy.rs",
    "//emulation:fujimod-xm1/src/loader_startup_program.rs",
    "//emulation:fujimod-xm1/src/loader_timer_check.rs",
    "//emulation:fujimod-xm1/src/plain_program.rs",
    "//emulation:fujimod-xm1/src/plain_wire.rs",
    "//emulation:fujimod-xm1/src/plain_runtime.rs",
    "//emulation:fujimod-xm1/src/plain_check.rs",
    "//emulation:fujimod-xm1/src/image5_program.rs",
    "//emulation:fujimod-xm1/src/image5_wire.rs",
    "//emulation:fujimod-xm1/src/image5_runtime.rs",
    "//emulation:fujimod-xm1/src/image5_check.rs",
    "//emulation:fujimod-xm1/src/lib.rs",
    "//emulation:fujimod-xm1/src/loader_code.rs",
    "//emulation:fujimod-xm1/src/machine.rs",
    "//emulation:fujimod-xm1/src/main.rs",
    "//emulation:fujimod-xm1/src/memory_view.rs",
    "//emulation:fujimod-xm1/src/modal_guard.rs",
    "//emulation:fujimod-xm1/src/modal_lifetime.rs",
    "//emulation:fujimod-xm1/src/modal_memory.rs",
    "//emulation:fujimod-xm1/src/modal_permit.rs",
    "//emulation:fujimod-xm1/src/modal_task.rs",
    "//emulation:fujimod-xm1/src/modal_trace.rs",
    "//emulation:fujimod-xm1/src/nand_image.rs",
    "//emulation:fujimod-xm1/src/native_case.rs",
    "//emulation:fujimod-xm1/src/native_case_main.rs",
    "//emulation:fujimod-xm1/src/own_first_pair.rs",
    "//emulation:fujimod-xm1/src/peer_witness.rs",
    "//emulation:fujimod-xm1/src/peer_witness_critical_pair.rs",
    "//emulation:fujimod-xm1/src/peer_witness_initializer.rs",
    "//emulation:fujimod-xm1/src/peer_witness_interrupt_mask.rs",
    "//emulation:fujimod-xm1/src/peer_witness_loader.rs",
    "//emulation:fujimod-xm1/src/peer_witness_primary_initializer.rs",
    "//emulation:fujimod-xm1/src/peer_witness_primary_owned.rs",
    "//emulation:fujimod-xm1/src/peer_witness_primary_root0.rs",
    "//emulation:fujimod-xm1/src/peer_witness_startup.rs",
    "//emulation:fujimod-xm1/src/peer_witness_vectors.rs",
    "//emulation:fujimod-xm1/src/peer_witness_wait.rs",
    "//emulation:fujimod-xm1/src/pool_allocation.rs",
    "//emulation:fujimod-xm1/src/power_hold.rs",
    "//emulation:fujimod-xm1/src/provenance.rs",
    "//emulation:fujimod-xm1/src/provenance_main.rs",
    "//emulation:fujimod-xm1/src/queue_boundary.rs",
    "//emulation:fujimod-xm1/src/r4f.rs",
    "//emulation:fujimod-xm1/src/ram_push.rs",
    "//emulation:fujimod-xm1/src/render.rs",
    "//emulation:fujimod-xm1/src/run.rs",
    "//emulation:fujimod-xm1/src/reset_observation.rs",
    "//emulation:fujimod-xm1/src/root_compact.rs",
    "//emulation:fujimod-xm1/src/rtos_census.rs",
    "//emulation:fujimod-xm1/src/sd_host.rs",
    "//emulation:fujimod-xm1/src/startup_callers.rs",
    "//emulation:fujimod-xm1/src/startup_consumers.rs",
    "//emulation:fujimod-xm1/src/startup_consumers_shared.rs",
    "//emulation:fujimod-xm1/src/startup_dispatch.rs",
    "//emulation:fujimod-xm1/src/startup_dispatch_check.rs",
    "//emulation:fujimod-xm1/src/startup_dispatch_shared.rs",
    "//emulation:fujimod-xm1/src/startup_joined.rs",
    "//emulation:fujimod-xm1/src/startup_sites.rs",
    "//emulation:fujimod-xm1/src/startup_tasks.rs",
    "//emulation:fujimod-xm1/src/idle_tasks.rs",
    "//emulation:fujimod-xm1/src/task_constructors.rs",
    "//emulation:fujimod-xm1/src/task_storage.rs",
    "//emulation:fujimod-xm1/src/task_storage_check.rs",
    "//emulation:fujimod-xm1/src/task_submission_control.rs",
    "//emulation:fujimod-xm1/src/task_submission_program.rs",
    "//emulation:fujimod-xm1/src/task_submission_runtime.rs",
    "//emulation:fujimod-xm1/src/toggle.rs",
    "//emulation:fujimod-xm1/src/trace.rs",
    "//emulation:fujimod-xm1/src/trace_save_lifecycle.rs",
    "//emulation:fujimod-xm1/src/ui_conversion.rs",
    "//emulation:fujimod-xm1/src/updater.rs",
    "//emulation:fujimod-xm1/src/witness.rs",
    "//emulation:fujimod-xm1/tests/application_observation.rs",
    "//emulation:fujimod-xm1/tests/composed_startup.rs",
    "//emulation:fujimod-xm1/tests/data/composed-contract-v4.json",
    "//emulation:fujimod-xm1/tests/exclusive_monitor.rs",
    "//emulation:fujimod-xm1/tests/modal_lifetime.rs",
    "//emulation:fujimod-xm1/tests/modal_lifetime_config.rs",
    "//emulation:fujimod-xm1/tests/modal_permit.rs",
    "//emulation:fujimod-xm1/tests/modal_physical_guard.rs",
    "//emulation:fujimod-xm1/tests/modal_trace.rs",
    "//emulation:fujimod-xm1/tests/native_case.rs",
    "//emulation:fujimod-xm1/tests/peer_wait_sdk.rs",
    "//emulation:fujimod-xm1/tests/peer_witness.rs",
    "//emulation:fujimod-xm1/tests/r4f_guard.rs",
    "//emulation:fujimod-xm1/tests/support/composed_nand_fixture.rs",
    "//emulation:fujimod-xm1/tests/trace_throughput.rs",
    "//emulation:fujimod-xm1/tests/trace_throughput_retain.rs",
]

_NIX_BUILD_CAP = ["--max-jobs", "2", "--cores", "2"]
_RUST_NIX_BUILD_CAP = ["--max-jobs", "1", "--cores", "2"]

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
        name = "lab_recorder_python",
        nix_file = "//build-support:recorder-python.nix",
        nix_file_deps = ["//build-support:nixpkgs.nix", "//:devenv.lock"],
        repository = "@lab_nixpkgs",
        build_file_content = _RECORDER_PYTHON_BUILD,
    )
    nixpkgs_package(
        name = "lab_libusb",
        attribute_path = "libusb1.dev",
        repository = "@lab_nixpkgs",
        build_file_content = _LIBUSB_HEADERS_BUILD,
    )
    nixpkgs_package(
        name = "lab_libusb_runtime",
        nix_file = "//build-support:libusb-runtime.nix",
        nix_file_deps = ["//build-support:nixpkgs.nix", "//:devenv.lock"],
        repository = "@lab_nixpkgs",
        build_file_content = _LIBUSB_RUNTIME_BUILD,
    )
    for name, attribute, binary in [
        ("lab_jsonschema", "check-jsonschema", "check-jsonschema"),
        ("lab_jq", "jq", "jq"),
        ("lab_rustc", "rustc", "rustc"),
        ("lab_exiftool", "exiftool", "exiftool"),
        ("lab_git", "git", "git"),
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

    # //emulation:card-with-files writes files into a card image's FAT partition.
    nixpkgs_package(
        name = "lab_mtools",
        attribute_path = "mtools",
        repository = "@lab_nixpkgs",
        build_file_content = 'exports_files(["bin/mcopy", "bin/mdir", "bin/mtype"], visibility = ["//visibility:public"])',
    )
    nixpkgs_package(
        name = "lab_dosfstools",
        attribute_path = "dosfstools",
        repository = "@lab_nixpkgs",
        build_file_content = 'exports_files(["bin/mkfs.vfat"], visibility = ["//visibility:public"])',
    )
    nixpkgs_package(
        name = "lab_fujimod",
        nix_file = "//build-support:fujimod.nix",
        nix_file_deps = _FUJIMOD_NIX_DEPS,
        nixopts = _RUST_NIX_BUILD_CAP,
        repository = "@lab_nixpkgs",
        build_file_content = 'exports_files(["bin/fujimod-emulator"], visibility = ["//visibility:public"])',
    )
    nixpkgs_package(
        name = "lab_fujimod_xm1",
        nix_file = "//build-support:fujimod-xm1.nix",
        nix_file_deps = _FUJIMOD_NIX_DEPS + _FUJIMOD_XM1_CRATE,
        nixopts = _RUST_NIX_BUILD_CAP,
        repository = "@lab_nixpkgs",
        build_file_content = 'exports_files(["bin/fujimod-xm1", "bin/fujimod-xm1-composed-startup", "bin/fujimod-xm1-heap-observe", "bin/fujimod-xm1-heap-replay", "bin/fujimod-xm1-idle-tasks", "bin/fujimod-xm1-kernel-install", "bin/fujimod-xm1-loader-continuations", "bin/fujimod-xm1-native-case", "bin/fujimod-xm1-own-first-pair", "bin/fujimod-xm1-provenance", "bin/fujimod-xm1-queue-boundary", "bin/fujimod-xm1-ram-push", "bin/fujimod-xm1-root-compact", "bin/fujimod-xm1-rtos-census", "bin/fujimod-xm1-startup-callers", "bin/fujimod-xm1-startup-consumers", "bin/fujimod-xm1-startup-dispatch", "bin/fujimod-xm1-startup-tasks", "bin/fujimod-xm1-task-storage", "bin/fujimod-xm1-toggle", "bin/fujimod-xm1-updater", "share/fujimod-xm1/store-path", "share/fujimod-xm1/xm1-150.json"], visibility = ["//visibility:public"])\nfilegroup(name = "test-harnesses", srcs = glob(["libexec/fujimod-xm1-tests/*"]), visibility = ["//visibility:public"])\nfilegroup(name = "test-manifest", srcs = ["libexec/fujimod-xm1-tests/manifest"], visibility = ["//visibility:public"])',
    )

    # The RAM-push relinker's tools; ram-push-toolchain-paths records their
    # store paths.
    for name, attribute, binaries in [
        ("lab_bash", "bash", ["bin/bash"]),
        ("lab_clang", "clang", ["bin/clang"]),
        ("lab_lld", "lld", ["bin/ld.lld"]),
        ("lab_llvm", "llvm", ["bin/llvm-objcopy", "bin/llvm-nm", "bin/llvm-objdump", "bin/llvm-readelf"]),
        ("lab_coreutils", "coreutils", ["bin/sha256sum", "bin/sort"]),
    ]:
        nixpkgs_package(
            name = name,
            attribute_path = attribute,
            repository = "@lab_nixpkgs",
            build_file_content = "exports_files(%r, visibility = [\"//visibility:public\"])" % binaries,
        )

    # //replacement/toolchain:rust.bzl cross-compiles no_std ARM modules with
    # this pinned rustc, its matching core and compiler_builtins.
    nixpkgs_package(
        name = "lab_xm1_rust",
        nix_file = "//replacement/toolchain:xm1-rust.nix",
        nix_file_deps = ["//build-support:nixpkgs.nix", "//:devenv.lock"],
        repository = "@lab_nixpkgs",
        build_file_content = 'exports_files(["bin/rustc", "share/xm1-rust/build-facts.txt", "share/xm1-rust/target-spec.json"], visibility = ["//visibility:public"])',
    )

    # //host/firmware:ram-push-toolchain-module bakes these store paths into
    # RamPushToolchain.hs.
    nixpkgs_package(
        name = "lab_ram_push_toolchain_paths",
        nix_file = "//build-support:ram-push-toolchain-paths.nix",
        nix_file_deps = ["//build-support:nixpkgs.nix", "//:devenv.lock"],
        repository = "@lab_nixpkgs",
        build_file_content = "exports_files(%r, visibility = [\"//visibility:public\"])" % [
            "share/ram-push-toolchain-paths/bash",
            "share/ram-push-toolchain-paths/clang",
            "share/ram-push-toolchain-paths/ld.lld",
            "share/ram-push-toolchain-paths/llvm-nm",
            "share/ram-push-toolchain-paths/llvm-objcopy",
            "share/ram-push-toolchain-paths/llvm-objdump",
            "share/ram-push-toolchain-paths/sha256sum",
        ],
    )

    # //matching:pilot compiles x70view C with these.
    nixpkgs_package(
        name = "lab_exr2_gcc",
        nix_file = "//build-support:exr2-gcc.nix",
        nix_file_deps = [
            "//build-support:nixpkgs.nix",
            "//build-support:exr2-gcc/host-qsort.c",
            "//:devenv.lock",
        ],
        repository = "@lab_nixpkgs",
        build_file_content = 'exports_files(["gcc-4.6.4/bin/arm-linux-gnueabi-gcc", "gcc-4.4.7/bin/arm-linux-gnueabi-gcc", "binutils/bin/arm-linux-gnueabi-ld"], visibility = ["//visibility:public"])',
    )

lab_dependencies = module_extension(implementation = _lab_dependencies_impl)
