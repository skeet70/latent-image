def heap_bridge_replay(name):
    native.genrule(
        name = name,
        srcs = [
            "Cargo.lock", "Cargo.toml", "src.rs", "backend.rs", "tests.rs", "replay.rs", "replay.nix",
            "//:devenv.lock", "//build-support:nixpkgs.nix",
            "//emulation:fujimod-xm1/src/heap_state.rs",
            "//emulation:fujimod-xm1/src/heap_replay_state.rs",
        ],
        outs = [name + ".bin"],
        cmd = """
set -eu
output=$$(nix-build --no-out-link --max-jobs 1 --cores 2 $(location replay.nix))
cp "$$output/bin/heap-bridge-replay" "$@"
chmod 755 "$@"
""",
        executable = True,
        local = True,
        tags = ["manual"],
    )
