# X-M1 research specification and documentation map

The target is Fujifilm X-M1, using version-bound firmware findings and a Linux host. The current lens is a Viltrox AF 28mm F4.5 on firmware 1.05. Host tooling uses Haskell; project models and execution code use Haskell/Rust, with C limited to necessary engine or ABI boundaries. Bazel owns the build graph and devenv supplies pinned tools.

## Authoritative homes

| Question | Start here |
| --- | --- |
| Current policy, language/build choices and public/private asset boundary | [Current principles](../docs/principles.md) |
| Open work, feature priority and open gates | [GitHub Issues](https://github.com/skeet70/latent-image/issues) |
| Research requirements and safety acceptance criteria | Numbered specification sections below |
| Supported host and camera commands | Package READMEs linked from [the repository overview](../README.md) |
| Camera authority, review and stop rules | [Camera-run procedure](07-governance-and-handoff.md#3-camera-runs) |
| Scoped claims and artifact identities | [Evidence ledger](../docs/evidence.jsonl) |
| Redistribution rights and release blockers | [Licensing review](../LICENSING.md) |

Reports retain technical findings, interfaces, bounded experiments and unresolved assumptions. Superseded instructions and ordinary execution history belong in Git, not a second policy or task list. Firmware, dumps, raw camera logs, calibration and other proprietary or body-specific inputs remain under ignored `private-data/`; their absence does not make the recovered technical knowledge private.

## Specification

1. [Mission, evidence and source baseline](01-mission-and-evidence.md)
2. [Project structure, environment and shared contracts](02-project-and-environment.md)
3. [Execution phases, gates and dependencies](03-phases-and-gates.md)
4. [Recovery research](04-recovery.md)
5. [Autofocus research and implementation](05-autofocus.md)
6. [Benchmark and acceptance protocol](06-benchmarks.md)
7. [Governance, camera runs and handoff](07-governance-and-handoff.md)
8. [Artifact and runtime contracts](08-artifact-and-runtime-contracts.md)
9. [Sources and starting references](09-sources.md)
10. [Film recipes](10-film-recipes.md)

Recommended AF sequence: ordinary-boot stock trace and coherent observations, a bounded transient adapter, short automated household-scene batches, recorded/calibrated host experiments, then measured on-camera speed and accuracy with untethered validation. Cardless research can proceed before SD repair; recovery availability and ordinary save/playback acceptance remain separate. [Section 11.0](05-autofocus.md#110-stock-first-sensor-backed-iteration) owns this process. Host work can proceed around dependencies without inventing target facts. Original design v1.0 provenance is SHA-256 `62d08fb6dc2b0362139a595a8260a73c7bc753085598e1b64f192c5c17115efe` (2026-09-28).

## Technical findings and reproduction

| Area | Technical home | Commands and follow-on work |
| --- | --- | --- |
| Firmware structure, CPU limits and installed-byte correspondence | [Firmware extraction](../reports/firmware-extraction.md), [update validation and boot](../reports/update-validation-and-boot.md), [container rebuild](../reports/update-container-rebuild.md) | [Host review tools](../reports/review/README.md), [LZ decoder/encoder targets](../lz/BUILD.bazel), [recovery kit](../recovery/README.md) |
| ROM, boot selectors and physical recovery | [Boot-ROM window](../reports/bootrom-window.md), [card-entry and rollback findings](../reports/sd-boot-card-entry.md), [coverage and preservation gaps](../reports/recovery-status.md) | [Recovery kit](../recovery/README.md), [R3/R4 requirements](../reports/recovery-r3-r4-plan.md) |
| Service transport and configuration reads | [Source and transport audit](../reports/source-and-transport-audit.md), [adjustment and service interfaces](../reports/adjustment-and-service-interfaces.md), [service/query and jig reset](../reports/service-query-and-jig-reset.md), [End Setting protocol](../reports/end-setting-live-protocol.md) | [Transport commands](../transport/README.md), [bounded read planning](../readplan/README.md) |
| Emulation and execution boundaries | [Full-system report](../reports/full-system-emulation.md), [X70 reference scope and terms](../reports/x70-emulator-reference.md) | [Emulation commands and fixtures](../emulation/README.md) |
| Autofocus control flow and observation | [1.50 static map](../reports/autofocus-static-map.md), [observation instruments](../reports/af-observe-instruments-150.md), [observation plan](../reports/af-observation-plan.md), [candidate reproducibility](../reports/af-candidates.md) | [AF model and candidate comparison](../af/README.md), [stock pilot protocol](../reports/stock-pilot-protocol.md), [session kit](../reports/af-session-kit.md) |
| Film simulation and recipes | [Film-simulation findings](../reports/film-simulation.md), [settings map](../reports/custom-settings.md), [recipe design](../reports/recipe-design.md), [D3 patch](../reports/recipe-patch-d3.md) | [Compiled recipe builder](../reports/compiled-recipes-builder.md), [installation constraints](../reports/install-procedure.md) |
| Validation scope and synthetic provenance | [Validation report](../reports/validation.md) | [Schemas](../schemas/README.md), [fixture policy](../fixtures/README.md), [build integration](../build-support/README.md) |

The recorder rev-2 failed boot is repaired and recovery-adjacent checks are complete. The [exact v5 physical result](../reports/sd-boot-card-entry.md#260-physical-rollback-readback-status-20) owns that bounded claim; [recovery status](../reports/recovery-status.md) records the formal gate distinctions. Detailed lens/AF and new-recipe acceptance remain separate. Historical session scripts are not current authorization or evidence that a feature works.
