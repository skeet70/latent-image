# Replacement firmware packages

Owned X-M1 replacement code is built through the existing Bazel/Nix graph. Each package stays separate so the shared build declarations change in one place.

- [`toolchain/`](toolchain/README.md): the pinned `armv7r-none-eabi` rustc, its `core` and `compiler_builtins`, and the `xm1_rust_staticlib` / `xm1_rust_elf` macros.
- [`abi/`](abi/README.md): the #119 build and ABI qualification, including the bounded native case, the ABI defect control and the facts #120/#123 consume.

```sh
devenv shell -- bazel test //replacement/abi:pilot-abi-tests --test_output=all
```
