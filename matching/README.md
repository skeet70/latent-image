# Matching C pilot (MATCHING-C-01)

`//matching:pilot` compiles the C translation units that jitl (justjake)'s [x70view](https://github.com/justjake/x70view) X-M1 1.50 decomp tree assigns to each function in [pilot-functions.json](pilot-functions.json), links each function's section at its stock address and compares the linked bytes with the owner's decoded images. Results and their limits are in [the firmware extraction report](../reports/firmware-extraction.md#matching-c-recompilation-lead).

```sh
devenv shell -- bazel run //matching:pilot -- \
  --x70view /path/to/x70view --out private-data/matching-c/NEW_RUN \
  --image loader-a=private-data/analysis/x-m1-1.50/decode/loader-40000.bin \
  --image loader-b=private-data/analysis/x-m1-1.50/decode/loader-d0000.bin \
  --image image-4=private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin \
  --image image-5=private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin \
  --image plain=private-data/analysis/x-m1-1.50/lz/plain-01100000.bin
devenv shell -- bazel test //matching:compare-test
```

Inputs are owner-supplied. The x70view checkout must be at the manifest's revision with a clean `decomp/`; each translation unit, each image and each stock function body must match the manifest's SHA-256. `--out` must not exist. `--force-gcc 4.6.4` or `--force-gcc 4.4.7` compiles every function with one release instead of the source's annotation, as a compiler-discrimination control.

Compilation follows x70view's build for firmware `xm1-150`: its common EXR II flags, the source's first-line `/* flags: */` comment or `-O1 -fno-omit-frame-pointer`, a `/* compiler: */` release annotation (default GCC 4.6.4), the unit's include and `KERNEL_CORE` defines, and `-fdata-sections`. x70view generates per-unit `api.h` headers; the pilot supplies an empty `api.h`, and the report marks each translation unit that includes it. The compilers are `@lab_exr2_gcc` from [build-support/exr2-gcc.nix](../build-support/exr2-gcc.nix).

Linking places `.text.NAME` at the stock entry. Each relocation target resolves through x70view's `symbols/` address tables, or, for a section or local label of the same object, through its `native-bindings.json` section placement; the referenced object sections are placed at those addresses and everything else is discarded. A name with conflicting bound addresses is unresolved.

Each function is classed as `exact` (linked section equals stock and covers the whole stock body), `normalized` (differences lie only in relocated fields: 24-bit branch offsets, MOVW/MOVT immediates, absolute words) or `failure` (other differences, a matching section shorter than the stock body, an unresolved symbol, or a compile or link error). The output directory holds the objects, linker scripts, linked ELFs, emitted bytes and `report.json` with per-function flags, relocation targets, emitted, literal-pool and beyond-body byte counts, and dependency hashes. It derives from firmware and stays under `private-data/`.

## Translation-unit gate and patch check

`//matching:unit-gate` rebuilds the whole translation unit frozen in [unit-gate.json](unit-gate.json), x70view's `storage_command_dispatch.c`, which owns the image-4 media receive helper `0x0071fe2c`. Compilation and linking are the pilot's, with every owned function linked at its stock entry in one ELF. The gate passes only when the object emits exactly the owned functions, defines no allocated data, and every function is `exact`.

```sh
devenv shell -- bazel run //matching:unit-gate -- \
  --x70view /path/to/x70view --out private-data/matching-c/NEW_RUN \
  --image private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin \
  [--patch-source PATCHED.c --patch-function NAME --patched-image OUT.bin [--trap-image TRAP.bin]]
```

With the patch arguments it then compiles the patched copy of the unit with the same flags (plus the original directory on the quote path) and requires, in order: every other owned function still `exact`; the patched function no larger than its stock extent; and the patched decoded image differing from stock only inside that extent. It writes that image, and with `--trap-image` a stock image whose patched entry holds `UDF`, which shows whether a boot route reaches the function. Both are decoded image-4 files for `//lz:lz-encode 0x160000` and `//host/firmware:rebuild-update replace-record4`, then [`//emulation:flash-boot-check`](../emulation/README.md). Patched sources and every output stay under `private-data/`; these images are demonstrations and are never installed.
