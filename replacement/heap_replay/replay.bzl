load("//replacement/toolchain:rust.bzl", "xm1_rust_elf")

def heap_replay_subject(name, backend):
    if backend not in ["rlsf", "talc", "buddy"]:
        fail("unsupported replay backend")
    native.genrule(
        name = name + "-core",
        srcs = [
            "Cargo.lock",
            "Cargo.toml",
            "heap-replay.nix",
            "observations.rs",
            "src.rs",
            "//:devenv.lock",
            "//build-support:nixpkgs.nix",
            "//replacement/toolchain:xm1-rust.nix",
        ],
        outs = [name + "-core.a"],
        cmd = """
set -eu
output=$$(nix-build --no-out-link --max-jobs 1 --cores 2 --argstr backend %s $(location heap-replay.nix))
cp "$$output/lib/libxm1_heap_replay.a" "$@"
""" % backend,
        local = True,
        tags = ["manual"],
    )
    xm1_rust_elf(
        name = name,
        rust_staticlib = ":" + name + "-core",
        asm_srcs = ["entry.S"],
        link_script = "heap-replay.ld",
        entry = "heap_replay_init",
        extra_ldflags = ["-Wl,--undefined=" + symbol for symbol in [
            "heap_replay_init", "heap_replay_alloc", "heap_replay_free", "heap_replay_footprint",
            "heap_replay_stats", "heap_replay_zero", "heap_replay_copy", "heap_replay_status", "heap_replay_panic",
        ]],
    )
