load("//replacement/toolchain:rust.bzl", "xm1_rust_elf")

_CORE_INPUTS = [
    "Cargo.lock", "Cargo.toml", "general-heap.nix", "src.rs", "backend.rs", "tests.rs", "replay.rs",
    "//:devenv.lock", "//build-support:nixpkgs.nix", "//replacement/toolchain:xm1-rust.nix",
]

def general_heap_subject(name, recipe_arena = False):
    native.genrule(
        name = name + "-core",
        srcs = _CORE_INPUTS,
        outs = [name + "-core.a"],
        cmd = """
set -eu
output=$$(nix-build --no-out-link --max-jobs 1 --cores 2 --arg recipeArena %s $(location general-heap.nix))
cp "$$output/lib/libxm1_general_heap.a" "$@"
""" % ("true" if recipe_arena else "false"),
        local = True,
        tags = ["manual"],
    )
    xm1_rust_elf(
        name = name,
        rust_staticlib = ":" + name + "-core",
        asm_srcs = ["entry.S"],
        link_script = "general-heap.ld",
        entry = "xm1_heap_malloc_entry",
        extra_ldflags = ["-Wl,--gc-sections", "-Wl,--orphan-handling=error", "-Wl,--enable-non-contiguous-regions"] + ["-Wl,--undefined=" + symbol for symbol in [
            "xm1_heap_malloc_r", "xm1_heap_free_r", "xm1_heap_realloc_r", "xm1_heap_calloc_r", "xm1_heap_trim_r",
        ]],
    )

def general_heap_host_tests(name, recipe_arena = False):
    native.genrule(
        name = name,
        srcs = _CORE_INPUTS,
        outs = [name + ".Cargo.lock"],
        cmd = """
set -eu
output=$$(nix-build --no-out-link --max-jobs 1 --cores 2 --arg hostTests true --arg recipeArena %s $(location general-heap.nix))
cp "$$output/share/general-heap/Cargo.lock" "$@"
""" % ("true" if recipe_arena else "false"),
        local = True,
        tags = ["manual"],
    )
