# Host tools and review probes

Run through the checked-in devenv/Bazel environment. These utilities inspect pinned public source, private firmware or synthetic fixtures; none enumerates/opens a camera or establishes target CPU/map/ABI, restoration or gate acceptance. Use absolute caller input paths because `bazel run` changes the working directory.

| Purpose | Target and arguments after `devenv shell -- bazel run TARGET --` |
| --- | --- |
| Exact public Git inventory | `//third_party:source-inventory CHECKOUT REVISION URL --output NEW_OUTPUT` |
| Bounded export checks | `//host/firmware:check-export --repo REPOSITORY [PATCHER_CHECKOUT EXACT_PRIVATE_DAT]` |
| Exact supported DAT framing metadata | `//host/firmware:framing-probe EXACT_PRIVATE_DAT` |
| X-M1 mapped-section export for the X70 emulator scratch run | `//host/firmware:export-x70-map [--expected-sha256 SHA256] DAT DECODE_ID4 DECODE_ID5 ROM FFLZD SCRATCH_SOURCE` |
| Firmware-source inventory/checksum verification | `//reports/review:f01-source-check --fffw CHECKOUT --fujihack CHECKOUT --patcher CHECKOUT [--dat EXACT_PRIVATE_DAT] --repository REPOSITORY` |
| Exporter FIFO boundary probe | `//reports/review:f01-firmware-probes --repository REPOSITORY` |
| Static USB dependency metadata | `//reports/review:f02-native-metadata` (`pkg-config`, hashes and `readelf`; no library load) |

```sh
devenv shell -- bazel test //reports/review:all //third_party:inventory-tests //host/firmware:framing-tests
```

Default tests use only synthetic data. Omit both optional export arguments for eight synthetic guards; supply both for twelve exact-input checks. Export checks remove temporary private derivatives. Framing emits metadata only and accepts only the exact DAT in [firmware findings](../firmware-extraction.md).

`export-x70-map` is host-only and exports external-emulator inputs, not an installable firmware image. Without `--expected-sha256`, `DAT` must hash to the pinned stock X-M1 1.50 container (`43d67afa…`) and the pinned stock checks run. The option accepts an exact caller-pinned X-M1 candidate digest and runs the structural and carried-field checks without requiring stock checksum equality. `ROM` must be this body's captured 16 KiB block (`4ecedfbf…`); both decoded artifacts are accepted only by whole-byte equality with the supplied `//lz:fflzd` decode of the DAT's own records. `SCRATCH_SOURCE` must resolve below ignored `private-data/`, the tool refuses an existing `firmware/mapped-xm1` there, and every exported binary, manifest, DAT and ROM copy stays under that scratch root. Its consumer and full argument contract are in [the reference assessment](../x70-emulator-reference.md#57-prepared-x-m1-scratch-run).

Git inventories read objects without importing upstream code. Generation/selected-source verification have 60-second whole-call bounds, 10-second/64-MiB Git output caps, finite tree/blob counts and 15-second hash helpers. Original source languages and notices remain intact. Inventory JSON is canonical metadata; serialized whitespace is not a research claim and need not be checked in for every whole tree.

Review tests retain independent filesystem/output alias, native writeback, schema/descriptor, lease/owner-death, private snapshot and strict JSON/process lifecycle checks. [Validation](../validation.md) describes their contracts and limitations. Historical filename prefixes identify stable test targets; the old per-review narratives and hash lists are in Git.
