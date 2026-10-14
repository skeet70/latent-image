# Project structure, environment, and shared contracts

## 5. Repository and evidence layout

This repository is the `fuji-af` research platform. Package READMEs own supported commands; [the task board](../docs/task-board.md) owns implementation work and gate decisions.

| Path | Contents and owner | Current state |
| --- | --- | --- |
| `AGENTS.md` | Scope, camera-run rules, evidence rules | Present |
| `devenv.nix`, `devenv.yaml`, `devenv.lock` | Pinned host environment; A | Present; host checks only |
| `MODULE.bazel`, `MODULE.bazel.lock`, `.bazelversion`, `.bazelrc` | Pinned build rules and shared build settings; A | Bazel build/test graph; preserved Rust engine built with locked Nix |
| `build-support/`, package `BUILD.bazel` files | Nix-backed host toolchains and declared targets; A | GHC/Rust/GCC/Unicorn and validation tools use the locked Nixpkgs input |
| `docs/task-board.md`, `docs/principles.md` | Open work and current decisions | Current work, gates and rules |
| `docs/evidence.jsonl` | Scoped evidence ledger | Compact durable claims with evidence labels |
| `manifests/`, `schemas/` | Versioned identities, maps, experiments and evidence/trial contracts; coordinator | Public provenance and offline contracts; unknown fields stay explicit |
| `broker/FujiAF/` | Protocol profile and packet types shared with `transport/` | Haskell library |
| `transport/` | `//transport:camera`: closed camera operations and the narrow native USB provider | Host-tested Haskell/C; live commands run per [the camera-run procedure](07-governance-and-handoff.md#31-procedure) |
| `readplan/` | Offline read-plan validation and stable-capture comparison | Haskell host commands; [supported contracts](../readplan/README.md) |
| `host/firmware/`, `host/FujiAF/`, `lz/` | Container inspection, extraction, codecs, patch/rebuild and recipe tools; B | Haskell tooling with audited upstream export and existing native boundaries; [firmware findings](../reports/firmware-extraction.md) |
| `emulation/` | Bounded native execution and full-system viewer; D | Haskell orchestration, existing C Unicorn harness and pinned upstream Rust/Python components; [supported scope](../emulation/README.md) |
| `af/`, `af-session/` | Controller simulation and session preparation/analysis; E | Haskell host orchestration and existing C decision prototype; no calibrated lens or target integration |
| `payload/` | Target hook and integration requirements; B/E | Target-specific entry, ABI, lifetime and synchronization need evidence |
| `recovery/` | Host firmware triage, scoped image generation and card preparation; C | [Public kit](../recovery/README.md); failed-boot incident and adjacent checks complete; board records R3 demonstrated 2026-10-13 and R4 unclaimed |
| `tests/` and package test targets | Focused format, protocol, emulator, and AF checks | Synthetic host suite implemented; [validation scope](../reports/validation.md); hardware opt-in |
| `fixtures/` | Synthetic or redistributable fixture policy | Public synthetic cases under `emulation/fixtures/` and `af/fixtures/`; captured proprietary inputs remain private |
| `reports/` | Firmware-version-bound findings, procedures and evidence scope | Static, emulated and camera-observed results with their limitations |
| `third_party/` | Selected source inventories and notices | Firmware/native audit provenance retained; whole Git inventories regenerable; acquired checkouts outside source control |
| `private-data/` | Ignored owner-supplied firmware, dumps, serials, session outputs and captures | Private inputs and body-specific backups, not distributed with the source |

Keep expensive or irreproducible camera data in two durable copies, with checksums. The Git history must retain code, schemas, manifests, and provenance without requiring camera dumps to be published. A fresh agent should be able to resume from `docs/task-board.md`.

## 6. Reproducible host environment with devenv

### Implementation policy

Follow [current principles](../docs/principles.md#implementation-and-build) for implementation languages, build ownership and upstream exceptions.

### 6.1 Environment maintenance

1. Record host OS/architecture, Nix version, `devenv` version, available disk, and USB access policy.
2. Maintain the checked-in `devenv` inputs and `devenv.lock`. Pin Bazel and its rule modules with `.bazelversion`, `MODULE.bazel`, and `MODULE.bazel.lock`.
3. Pin upstream repositories by commit, not only by branch or tag.
4. Add dependencies declaratively; do not install tools globally.
5. Keep shell activation and ordinary tests free of camera I/O. Connecting a USB camera must not cause a build, `--help`, or import to operate it.
6. Inspect upstream build scripts before executing them. Build failure in incomplete `ffun` modules is a dependency report, not a reason to replace them with guessed firmware parsing.

### 6.2 Host toolchain and optional research dependencies

The real [devenv configuration](../devenv.nix) supplies Bazel, Haskell, the Nix-built Rust emulator extension, C build tools for existing boundaries, Unicorn, libusb, Node for the audited upstream firmware exporter and host utilities. Bazel owns the project build/test graph, as recorded in [current principles](../docs/principles.md#implementation-and-build). Cabal, Stack and HLS are disabled in the baseline. The documented shell interface is `devenv shell -- COMMAND`. [S09](09-sources.md#s09) Reproduction commands and observed versions are in the [validation guide](../reports/validation.md) and scoped research reports.

The [build integration](../build-support/README.md) imports toolchains, Unicorn and validation tools from the same locked Nixpkgs source with user overlays disabled. Build rules are pinned separately by Bzlmod. Project-owned native emulation belongs in Rust; existing C harnesses require a necessity review or migration. Keep engine/ABI boundaries narrow and expose a stable file/stdio or typed FFI contract to host orchestration. Declare native libraries as build dependencies and verify runtime discovery in sandboxed tests. Future target firmware builds require their own evidenced cross toolchain; the current Linux x86_64 host platform does not specify a camera ABI.

Haskell packages and their dependencies are declared in the Bazel graph and supplied by the shared locked Nix package set. Before adding packages, select the pinned package set or the Haskell rules' supported locked dependency mechanism and record versions and hashes. Initial dependency acquisition may use the network; cached host tests must require no device or firmware input. Shell activation must not download firmware, execute source programs, or operate devices. Private local devenv configuration can change the shell despite a lockfile; record it or reproduce without it for G0.

| Need | Implementation or dependency | Verification requirement |
| --- | --- | --- |
| Service transport | Haskell transport with a narrow native libusb provider | Compile and fake-transport checks with no device connected. |
| Formats and tests | Haskell test harness and focused packages for serialization/ELF as needed | Lock versions; ensure parser checks require no real camera or firmware download. |
| Disassembly | Locked LLVM and Ghidra tools | Verify mapped/headless analysis against exact input identities. |
| Native emulation | Rust engine calling Unicorn's C ABI; existing C harnesses require necessity review | Run bounded instruction fixtures, then stock firmware fixtures. |
| Analysis | Haskell host tooling; add specialist image/plot tools only for concrete experiments | CPU-only path first; deterministic recorded inputs. |
| Target payload | Pinned cross-target Clang/LLD and explicit target build rules | Validate the port's architecture/ABI, float, linker and cache handling against X-M1 evidence; host defaults are insufficient. |
| Public `ffun` subset | Optional Go compatible with its pinned module plus declared dependencies | Not yet provisioned; inventory missing packages/manifests before choosing the build environment. |

Add heavy ML or full-system emulation dependencies only when a concrete experiment needs them. Do not enable a broad privileged USB container as a shortcut. On non-Linux hosts, keep offline tasks portable and create a separate documented hardware-access route.

### 6.3 Supported command entry points

Use package targets through `devenv shell -- bazel run TARGET -- ARGS`. [The repository README](../README.md) links the supported host tools; package READMEs document arguments and input requirements. Closed camera operations live in `//transport:camera` ([its README](../transport/README.md)) and follow [the camera-run procedure](07-governance-and-handoff.md#31-procedure). Host commands, help and version output never access a camera.

## 7. Shared contracts

### 7.1 Identity and address manifests

Every firmware artifact and payload must carry:

- Exact camera model, firmware version, official input SHA-256, and input URL/date.
- Upstream tool commit, local code commit, `devenv.lock` and `MODULE.bazel.lock` hashes, and effective build configuration/flags.
- Observed USB identity plus the expected camera-specific identity; full serial stays private.
- Lens model, firmware, and relevant settings when testing AF.
- CPU family/mode/endianness as observed or still unknown; never infer the X-M1's complete memory map from X-E2.
- An address table where every region includes start, length, classification, provenance, and confidence. Classifications include code, writable RAM, ROM, device registers, internal storage window, and unknown.
- For a payload, expected original bytes at every patch site, replacement bytes, scratch-space ownership, entry/return contract, and the exact allowed write set.

A new firmware version requires a new port review. Units, clocks and payload contracts are defined in [artifact/runtime contracts](08-artifact-and-runtime-contracts.md).

### 7.2 Camera operations

Each camera operation is a closed command in `//transport:camera` whose README states its purpose, exact transfer sequence, expected replies, possible persistent effects and stop conditions. The command journals the packets actually sent and received with timestamps. Do not log only the intended command; reconstructing what the camera actually received matters after a failure.

### 7.3 AF observation and action interface

Define a small host/on-camera contract before writing competing controllers:

```text
Observation:
  sequence_id, acquisition_timestamp, delivery_timestamp
  focus_command_at_acquisition
  lens_position_reported (optional), movement_complete (optional)
  sharpness_value, region_id, measurement_valid
  exposure_time, gain_or_iso, aperture, available noise indicators
  stock_af_state (optional), cancellation_or_mode_change

Action:
  hold | move_to | move_by | finish | fail | hand_back_to_stock
  target_or_delta, bounded_speed_if_supported
  reason, confidence_if_calibrated, expected_next_observation
```

Unknown or unavailable fields stay explicitly missing. A commanded focus position is not measured lens position. Match every sharpness sample to the lens state during sensor acquisition, accounting for queued frames and image-processing delay. Use consistent physical/actuator units or documented conversions; do not label raw motor codes as meters.

### 7.4 Evidence labels

Use `reported`, `source-inspected`, `static-inference`, `emulated`, `hardware-observed`, and `independently-reproduced`. Attach scope to every label: a stock-function fixture may be emulated correctly while the larger AF pipeline remains unknown. A reviewer can reject a claim that exceeds its evidence without blocking unrelated work.
