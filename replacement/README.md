# Replacement firmware packages

Owned X-M1 firmware code and its host tooling use the repository's Bazel/Nix build. Start with the workflow for the subsystem you are changing.

| Workflow | Packages and commands |
| --- | --- |
| [Startup and memory](startup.md) | Early stages, loaders, composed startup, heap staging, task bodies and startup-consumer checkers |
| [Settings and record storage](settings-storage.md) | Property service staging/qualification, record-store carrier, native oracle and generated counterexamples |
| [Menus and RAM packages](menus.md) | Bank publication groups, menu-engine inputs, packaging and pair comparisons |
| [RTOS platform](rtos.md) | Shared layouts, family build/oracle entry points, scheduler staging, generation/replay and timer composition |

## Build and inputs

Run commands from the repository root through [host-build](../build-support/README.md#commands):

```sh
devenv shell -- host-build build //replacement/abi:pilot-image
devenv shell -- host-build test //replacement/abi:pilot-abi-tests
```

The [Rust toolchain guide](toolchain/README.md) owns compiler provenance and build macros; the [ABI guide](abi/README.md) owns the bounded ARM qualification. Existing package guides cover the [record validator](validator/README.md), [retained row-availability service](service/README.md) and [render adapter](render_adapter/README.md).

Stagers require their declared canonical X-M1 1.50 inputs, not an arbitrary previously staged candidate. Firmware and generated artifacts remain in ignored `private-data/`; use a named `private-data/scratch/` directory or `private-data/verify-candidate/runs/` for new outputs. Individual commands specify whether directories must already exist or be fresh. Guest execution uses the shared [emulator slot](../build-support/README.md#commands), normally `XM1_EMULATOR_SLOTS=1`; file-only admission and comparison do not execute firmware.

## Authoritative references

| Question | Home |
| --- | --- |
| Replacement requirements, ownership stages and qualification process | [Replacement specification](../spec/11-incremental-firmware-replacement.md) |
| Current subsystem boundaries and retained dependencies | [Subsystem map](../reports/subsystem-replacement-map.md), [ownership inventory](../reports/replacement-inventory-baseline.md#8-current-ownership-and-retirement-view) |
| Open work and acceptance status | [Replacement epic #117](https://github.com/skeet70/latent-image/issues/117), [retirement backlog #127](https://github.com/skeet70/latent-image/issues/127) |
| Scoped findings and artifact identities | [Evidence ledger](../docs/evidence.jsonl), [composed-contract qualifications](startup_artifact/qualifications.json) |
| Full-system observer commands | [Emulation guide](../emulation/README.md) |
| Camera operations | [Camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs) |

These guides document host workflows, not camera authorization. Build, native, finite system and hardware qualification are distinct evidence scopes. Keep findings in their technical reports, claims in the ledger and open work in issues rather than appending progress reports here.
