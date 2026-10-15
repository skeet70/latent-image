# Pinned sources and inherited terms

[LICENSING.md](../LICENSING.md) owns the project grant, attribution, dependency obligations and [release decisions](../LICENSING.md#release-decisions). Original software is `AGPL-3.0-only`; inherited material retains its own terms. jitl (justjake) and x70view receive primary credit for the inherited emulator. jitl explicitly permitted redistribution on 2026-10-14 provided he is credited; this is not an author-provided AGPL or other SPDX license.

[The source manifest](../manifests/sources.json) records exact commits and scoped evidence. Inspect imports and build scripts before execution; a pin or inventory is not a completed audit. Keep upstream notices and local modifications explicit. Acquired checkouts, firmware, ROM, captures and runtime outputs remain outside source control except individually recorded scoped fixture dispositions.

| Record | Purpose |
| --- | --- |
| [Dependency license declarations](dependency-licenses.json) | All locked Bazel modules and native Cargo registry packages, plus selected locked Nix package declarations, with primary evidence and audit limits |
| [Retained license texts](licenses/) | rules_haskell Apache-2.0 terms and the fffw MIT copyright/permission notice; not replacement terms for other upstream material |
| [Transport/protocol and native effects](../reports/source-and-transport-audit.md) | ff80 source digests, [selected libusb/libudev inventory](native-usb-source-inventory.json), reviewed kernel commit/profile |
| [Firmware/export/recovery](../reports/firmware-extraction.md) | [Selected exact firmware/boot/patcher source metadata](firmware-source-inventory.json), without firmware bytes |

Whole-tree fffw/python-libusb1 inventories are reproducible from the manifest's commits with `//third_party:source-inventory`; [tool arguments](../reports/review/README.md) describe regeneration. The tool reads Git objects without importing or executing upstream code. These metadata inventories are not permission to redistribute a source archive or proprietary inputs.

Only the exact inspected patcher header/complement path is executed through the bounded exporter. CLI/assembler/updater/camera paths are excluded. Fujihack's sibling examples supply no X-M1 port or map. Reference-only algorithm inputs still need commit selection and a scoped audit before source reuse.

## Retained x70view closure

`emulation/xm1-upstream/` preserves a selected runtime closure from jitl (justjake)'s [x70view](https://github.com/justjake/x70view) commit `40406daf52228ace99cb1a3e322f3193f96b0283`, with X-M1 adaptations. Its native package retains upstream names. The [reference assessment](../reports/x70-emulator-reference.md#12-the-authors-grant) preserves the initial 2026-10-09 personal permission and the 2026-10-14 explicit redistribution permission conditioned on credit. The [emulation report](../reports/full-system-emulation.md#119-interactive-x-m1-application-emulation) records target execution scope. The public upstream pin remains unavailable. No author-provided SPDX license, sublicensing authority or downstream modification terms were supplied; do not relabel this inherited implementation AGPL. These scope questions do not negate the redistribution grant.

The owner chose on 2026-10-14 to retain the exact sixteen-byte remembered MCU packet in `emulation/xm1-upstream/emulator/captured_mcu_profile.py` publicly as a scoped observed fixture. It is not synthetic data, and this disposition is not permission to redistribute other Fujifilm firmware or captures.

Unicorn implementation code is not vendored, but the bounded runner links its engine and the retained native viewer loads it in-process. The locked Nix engine is Unicorn 2.1.4, declared `GPL-2.0-only`; header or binding terms do not relicense the engine. Assess compatibility for actual combinations, distributed linked/load-in-process binaries, bundled Nix closures and deployed services. A flake reference alone neither redistributes the engine nor automatically clears or blocks source-only publication. PyO3, libloading and their transitive Cargo terms are recorded in the dependency metadata; preserve their notices when conveying dependency code or linked artifacts.

The optional x70 Unicorn acceleration source is pinned by the upstream `native/unicorn` gitlink to `9e42f34006b10ad67f3cfed7cc5c7ab2209a53b3` at `https://github.com/justjake/unicorn.git`. The upstream native README identifies that fork as private and based on Unicorn 2.1.4. Its build script selects ARM-only, CMake Release, shared libraries and disabled tests. The exact source and patch notices are absent from the acquired closure. Packaging requires an accessible immutable archive or exact auditable patches and applicable redistribution rights. The viewer remains on stock Nix Unicorn; no private binary is retained.
