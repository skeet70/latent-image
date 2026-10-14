# Manifests

`sources.json` records exact pins and current evidence IDs for the selected [transport/native audit](../reports/source-and-transport-audit.md) and [firmware/recovery findings](../reports/firmware-extraction.md). `audited` refers to those paths, not every upstream file or camera behavior.

`camera.json`, `firmware.json` and `memory-map.json` are partial records, not a current body inventory. Null fields remain unknown, and `firmware.json` retains the original installed-1.00 baseline rather than tracking later installations. The owner-supplied official 1.50 update has exact provenance; it is not bundled firmware, installed-byte correspondence or a complete backup claim. Current observations and their scope belong to [the evidence ledger](../docs/evidence.jsonl) and [the task board](../docs/task-board.md).

`benchmark-protocol.json` is an offline design draft with no results; `E01-stock-pilot-v1-order.txt` is its checked-in 110-attempt visit order, regenerable by the shell pipeline in [the session kit](../reports/af-session-kit.md#6-attempt-order) and fixed by SHA-256 `05d509d2c1c8ff12e6b312b90e1895a6708aeb080f52fc1ac9c4af10e4470f9b`.

Use private aliases publicly and keep serials/raw captures private. See [artifact/runtime contracts](../spec/08-artifact-and-runtime-contracts.md).
