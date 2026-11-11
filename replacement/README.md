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

## Lens transport

[`lens_transport/`](lens_transport/) contains the allocation-free lens wire engine, bounded packet codecs, serial/DMA and timer programming, and firmware publications. Its host scenarios supply wire bytes and timer expirations explicitly. `//replacement/lens_transport:host-tests` covers the public request/completion lifecycle; `//replacement/lens_transport:codec` and `//replacement/lens_transport/native:codec-suite` provide the bounded stock-byte/owned-A32 comparison source.

The native suite takes `RUNNER CACHE OUTPUT IMAGE4 OWNED_CODEC_ELF`. It authenticates the X-M1 1.50 image and selected function extents, runs request constructors to their return with the event-3 publication edge at `0x006bf218` modelled as a synthetic returning interface, and compares parser results, transmitted bytes and queue records. It does not establish physical timing, endpoint availability or whole-actor qualification. The firmware archive requires the owned Power critical-section/resource-route and RTOS IRQ providers, plus exclusive reservation of serial 2, DMA 2/3, timer 11, IRQs 31/94 and their scratch windows. The host and ARM builds, the host behavioral tests and the 462-case native differential run pass with no differences; the observation, cache and report live under a named private scratch root, and nothing links the unit into a firmware root yet.

The task-99 control modules implement the six-state opcode dispatcher, initialization/response CPU and typed cache/lifecycle interfaces. Restricted initialization uses an externally owned shared readiness flag and timer continuations; task99 can process readiness events while the restricted wait is pending. Timer deliveries retain their allocation identity. Cross-task clients use synchronized coherent publications, and completion effects are exposed after state publication. The Vendor `lens_control_runtime` adapter translates queue-98 command, calibration-read and capability-write transactions into task99 events. `control_root` owns the native task: `Task99Reservations` rejects duplicate timer slots and `Task99Root::initialize` rejects a queue or timer that disagrees with its reservation, then creates queue 99 and stores the actor, outbox and providers before task activation, `xm1_lens_task99_run` enters the synchronous receive/dispatch loop, and `Task99Ingress::try_post` claims only the reserved queue for Power's `LensQueueIngress` hook. `control_callbacks` admits registered completion addresses against RootPackage code regions and releases each at completion; `LensEffects` routes admitted tokens to those addresses, foreign tokens (iris periodic, Power callbacks) to an owned fallback, task wakeups to `xm1_power_set_task_flags`, key releases through `key_ui_native`'s queue 1/7 service, and diagnostics/detection/GPIO to composition providers. `wire_native::install_irq_handlers` gives the wire and timer vectors one installed owner: the queue-98 runtime in firmware, or the bounded `abi.rs` transport, which installs its own handlers only while initialized and removes them on unregister. `//replacement/lens_transport/native:root-components` supplies the entry object, the admission object and `control-roots.ld` for each RootPackage link; the admission genrule defines `xm1_lens_root_{a,b}_package_admitted` only when every `control-roots.ld` address matches the same symbol in that domain's `carriers.json` entries. Whole-actor ownership still requires the Vendor queue98 startup root (the 6b8f78 context body, detection bind and queue98 canonical state), and both RootPackage compositions binding Power/profile/detection/key providers. Those are software dependencies, not physical-device boundaries. The pinned FULL emulator supplies no stateful R4 cache geometry or maintenance model; native cache admission remains closed. No whole-CPU, hardware-quiescence or physical-device law qualification is implied.

`Task99Client::begin_snapshot` returns an allocation-free `SnapshotRequest` that can be retained between task polls. `try_complete` returns `None` until a matching task99 completion is delivered; dropping the ticket releases its mailbox registration. The asynchronous `snapshot` method uses the same registration and decoding path.

## Develop pipeline

[`develop/`](develop/) holds the develop transaction framework, the image, sensor, processing, output, metadata and font pipeline stages, and the portable calibration and font cores other units mount. `//replacement/develop:develop-candidate` links the whole unit for ARM with no duplicate or undefined symbol. `//replacement/develop:portable-cores-tests` executes the calibration and font cores, including the shaping capacity and classification boundaries; `//replacement/develop:model-catalogue-test` keeps every `model/*.hs` module named by the stage catalogue.

`//replacement/develop:model` builds all 61 Haskell scenario modules under `develop/model/`, one or more per pipeline stage, and `:develop-qualification` links it. `develop-qualification --list IMAGE4 IMAGE5` reports 56 stages and 7,973 cases against 294 stock extents: 5,133 cases name an entry outside the develop stock region `0xe40000..0xeb2000` and 87 name an address inside it that the pinned decomp index has no range row for (all in `operation-predicates`). The 87 become runnable when extents are supplied; the rest need the stock application image mapped. `--stage NAME LIMIT RUNNER ...` builds each case's stock and owned side from the same scenario, admits it through the native case contract and compares the observations.

The stage smoke declares each side's entry call closure rather than every candidate symbol, and derives case ids from the sanitized scenario name with the full name in the provenance. In the 64-case ObjectAllocator smoke, 8 cases pass and 56 fail: the stock side stops at stock application functions in image4 (`0x00723cb8`, `0x007235d8`, `0x00726658`) that the stage harness does not map, and the owned side stops at image4 data and RAM cells (`0x007cad98`, `0x0093e6f0`) that the stage models patch but the harness does not map. Those are harness gaps, not unit differences. The image-geometry border law matches stock `configure_borders` at `0xe4f3b0`: the non-packed right tail starts at `max(next eight-byte boundary, width & ~7)`, and the packed right edge clears twelve-byte groups to `width/12`. The two `transforms` cases that fail to generate use a 96x12 copy packet; stock `hw_register_block_fff6f000_configure` at `0x68d9c0` returns `0xffffffe2` unless width and height are both multiples of eight, so those success-path packets are wrong and the law is right. The low-memory words some scenarios depend on come from one warm-attach observation (`checkpoint-low-prefix` over an SDK checkpoint), not from cold-boot state.

## Power lifecycle

[`power_lifecycle/`](power_lifecycle/) is two crates. `xm1_power_services` (root `src/services.rs`) is the services leaf: OSAL and delay primitives, the timer services, queue construction, resource routes, the serial driver, area 2, standby, the shutdown path with its task 6, 7 and 8 executors, the settings and storage shutdown actors, a typed callback registry and the dispatch of stock's installed-callback words through it. `:services-host-tests` runs its host tests and `:services-arm` builds the ARM rlib.

The application crate (root `src/lib.rs`) holds the mode frontends, audio release, capture task 3 and the recording path (the task 43 machines, scan, profiles, statistics and completion callbacks) over a shared guest address space. `:host-tests` runs its host tests and `:arm-core` builds the ARM rlib. It cannot link: the ARM rlib leaves over a thousand `xm1_power_record_helper_*` symbols undefined that no unit provides, 34 sites call a guest word as a function pointer instead of resolving it through the leaf's registry, and the unit's review findings are open. Nothing links either crate into a firmware image, and the event-flag carriers the application needs from the RTOS platform are not in the platform images.

## Native rewrite qualification

For an applicable native-only increment, run the native SDK and runner suites plus the affected caller's qualification command:

```sh
devenv shell -- env XM1_EMULATOR_SLOTS=1 host-build test --jobs=2 --local_test_jobs=1 \
  //emulation:native-sdk-tests //emulation:native-runner-tests
```

`//emulation:native-sdk-tests` selects the native SDK build rather than unrelated SDK executables and harnesses. Keep `//emulation:fujimod-xm1-tests` and the complete existing qualification at the full-integration boundary. The native pair does not replace family, subsystem, system, control or hardware checks. Reuse compatible completed observations; adopting this tooling alone does not require replaying them.

The current `host-build` automatically reports the kernel-sampled lock holder on first contention and owner changes. Use its PID, workspace/output-base context and existing `command.log` pointer before doing manual process archaeology. Unavailable or raced metadata is not evidence that the lane is free, and elapsed time does not authorize stopping a holder. The [build command guide](../build-support/README.md#commands) owns diagnostic details, resource limits and the real-wrapper checks. `host-build run` releases the build lock before executing its target; emulator targets then take their own slot.

An older topic checkout may still provide an older launcher. Use the current qualified `host-build` executable through `devenv shell` from the worker checkout with its usual output base. Resolve it from current main only when that checkout is ready for Nix evaluation; otherwise use the integration owner's frozen launcher path. Keep the worker's source and working directory; do not change SDK identities merely to adopt admission. Each topic's emulator binary retains its compiled policy. Use the qualified shared wrapper or adopt the current admission modules before requesting the new RAM and swap caps; setting an environment variable does not upgrade an old binary. Current defaults are 12 GiB RAM plus 8 GiB swap for builds and native suites, and 8 GiB RAM plus 8 GiB swap for full-system runs, within the shared 64 GiB combined reservation budget. Keep completed and healthy running cohorts bound to their original scopes.

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
