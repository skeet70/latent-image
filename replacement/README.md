# Replacement firmware packages

Owned X-M1 replacement code is built through the existing Bazel/Nix graph. Each package stays separate so the shared build declarations change in one place.

- [`toolchain/`](toolchain/README.md): the pinned `armv7r-none-eabi` rustc, its `core` and `compiler_builtins`, and the `xm1_rust_staticlib` / `xm1_rust_elf` macros.
- [`abi/`](abi/README.md): the #119 build and ABI qualification, including the bounded native case, the ABI defect control and the facts #120/#123 consume.
- `first_stage/`: the owned first-record startup staged at slot offset 0x6000 by `//host/firmware:build-first-stage`, with the stock record-1 helpers retained. The correct candidate's natural-reset handoff witness now passes on the public runtime pin (E-FIRST-STAGE-PUBLIC-HANDOFF-150). Faulty-handoff, mismatched-image and cold-menu replay remain pending on that runtime. The earlier SHOOTING MENU (1, 5, 2) and negative-control results remain limited to the reviewed private YIELD-corrected runtime (E-FIRST-STAGE-NATURAL-HANDOFF-150, E-FIRST-STAGE-MENU-CONTROLS-150). Full replacement startup, runtime retirement and hardware startup remain separate.

The first-stage witness checks the guest handoff target against loader A's entry word and the staged owned bytes against the supplied candidate. A producer receipt with `result: passed` records completed observation, not consumer acceptance. The negative controls reach the handoff and collect all twelve reads without a model failure: the faulty candidate exposes target `0xdeadbe00` instead of `0x40020`, while the mismatched-image control runs the correct DAT against the faulty candidate's expected image digest. Both return nonzero from the witness consumer. The correct DAT also boots from natural reset through the fresh-settings English/date route to a rendered SHOOTING MENU page 1; visual acceptance remains the owner's check.

Record 2, the loaders, application images and retained record-1 helpers remain proprietary execution dependencies. Core 1's startup path is untraced. These results do not establish application/runtime retirement, full replacement startup, hardware behavior, silicon timing or persistence.

```sh
devenv shell -- bazel test //replacement/abi:pilot-abi-tests --test_output=all
```
