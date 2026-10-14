# Latent Image

Host tools, executable models, and firmware research for the Fujifilm X-M1. Goals are improved autofocus and [named film recipes](spec/10-film-recipes.md). The repository also provides firmware analysis, bounded native emulation, and an [X-M1 recovery kit](recovery/README.md). Research tools are not a generally safe firmware installer or a demonstrated autofocus upgrade.

**Release status:** The owner authorizes source-only publication of this cleaned snapshot as **Latent Image** at `skeet70/latent-image`, without waiting for concurrent private work. The public history starts with a new root; later reviewed changes arrive as source-only imports, not private-history merges. The owner reports explicit permission from jitl (justjake) to redistribute the inherited x70view implementation with credit. That implementation retains its reported permission terms rather than being silently relicensed under AGPL. Unicorn-dependent combined programs require artifact-specific license compatibility review; a Nix dependency declaration does not distribute Unicorn itself. [Release decisions](LICENSING.md#release-decisions) records the scope and remaining obligations. Source commit, push and runtime verification are not established by this authorization.

## Start here

The supported host is Linux x86_64 with Nix and devenv. Bazel owns the build; the checked-in devenv environment supplies pinned dependencies. Initial setup downloads toolchains and dependencies. Shell activation, builds, tests, and the commands below perform no camera I/O.

```sh
devenv shell -- bazel build //...
devenv shell -- bazel test //...
devenv shell -- bazel run //af:simulate -- af/fixtures/peak-delayed.json
devenv shell -- bazel run //emulation:native-case -- --runner bazel-bin/emulation/native-runner emulation/fixtures/arithmetic.json
```

The simulation and native case use public synthetic fixtures. Neither demonstrates camera behavior. See [build integration](build-support/README.md) for toolchain and offline-cache details.

| If you want to… | Start with |
| --- | --- |
| Understand current goals, limits, and open work | [Principles](docs/principles.md), [task board](docs/task-board.md), [specification](spec/README.md) |
| Model autofocus and annotate observations | [AF simulation](af/README.md), [session annotation](reports/af-session-kit.md) |
| Inspect firmware formats and load behavior | [Firmware findings](reports/firmware-extraction.md), [container contract](reports/update-container-rebuild.md) |
| Understand recovered X-M1 interfaces | [AF map](reports/autofocus-static-map.md), [service and settings map](reports/service-query-and-jig-reset.md) |
| Execute synthetic ARM cases or study the emulator | [Emulation](emulation/README.md), [full-system findings](reports/full-system-emulation.md) |
| Investigate recovery or prepare a supported card | [Recovery kit and limits](recovery/README.md) |
| Study film recipes and editable banks | [Recipe specification](spec/10-film-recipes.md), [bank design](reports/recipe-design.md) |
| Plan bounded camera reads | [Transport](transport/README.md), [read plans](readplan/README.md), [camera-run procedure](spec/07-governance-and-handoff.md#3-camera-runs) |

## Repository map

| Area | Responsibility |
| --- | --- |
| `host/`, `lz/`, `recovery/` | Firmware formats, transformations, compression, recovery preparation |
| `af/`, `af-session/`, `broker/` | Decision models, observations, action ownership |
| `transport/`, `readplan/` | Explicit live operations and bounded read planning |
| `console-trace/` | Host-only log decoding and comparison with [format findings](reports/console-trace-instrument.md) |
| `emulation/`, `menu/` | Execution models, upstream emulator integration, recipe target code |
| `schemas/`, `manifests/`, `fixtures/` | Data contracts, pinned public metadata, fixture provenance |
| `docs/`, `spec/`, `reports/` | Current decisions/evidence/work, requirements, technical findings |
| `third_party/`, `build-support/` | Upstream attribution and pinned build integration |

Package READMEs own supported commands. The specification owns research requirements, the task board owns open work, and the evidence ledger records scoped claims. Existing C host models and retained Python emulator responsibilities are migration debt; new project tooling uses Haskell/Rust, with C limited to necessary engine or ABI boundaries.

## Public knowledge, private inputs

**Publish tools, implementations, and distilled findings; don’t redistribute proprietary source material.** Findings include firmware-version-bound addresses, recovered interfaces, control flow, protocol bytes, and executable models. Firmware derivation alone does not make knowledge private. [Publication policy](docs/principles.md#publication-and-asset-boundaries) defines the boundary.

Firmware, extracted proprietary code/data, dumps, calibration, private identities, raw camera logs, and local artifacts stay under ignored `private-data/`. Firmware-backed commands require independently obtained inputs and their documented identity checks. A clean public checkout supports synthetic host work without those inputs; firmware-backed reproduction has additional prerequisites described by each tool.

## Camera safety

Camera access exists in explicit live commands under `//transport:camera`. The camera owner runs those commands after approving the named operation. State-changing bytes require independent review before their first live run; persistent writes require operation-specific approval under the [camera-run procedure](spec/07-governance-and-handoff.md#3-camera-runs). A passing emulator case does not establish safe installation, recovery, or functional acceptance on hardware. Addresses and behavior are X-M1 firmware-version-specific, not transferable from sibling models.

## License and upstream work

Original software is licensed under [AGPL-3.0-only](LICENSE), which permits commercial use and imposes source-sharing obligations, including for modified versions offered over a network. Upstream components retain their own licenses. See [licensing and release obligations](LICENSING.md) and [pinned sources and attribution](third_party/README.md) before redistributing the complete repository.
