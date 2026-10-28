# X-M1 Rust cross-toolchain

`@lab_xm1_rust` is the locked nixpkgs rustc 1.98.1 with an `armv7r-none-eabi` sysroot. The pinned rustc ships no target `core`, so [`xm1-rust.nix`](xm1-rust.nix) builds `core` and `compiler_builtins` from the matching `rustLibSrc` with `-Z build-std` and installs them beside the host std. The `bin/rustc` wrapper adds `--sysroot`; Bazel actions need no registry, network or `RUSTC_BOOTSTRAP`.

## Targets

- `@lab_xm1_rust//:bin/rustc`: the sysroot wrapper.
- `@lab_xm1_rust//:share/xm1-rust/build-facts.txt`: rustc version, target flags, core and compiler_builtins source paths and SHA-256.
- `@lab_xm1_rust//:share/xm1-rust/target-spec.json`: the rustc target specification.
- `//replacement/toolchain:toolchain-facts`: both files as one filegroup.

The `devenv.lock` revision fixes rustc; the facts file fixes the core and compiler_builtins built from that same source tree.

## Macros

```python
load("//replacement/toolchain:rust.bzl", "xm1_rust_elf", "xm1_rust_staticlib")
```

`xm1_rust_staticlib(name, src, crate_name, edition, opt_level, rustc_flags)` compiles one crate root with:

```sh
rustc --target=armv7r-none-eabi --crate-type=staticlib \
  -C panic=abort -C relocation-model=static -C target-cpu=cortex-r4f \
  -C codegen-units=1 -C debuginfo=0 -C opt-level=z SRC -o NAME.a
```

`xm1_rust_elf(name, rust_staticlib, c_srcs, asm_srcs, link_script, entry, extra_ldflags)` compiles C with `--target=armv7r-none-eabi -march=armv7-r -marm -mfloat-abi=soft -ffreestanding -nostdlib -fno-builtin -fno-stack-protector -mno-unaligned-access -Oz`, links with LLD and emits four files in the caller's package:

- `NAME.elf`
- `NAME.bin`: flat image from the ELF
- `NAME.symbols`: `llvm-nm -S --defined-only --numeric-sort`, one `<address> <size> <type> <name>` row per symbol
- `NAME.disasm`: `llvm-objdump -d --no-show-raw-insn`

The linker script owns all load addresses; the link recipe adds `-nostdlib -static -fuse-ld=lld -Wl,--no-dynamic-linker -Wl,--build-id=none` and the caller's `-Wl,-e,SYMBOL`.

## Crates

Each crate supplies its own `#[panic_handler]`; the macros do not inject one. The pilot uses a `loop {}` handler and `panic = "abort"`, so no unwinding runtime is linked. A crate that needs a memory intrinsic gets it from the staticlib's `compiler_builtins` members.

## Verification

```sh
devenv shell -- bazel test //replacement/abi:pilot-abi-tests --test_output=all
```
