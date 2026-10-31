# Replacement firmware packages

Owned X-M1 replacement code is built through the existing Bazel/Nix graph. Each package stays separate so the shared build declarations change in one place.

- [`toolchain/`](toolchain/README.md): the pinned `armv7r-none-eabi` rustc, its `core` and `compiler_builtins`, and the `xm1_rust_staticlib` / `xm1_rust_elf` macros.
- [`abi/`](abi/README.md): the #119 build and ABI qualification, including the bounded native case, the ABI defect control and the facts #120/#123 consume.
- `first_stage/`: the owned first-record startup staged at slot offset 0x6000 by `//host/firmware:build-first-stage`, with the stock record-1 helpers retained. The correct candidate's natural-reset handoff witness passes on the public runtime pin (E-FIRST-STAGE-PUBLIC-HANDOFF-150) and on combined SDK `97a6ebe` (E-FIRST-STAGE-COMBINED-SDK-HANDOFF-150). On that combined SDK the faulty-handoff and mismatched-image controls fail at their intended checks and the staged DAT cold-boots to SHOOTING MENU (1, 5, 2) (E-FIRST-STAGE-COMBINED-SDK-MENU-CONTROLS-150). The earlier private YIELD-corrected runtime results are E-FIRST-STAGE-NATURAL-HANDOFF-150 and E-FIRST-STAGE-MENU-CONTROLS-150. Full replacement startup, runtime retirement and hardware startup remain separate.
- `secondary_stage/`: the owned record-2 startup for [#135](https://github.com/skeet70/latent-image/issues/135), staged at record-2 slot offset `0x400` (guest `0x10400`) by `//host/firmware:build-secondary-stage`. Rust owns the entry decisions, MPU programming, the companion startup sequence of stock `0x11cc0` including its peripheral predicates, the event log, the rendezvous and the loader-B publication, which keeps stock's unchecked segment dereference; `entry.S` calls the retained execution-state and TCM/stack services and performs the SVC `mov pc,r3` handoff. `adapter.rs` holds the recovered addresses and the typed retained-service ABI, and `secondary-stage.ld` the eleven service entries, the retained vector/body/data extents and the candidate forbidden fetch/read ranges. The [startup inventory](../reports/replacement-inventory-baseline.md#owned-record-2-source-increment-135) records the service ABI and the loader-B register evidence. The schema-3 cohort accepts all five retained and five owned scenarios, four endpoint comparisons and the intended publication/stack fault refusals (`E-SECONDARY-STAGE-SCHEMA3-SCENARIOS-150`). An actual forbidden-stock-fetch fault control remains open; this does not qualify full startup or hardware.
- `loader_prefix/`: [#136](https://github.com/skeet70/latent-image/issues/136) compiles the secondary startup and loader-B core-1 prefix into one record-2 image. Rust owns vector installation, workspace clearing and the unbounded nonzero completion wait; assembly handles privileged setup and the caller frame. The staged loader-B entry names the interior prefix symbol. Core0 continues through retained `0xd09d0`; core1 exits at retained initialization `0xd0aac`. Host staging, SDK feature admission and package checks pass (`E-LOADER-B-PREFIX-HOST-SCOPED-SDK-150`); guest comparisons and discriminating producer/consumer controls remain open. [#137](https://github.com/skeet70/latent-image/issues/137) tracks the separate composed DAT with the owned first stage in record1.

The first-stage witness checks the guest handoff target against loader A's entry word and the staged owned bytes against the supplied candidate. A producer receipt with `result: passed` records completed observation, not consumer acceptance. The negative controls reach the handoff and collect all twelve reads without a model failure: the faulty candidate exposes target `0xdeadbe00` instead of `0x40020`, while the mismatched-image control runs the correct DAT against the faulty candidate's expected image digest. Both return nonzero from the witness consumer. The correct DAT also boots from natural reset through the fresh-settings English/date route to a rendered SHOOTING MENU page 1; visual acceptance remains the owner's check.

Record 2's retained services, vectors and data, the loaders, application images and retained record-1 helpers remain proprietary execution dependencies. These results do not establish application/runtime retirement, full replacement startup, hardware behavior, silicon timing or persistence.

```sh
devenv shell -- bazel test //replacement/abi:pilot-abi-tests --test_output=all
```

Stage the owned record-2 candidate and its two controls from the original DAT: a wrong publication (target `0xdeadbe00`) and a handoff SP 8 bytes low. The expected digest is the stored record-2 slot `[0x50000,0x60000)` of the main pre-table; the builder refuses any other source slot, a misplaced entry, an image overlapping a retained extent, a retained service inside a forbidden range and any changed byte outside the reset literal and the image:

```sh
devenv shell -- bazel --batch build --jobs=2 \
  //replacement/secondary_stage:secondary-stage-image \
  //replacement/secondary_stage:secondary-stage-publication-fault-image \
  //replacement/secondary_stage:secondary-stage-stack-fault-image \
  //host/firmware:build-secondary-stage
for image in secondary-stage-image secondary-stage-publication-fault-image secondary-stage-stack-fault-image; do
  bazel-bin/host/firmware/build-secondary-stage private-data/firmware/x-m1-1.50/FWUP0002.DAT \
    "bazel-bin/replacement/secondary_stage/$image.bin" \
    "bazel-bin/replacement/secondary_stage/$image.symbols" "PRIVATE_OUTPUT/$image.DAT" \
    --load-offset 0x400 --entry 0x10400 \
    --expect-slot-sha256 909db53de40a0319541a30314bd942cb119eca3de1147e2b856eecf71c1a4809
done
```
