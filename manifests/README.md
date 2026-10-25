# Manifests

`sources.json` records exact pins and current evidence IDs for the selected [transport/native audit](../reports/source-and-transport-audit.md) and [firmware/recovery findings](../reports/firmware-extraction.md). `audited` refers to those paths, not every upstream file or camera behavior.

`camera.json`, `firmware.json` and `memory-map.json` are partial records, not a current body inventory. Null fields remain unknown, and `firmware.json` retains the original installed-1.00 baseline rather than tracking later installations. The owner-supplied official 1.50 update has exact provenance; it is not bundled firmware, installed-byte correspondence or a complete backup claim. Current observations and their scope belong to [the evidence ledger](../docs/evidence.jsonl) and the linked technical reports.

The X-T1 5.52 update in `firmware.json` is a sibling reference input for Classic Chrome ([spec/10 section 19.5.3](../spec/10-film-recipes.md#1953-inputs-and-rights)); it is read on the host only, so its `matches_installed` is null.

`benchmark-protocol.json` is an offline stock-pilot draft with no results or camera authorization. `stock-pilot-order.txt` contains its 22-attempt development example, generated from the manifest's seed, repeat count and SHA-256 ordering rule. The [pilot protocol](../reports/stock-pilot-protocol.md) separates a smaller feasibility check, this automatic pilot and a separately powered confirmation; the public example is not a mandatory owner workload.

Use private aliases publicly and keep serials/raw captures private. See [artifact/runtime contracts](../spec/08-artifact-and-runtime-contracts.md).
