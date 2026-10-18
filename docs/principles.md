# Current project principles

These are the active decisions. The specification owns the research requirements, package READMEs own supported commands, [GitHub Issues](https://github.com/skeet70/latent-image/issues) owns open work, and reports retain findings needed for the next experiment. Superseded decisions are recoverable from Git.

## Scope and evidence

The target is Fujifilm X-M1. The recorder rev-2 failed-boot incident and recovery-adjacent checks are complete: exact v5 physical rollback reports successful writer return and complete RC0 readback at unchanged block 14, followed by owner-reported ordinary startup after power removal, photography, normal-card operation/playback and retained film simulations/settings, including C-slot color +2 (E-RECOVERY-FRAGMENTED-V5-STATUS20-150, E-RECOVERY-V5-NORMAL-CARD-PLAYBACK-150, E-RECOVERY-V5-RETENTION-150). Detailed lens/AF and new recipe acceptance remain separate feature work. Current open work belongs in [GitHub Issues](https://github.com/skeet70/latent-image/issues); durable recovery results are in [the recovery report](../reports/sd-boot-card-entry.md#260-physical-rollback-readback-status-20). The last fully observed pre-incident installed state was the RC0 prefix container with seven film-simulation entries and retained settings after its 2026-10-09 install (E-FILMSIM-INSTALL-150). Its 1.00 calibration area is captured and recorded in two verified locations (E-S03-FULL-CAPTURE, E-S03-DURABLE-PRESERVATION), and the official 1.50 `FWUP0002.DAT` is retained (SHA-256 `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`). Those backups are not a full physical-media image or a demonstrated restore.

Captured primary-kernel startup reproduces the v2 refusal by overwriting its source and recorder reference (E-RECOVERY-REFERENCE-LIFETIME-150). Further idle-IRQ and log-initializer execution rejects v3/v4 placements (E-RECOVERY-IDLE-LOG-LIFETIME-150). The [reviewed two-buffer v5 candidate](../reports/sd-boot-card-entry.md#259-two-buffer-physical-recovery-candidate) preserves the complete RC0 reference in three checked native-stage holes and reaches exact modeled RC0 readback after captured startup/log/IRQ paths. Its authorized body run establishes physical rollback/readback; the subsequent owner-reported ordinary no-card startup supports persistence through the reported power cycle, not full functional acceptance (E-RECOVERY-FRAGMENTED-V5-STATUS20-150, E-RECOVERY-V5-COLD-START-SUCCESS-150).

Camera runs follow [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs). It protects against the risks that could plausibly damage this body: sending wrong bytes, writing persistent state unintentionally, and losing the body-specific calibration. Those are covered by fixed-byte operations in code, one review of new write-capable bytes, the owner's go, and the existing backup.

Evidence labels distinguish reported leads, source inspection, static inference, emulation, hardware observation and independent reproduction. Keep target-specific claims bound to the firmware version and input they came from. Never fill X-M1 CPU, address, ABI or behavior gaps from sibling examples.

The X-M1 core is inferred to be Cortex-R4F-class ([E-CPU-PROFILE](../reports/firmware-extraction.md#cpu-profile)); the native runner emulates Unicorn's Cortex-R5F, the closest available model. No fixture, stock case, payload or expected result may depend on behavior an R4F lacks or reports differently: A32 SDIV/UDIV, multiprocessing extensions and MPIDR, R5F ID-register values, a 16-region MPU, or R5 TCM/cache register semantics. The runner rejects each such feature it can reach; admitting one requires X-M1 evidence first.

## Implementation and build

The project structure is Haskell/Rust. Use Haskell for host tooling and orchestration; use Haskell/Rust for validation, models, simulation, fixtures, tests and native decision/execution code. C is limited to technically necessary engine or ABI boundaries, not a general-purpose native-helper exception. Existing C host models and retained Python emulator responsibilities need migration or an explicit necessity argument. Supported upstream tools keep their languages; pinned upstream build helpers and check-jsonschema may use Python. Add no project-authored Python.

Bazel owns the build/test graph; devenv supplies pinned tools. Both use the same locked Nix source with overlays disabled. Declare dependencies and upgrade compatible rules/toolchains together. Do not add parallel Cabal, Stack or CMake builds. The Linux host platform does not establish a target ABI. See [build integration](../build-support/README.md).

Camera code exposes only closed operations whose bytes are built internally. It sends no retries, stops on any unexpected reply and journals outgoing bytes before sending them. Help, builds and tests never touch USB.

## Collaboration and records

Contribute host tooling, firmware analysis, emulation, AF experiments or documentation through [GitHub Issues](https://github.com/skeet70/latent-image/issues). Independent review is required for new camera bytes that change camera state (H2/H3 in [the operation classes](../spec/07-governance-and-handoff.md#32-operation-classes)); the author fixes findings and the reviewer confirms. Assistant-specific instructions are in [AGENTS.md](../AGENTS.md).

Development loop: concurrent agents work in separate worktrees and Bazel output bases ([build integration](../build-support/README.md)) and leave their changes uncommitted there; one integration owner commits and merges them. A full-system candidate is checked in the emulator, including the owner's visual check, before independent review. Emulator runs are serialized by `~/.cache/latent-image-emulator.lock`, run under a memory cap, and start from a saved portable checkpoint where one exists; a new harness step is tried on a short checkpointed run before a full one. Assignments are sized to finish within one agent's request budget.

Maintain current conclusions, not a diary. Keep one authoritative home for each rule or design and link to it. Update GitHub Issues for changed open work and the evidence ledger for changed durable claims. Commits and handoffs carry ordinary checks.

Historical evidence-ledger paths and hashes identify the recorded artifact versions and private provenance aliases, not necessarily current files or distributed inputs. Previously tracked public files can be consulted in this repository's Git history; files from the private research archive are not provided here.

Private body-specific artifacts stay under ignored `private-data/`. Body-specific backups need two verified durable locations.

## Publication and asset boundaries

Publish tools, implementations, and distilled findings; don’t redistribute proprietary source material.

“Distilled findings” includes exact firmware-version-bound addresses, recovered interfaces and layouts, detailed control-flow findings, protocol bytes, algorithms, and executable models. Derivation from firmware is not, by itself, grounds for keeping a finding private. Keep proprietary inputs separate while publishing the knowledge needed to understand, reproduce, and extend the work.

Original implementations, technical maps, synthetic fixtures with provenance, and reusable research methods belong in the public tree. Firmware copies, extracted proprietary code/data, dumps, calibration, private identities, raw camera logs, irrelevant test shots, and local development archaeology stay private. Review copied instruction sequences, tables, disassembly excerpts, and captured fixtures individually for redistribution rights; neither public availability nor a “finding” label establishes those rights.

Reproduction uses documented owner-supplied inputs, identity checks, and extraction commands instead of bundled proprietary inputs. Publish evidence scope, uncertainty, and current safety constraints with the findings. Distill historical narratives into present-tense knowledge; preserve raw artifacts privately.

Original Latent Image software uses AGPL-3.0-only; commercial use is allowed. Preserve upstream licenses, notices and the permission to redistribute jitl (justjake)'s x70view work with credit, without inferring an AGPL relicensing grant. The [public history boundary](../LICENSING.md#release-decisions) keeps the full research history private and the public repository separate. Later reviewed changes enter as source-only imports, never merges of private-history branches. Review each import's privacy, provenance and attribution; review actual combined artifacts and dependency closures before distributing them. Source publication does not establish runtime verification or camera clearance.
